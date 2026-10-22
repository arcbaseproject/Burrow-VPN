//! burrow-helper: the only part of Burrow that runs with root/admin rights.
//!
//!   burrow-helper up [--kill-switch [--allow-lan]]          Linux: config on stdin (via pkexec)
//!   burrow-helper up [--kill-switch] <path>                 Windows: config file (via UAC)
//!   burrow-helper down
//!
//! It accepts nothing but a WireGuard config, validates it with the strict
//! parser, re-renders it, and hands it to the OS-native WireGuard tooling.
//! Exit codes: 0 ok, 2 bad input, 3 missing dependency, 4 tool failed, 5 I/O, 6 not privileged.

use std::process::{Command, ExitCode};
use burrow_helper::{parse, Config, MAX_CONFIG_BYTES};

type R = Result<(), (u8, String)>;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |f: &str| args.iter().any(|a| a == f);
    let (kill, lan) = (flag("--kill-switch"), flag("--allow-lan"));
    let rest: Vec<&String> = args.iter().skip(1).filter(|a| !a.starts_with("--")).collect();
    let res = match (args.first().map(String::as_str), rest.as_slice()) {
        (Some("up"), r) => privileged().and_then(|_| read_config(r)).and_then(|c| os::up(c, kill, lan)),
        (Some("down"), []) => privileged().and_then(|_| os::down()),
        _ => Err((2, "usage: burrow-helper up [--kill-switch [--allow-lan]] [config] | down".into())),
    };
    match res {
        Ok(()) => ExitCode::SUCCESS,
        Err((code, msg)) => {
            eprintln!("{msg}");
            ExitCode::from(code)
        }
    }
}

fn read_config(rest: &[&String]) -> Result<Config, (u8, String)> {
    use std::io::Read;
    let mut text = String::new();
    let res = match rest {
        [] if cfg!(unix) => std::io::stdin().take(MAX_CONFIG_BYTES as u64 + 1).read_to_string(&mut text),
        [path] if cfg!(windows) => std::fs::File::open(path)
            .and_then(|f| f.take(MAX_CONFIG_BYTES as u64 + 1).read_to_string(&mut text)),
        _ => return Err((2, "bad arguments".into())),
    };
    res.map_err(|e| (5, format!("reading config: {e}")))?;
    parse(&text).map_err(|e| (2, format!("invalid config: {e}")))
}

fn run(cmd: &str, args: &[&str]) -> R {
    let out = Command::new(cmd)
        .args(args)
        .env("PATH", os::SAFE_PATH)
        .output()
        .map_err(|e| (3, format!("cannot run {cmd}: {e}")))?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let tail: Vec<&str> = err.lines().rev().take(4).collect();
    Err((4, format!("{cmd} failed: {}", tail.into_iter().rev().collect::<Vec<_>>().join("\n"))))
}

#[cfg(unix)]
fn privileged() -> R {
    use std::os::unix::fs::MetadataExt;
    // /proc/self is owned by the effective uid.
    match std::fs::metadata("/proc/self") {
        Ok(m) if m.uid() == 0 => Ok(()),
        _ => Err((6, "burrow-helper must run as root (via pkexec)".into())),
    }
}

#[cfg(windows)]
fn privileged() -> R {
    // wireguard.exe enforces elevation itself and fails cleanly without it.
    Ok(())
}

#[cfg(target_os = "linux")]
mod os {
    //! Talks to the kernel WireGuard module over netlink (no wireguard-tools
    //! needed) and replicates wg-quick's routing with iproute2.
    use super::{run, Config, R};
    use defguard_wireguard_rs::{
        key::Key, net::IpAddrMask, peer::Peer, InterfaceConfiguration, Kernel, WGApi, WireguardInterfaceApi,
    };
    use std::io::Write;
    use std::net::{IpAddr, ToSocketAddrs};
    use std::path::Path;
    use std::process::{Command, Stdio};
    use burrow_helper::FWMARK;

    pub const SAFE_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
    const IFACE: &str = "burrow0";
    const TABLE: &str = "1112887890";
    const MAIN_PRIORITY: &str = "11000";
    const TUNNEL_PRIORITY: &str = "11001";
    const CHAIN: &str = "BURROW-KILLSWITCH";
    const RESOLV: &str = "/etc/resolv.conf";
    const RESOLV_BACKUP: &str = "/etc/resolv.conf.burrow-backup";

    fn have(cmd: &str) -> bool {
        SAFE_PATH.split(':').any(|d| Path::new(d).join(cmd).exists())
    }

    fn ipv6() -> bool {
        Path::new("/proc/net/if_inet6").exists()
    }

    fn bad(what: &str) -> (u8, String) {
        (2, format!("invalid {what}"))
    }

    pub fn up(mut cfg: Config, kill: bool, lan: bool) -> R {
        cfg.harden(kill, false);
        if !have("ip") {
            return Err((3, "ip not found: install iproute2".into()));
        }
        if kill && !(have("iptables") && have("ip6tables")) {
            return Err((3, "kill switch needs iptables and ip6tables".into()));
        }
        // Resolve endpoint hostnames before any routes change.
        let peers = cfg.peers.iter().map(peer).collect::<Result<Vec<_>, _>>()?;
        if kill {
            killswitch_on(lan)?;
        }
        cleanup()?;
        configure(&cfg, peers).inspect_err(|_| {
            let _ = cleanup();
        }).map(|_| {
            if !kill {
                killswitch_off();
            }
        })
    }

    fn peer(p: &burrow_helper::Peer) -> Result<Peer, (u8, String)> {
        let key = |k: &str| Key::try_from(k).map_err(|_| bad("key"));
        let mut out = Peer::new(key(&p.public_key)?);
        out.preshared_key = p.preshared_key.as_deref().map(key).transpose()?;
        out.persistent_keepalive_interval = p.persistent_keepalive.filter(|k| *k > 0);
        out.allowed_ips = p.allowed_ips.iter().map(|c| c.parse::<IpAddrMask>().map_err(|_| bad("AllowedIPs"))).collect::<Result<_, _>>()?;
        if let Some(e) = &p.endpoint {
            let addr = e.to_socket_addrs().ok().and_then(|mut a| a.next());
            out.endpoint = Some(addr.ok_or_else(|| (4, format!("cannot resolve endpoint {e}")))?);
        }
        Ok(out)
    }

    fn configure(cfg: &Config, peers: Vec<Peer>) -> R {
        let wg_err = |e: defguard_wireguard_rs::error::WireguardInterfaceError| (4, format!("WireGuard: {e}"));
        let mut wg = WGApi::<Kernel>::new(IFACE).map_err(wg_err)?;
        wg.create_interface().map_err(wg_err)?;
        let addresses = cfg.interface.addresses.iter().map(|a| a.parse().map_err(|_| bad("Address"))).collect::<Result<_, _>>()?;
        wg.configure_interface(&InterfaceConfiguration {
            name: IFACE.into(),
            prvkey: cfg.interface.private_key.clone(),
            addresses,
            port: cfg.interface.listen_port.unwrap_or(0),
            peers,
            mtu: Some(cfg.interface.mtu.unwrap_or(1420).into()),
            fwmark: Some(FWMARK),
        })
        .map_err(wg_err)?;
        run("ip", &["link", "set", "up", "dev", IFACE])?;
        routes(cfg)?;
        if !cfg.interface.dns.is_empty() {
            dns_set(&cfg.interface.dns)?;
        }
        Ok(())
    }

    /// Like wg-quick, exclude marked endpoint packets from tunnel routing and
    /// preserve more-specific main routes for a default-route profile. Keep all
    /// tunnel ranges in Burrow's table so host routes never need replacement.
    fn routes(cfg: &Config) -> R {
        let mark = FWMARK.to_string();
        let mut default = [false, false];
        for c in cfg.peers.iter().flat_map(|p| &p.allowed_ips) {
            let v6 = c.contains(':');
            if v6 && !ipv6() {
                continue; // IPv6 disabled on this host: nothing can leak over it.
            }
            let fam = if v6 { "-6" } else { "-4" };
            run("ip", &[fam, "route", "replace", c, "dev", IFACE, "table", TABLE])?;
            default[v6 as usize] |= c == "0.0.0.0/0" || c == "::/0";
        }
        for (fam, on) in ["-4", "-6"].iter().zip(default) {
            if on {
                run("ip", &[fam, "rule", "add", "priority", MAIN_PRIORITY, "table", "main", "suppress_prefixlength", "0"])?;
            }
            if *fam == "-4" || ipv6() {
                run("ip", &[fam, "rule", "add", "priority", TUNNEL_PRIORITY, "not", "fwmark", &mark, "table", TABLE])?;
            }
        }
        if default[0] {
            // Lets replies to the tunnel's marked packets pass reverse-path filtering.
            std::fs::write("/proc/sys/net/ipv4/conf/all/src_valid_mark", "1").map_err(|e| (5, format!("src_valid_mark: {e}")))?;
        }
        Ok(())
    }

    /// Uses resolvconf when present; otherwise swaps /etc/resolv.conf and
    /// restores the original on disconnect (rename keeps a symlink intact).
    fn dns_set(dns: &[String]) -> R {
        let (servers, search): (Vec<&String>, Vec<&String>) = dns.iter().partition(|d| d.parse::<IpAddr>().is_ok());
        let mut text = String::from("# Set by Burrow VPN, restored on disconnect\n");
        for s in servers {
            text += &format!("nameserver {s}\n");
        }
        if !search.is_empty() {
            text += &format!("search {}\n", search.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" "));
        }
        if have("resolvconf") && resolvconf_add(&text) {
            return Ok(());
        }
        if !Path::new(RESOLV_BACKUP).exists() && std::fs::symlink_metadata(RESOLV).is_ok() {
            std::fs::rename(RESOLV, RESOLV_BACKUP).map_err(|e| (5, format!("{RESOLV}: {e}")))?;
        }
        std::fs::write(RESOLV, text).map_err(|e| (5, format!("{RESOLV}: {e}")))
    }

    /// False if resolvconf refuses, e.g. when resolv.conf isn't managed by it.
    fn resolvconf_add(text: &str) -> bool {
        let child = Command::new("resolvconf")
            .args(["-a", &format!("tun.{IFACE}"), "-m", "0", "-x"])
            .env("PATH", SAFE_PATH)
            .stdin(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let Ok(mut child) = child else { return false };
        let wrote = child.stdin.take().unwrap().write_all(text.as_bytes()).is_ok();
        child.wait().is_ok_and(|s| s.success()) && wrote
    }

    fn dns_clear() {
        if std::fs::symlink_metadata(RESOLV_BACKUP).is_ok() {
            let _ = std::fs::rename(RESOLV_BACKUP, RESOLV);
        } else if have("resolvconf") {
            let _ = run("resolvconf", &["-d", &format!("tun.{IFACE}"), "-f"]);
        }
    }

    /// Idempotent: also cleans up after a tunnel that vanished or a crash.
    pub fn down() -> R {
        cleanup()?;
        killswitch_off();
        Ok(())
    }

    fn cleanup() -> R {
        if Path::new("/sys/class/net").join(IFACE).exists() {
            // Deleting the link also drops its addresses and routes.
            run("ip", &["link", "delete", "dev", IFACE])?;
        }
        let mark = FWMARK.to_string();
        for fam in ["-4", "-6"] {
            while run("ip", &[fam, "rule", "delete", "priority", TUNNEL_PRIORITY, "not", "fwmark", &mark, "table", TABLE]).is_ok() {}
            while run("ip", &[fam, "rule", "delete", "priority", MAIN_PRIORITY, "table", "main", "suppress_prefixlength", "0"]).is_ok() {}
        }
        dns_clear();
        Ok(())
    }

    /// Reject all outgoing traffic except: via the tunnel, loopback, the
    /// tunnel's own encrypted packets (fwmark), and local addresses.
    /// Stays in place if the tunnel dies, until `down` runs.
    /// `lan` also lets through private, link-local and multicast ranges.
    fn killswitch_on(lan: bool) -> R {
        let mark = FWMARK.to_string();
        let ranges: [(&str, &[&str]); 2] = [
            ("iptables", &["10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16", "169.254.0.0/16", "224.0.0.0/4"]),
            ("ip6tables", &["fe80::/10", "fc00::/7", "ff00::/8"]),
        ];
        for (ipt, private) in ranges {
            let _ = run(ipt, &["-N", CHAIN]);
            let mut rules = format!("*filter\n-F {CHAIN}\n-A {CHAIN} -o {IFACE} -j RETURN\n-A {CHAIN} -o lo -j RETURN\n-A {CHAIN} -m mark --mark {mark} -j RETURN\n-A {CHAIN} -m addrtype --dst-type LOCAL -j RETURN\n");
            for net in private.iter().filter(|_| lan) {
                rules += &format!("-A {CHAIN} -d {net} -j RETURN\n");
            }
            rules += &format!("-A {CHAIN} -j REJECT\nCOMMIT\n");
            let restore = format!("{ipt}-restore");
            let mut child = Command::new(&restore)
                .arg("--noflush")
                .env("PATH", SAFE_PATH)
                .stdin(Stdio::piped())
                .spawn().map_err(|e| (3, format!("cannot run {restore}: {e}")))?;
            child.stdin.take().unwrap().write_all(rules.as_bytes())
                .map_err(|e| (5, format!("{restore}: {e}")))?;
            if !child.wait().map_err(|e| (5, e.to_string()))?.success() {
                return Err((4, format!("{restore} failed")));
            }
            if run(ipt, &["-C", "OUTPUT", "-j", CHAIN]).is_err() {
                run(ipt, &["-I", "OUTPUT", "-j", CHAIN])?;
            }
        }
        Ok(())
    }

    fn killswitch_off() {
        for ipt in ["iptables", "ip6tables"] {
            if !have(ipt) {
                continue;
            }
            while run(ipt, &["-D", "OUTPUT", "-j", CHAIN]).is_ok() {}
            let _ = run(ipt, &["-F", CHAIN]);
            let _ = run(ipt, &["-X", CHAIN]);
        }
    }
}

#[cfg(windows)]
mod os {
    use super::{run, Config, R};
    use std::path::PathBuf;

    pub const SAFE_PATH: &str = r"C:\Windows\System32";
    const TUNNEL: &str = "burrow";

    fn wireguard_exe() -> PathBuf {
        let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
        PathBuf::from(pf).join("WireGuard").join("wireguard.exe")
    }

    fn conf_dir() -> PathBuf {
        let pd = std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into());
        PathBuf::from(pd).join("Burrow")
    }

    pub fn up(mut cfg: Config, kill: bool, _lan: bool) -> R {
        cfg.harden(kill, true);
        let wg = wireguard_exe();
        if !wg.exists() {
            return Err((3, "WireGuard for Windows is not installed".into()));
        }
        let _ = down();
        let dir = conf_dir();
        std::fs::create_dir_all(&dir).map_err(|e| (5, e.to_string()))?;
        // Only SYSTEM and Administrators may read the private key.
        let d = dir.to_string_lossy();
        run("icacls", &[&d, "/inheritance:r", "/grant:r", "*S-1-5-18:(OI)(CI)F", "*S-1-5-32-544:(OI)(CI)F"])?;
        let conf = dir.join(format!("{TUNNEL}.conf"));
        std::fs::write(&conf, cfg.render(None)).map_err(|e| (5, e.to_string()))?;
        run(&wg.to_string_lossy(), &["/installtunnelservice", &conf.to_string_lossy()]).inspect_err(|_| {
            let _ = std::fs::remove_file(&conf);
        })
    }

    pub fn down() -> R {
        let wg = wireguard_exe();
        let res = if wg.exists() { run(&wg.to_string_lossy(), &["/uninstalltunnelservice", TUNNEL]) } else { Ok(()) };
        let _ = std::fs::remove_file(conf_dir().join(format!("{TUNNEL}.conf")));
        res
    }
}
