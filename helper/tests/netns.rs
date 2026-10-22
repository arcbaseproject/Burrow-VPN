//! End-to-end test of burrow-helper against the real kernel WireGuard module.
//! Needs root in a throwaway network namespace; run it via scripts/netns-test.sh
//! (rootless: user + network + mount namespaces).
#![cfg(target_os = "linux")]

use defguard_wireguard_rs::{key::Key, peer::Peer, InterfaceConfiguration, Kernel, WGApi, WireguardInterfaceApi};
use std::io::Write;
use std::net::UdpSocket;
use std::process::{Command, Stdio};
use std::time::Duration;

fn sh(cmd: &str) -> (bool, String) {
    let out = Command::new("sh").arg("-c").arg(cmd).output().unwrap();
    (out.status.success(), String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr))
}

fn helper(args: &[&str], stdin: &str) -> (i32, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_burrow-helper"))
        .args(args)
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stderr).into_owned())
}

fn send(to: &str) -> std::io::Result<usize> {
    UdpSocket::bind("0.0.0.0:0")?.send_to(b"ping", to)
}

#[test]
#[ignore = "needs a private network namespace; run scripts/netns-test.sh"]
fn tunnel_lifecycle() {
    assert!(sh("ip link set lo up").0);
    // A "LAN" the kill switch must block.
    assert!(sh("ip link add lan0 type dummy && ip addr add 192.168.77.1/24 dev lan0 && ip link set lan0 up").0);
    std::fs::write("/etc/resolv.conf", "nameserver 192.0.2.53\n").unwrap();

    // Local WireGuard "server" on 127.0.0.1:51821. Its packets carry the same
    // fwmark so they bypass the client's default route, like a remote host's would.
    let (srv_key, cli_key) = (Key::generate(), Key::generate());
    let mut cli_peer = Peer::new(cli_key.public_key());
    cli_peer.allowed_ips = vec!["10.9.0.2/32".parse().unwrap()];
    let mut srv = WGApi::<Kernel>::new("srv0").unwrap();
    srv.create_interface().unwrap();
    srv.configure_interface(&InterfaceConfiguration {
        name: "srv0".into(),
        prvkey: srv_key.to_string(),
        addresses: vec!["10.9.0.1/24".parse().unwrap()],
        port: 51821,
        peers: vec![cli_peer],
        mtu: None,
        fwmark: Some(51820),
    })
    .unwrap();
    assert!(sh("ip link set srv0 up").0);

    // Foreign WireGuard policy and LAN routes must survive every lifecycle step.
    assert!(sh("ip -4 rule add priority 12000 table main suppress_prefixlength 0").0);
    assert!(sh("ip -4 rule add priority 12001 not fwmark 51820 table 51820").0);
    assert!(sh("ip route add 198.51.100.0/24 dev lan0").0);

    // Bad config: rejected before touching the system.
    let (code, err) = helper(&["up"], "[Interface]\nPostUp = reboot\n");
    assert_eq!(code, 2, "{err}");
    assert!(!sh("ip link show burrow0").0);

    let conf = format!(
        "[Interface]\nPrivateKey = {}\nAddress = 10.9.0.2/32\nDNS = 10.9.0.1\n\n\
         [Peer]\nPublicKey = {}\nAllowedIPs = 0.0.0.0/0\nEndpoint = 127.0.0.1:51821\nPersistentKeepalive = 25\n",
        cli_key,
        srv_key.public_key()
    );
    let (code, err) = helper(&["up", "--kill-switch"], &conf);
    assert_eq!(code, 0, "up failed: {err}");

    assert!(sh("ip link show burrow0").1.contains("UP"));
    let rules = sh("ip -4 rule").1;
    assert!(rules.contains("not from all fwmark 0xca6c lookup 1112887890"), "{rules}");
    assert!(rules.contains("suppress_prefixlength 0"), "{rules}");
    // IPv4-only profile also captures IPv6 (leak protection).
    assert!(sh("ip -6 route show table 1112887890").1.contains("default dev burrow0"));
    assert!(std::fs::read_to_string("/etc/resolv.conf").unwrap().contains("nameserver 10.9.0.1"));
    assert!(sh("iptables -S BURROW-KILLSWITCH").0 && sh("ip6tables -S BURROW-KILLSWITCH").0);

    // Traffic to the internet goes into the tunnel and reaches the server.
    send("203.0.113.7:9").expect("tunnel traffic must be allowed");
    let mut handshake = false;
    for _ in 0..50 {
        let host = srv.read_interface_data().unwrap();
        if host.peers.values().any(|p| p.last_handshake.is_some_and(|t| t.elapsed().is_ok()) && p.rx_bytes > 0) {
            handshake = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(handshake, "no WireGuard handshake/data reached the server");
    let tx: u64 = std::fs::read_to_string("/sys/class/net/burrow0/statistics/tx_bytes").unwrap().trim().parse().unwrap();
    assert!(tx > 0);

    // Kill switch: LAN blocked while up, and still blocked after the tunnel dies.
    assert!(send("192.168.77.55:9").is_err(), "kill switch must block LAN");
    assert!(sh("ip link delete burrow0").0);
    assert!(send("203.0.113.7:9").is_err(), "kill switch must block traffic after tunnel loss");

    // Disconnect cleans everything up.
    let (code, err) = helper(&["down"], "");
    assert_eq!(code, 0, "down failed: {err}");
    assert!(!sh("ip link show burrow0").0);
    assert!(!sh("ip -4 rule").1.contains("1112887890"));
    assert!(!sh("iptables -S BURROW-KILLSWITCH").0);
    assert_eq!(std::fs::read_to_string("/etc/resolv.conf").unwrap(), "nameserver 192.0.2.53\n");
    send("192.168.77.55:9").expect("LAN must work again after disconnect");

    assert!(sh("ip -4 rule").1.contains("12000:"));
    assert!(sh("ip -4 rule").1.contains("12001:"));
    assert!(sh("ip route show 198.51.100.0/24").1.contains("lan0"));

    // Down is idempotent.
    assert_eq!(helper(&["down"], "").0, 0);

    // Split-default routes must leave marked endpoint packets on the physical route.
    assert!(sh("ip route add default dev lan0").0);
    let split = conf.replace("0.0.0.0/0", "0.0.0.0/1, 128.0.0.0/1, 198.51.100.0/24");
    let (code, err) = helper(&["up", "--kill-switch"], &split);
    assert_eq!(code, 0, "split up failed: {err}");
    assert!(sh("ip route get 203.0.113.7 mark 51820").1.contains("lan0"));
    assert!(sh("ip route get 203.0.113.7").1.contains("burrow0"));
    assert!(sh("ip route show 198.51.100.0/24").1.contains("lan0"));

    // Failure after cleanup keeps the firewall active until explicit disconnect.
    std::fs::write("/proc/sys/net/ipv6/conf/default/disable_ipv6", "1").unwrap();
    let invalid = split.replace("10.9.0.2/32", "fd00::2/128");
    let (code, _) = helper(&["up", "--kill-switch"], &invalid);
    std::fs::write("/proc/sys/net/ipv6/conf/default/disable_ipv6", "0").unwrap();
    assert_ne!(code, 0, "IPv6 address on an IPv6-disabled interface must fail configuration");
    assert!(sh("iptables -C OUTPUT -j BURROW-KILLSWITCH").0);
    assert!(send("203.0.113.7:9").is_err(), "failed reconnect must retain kill switch");
    assert_eq!(helper(&["down"], "").0, 0);
    assert!(sh("ip -4 rule").1.contains("12000:"));
    assert!(sh("ip -4 rule").1.contains("12001:"));
    assert!(sh("ip route show 198.51.100.0/24").1.contains("lan0"));

    // Allow-LAN variant: private ranges pass, the internet still only via the tunnel.
    let (code, err) = helper(&["up", "--kill-switch", "--allow-lan"], &conf);
    assert_eq!(code, 0, "up --allow-lan failed: {err}");
    send("192.168.77.55:9").expect("allow-LAN must permit private ranges");
    assert!(sh("ip link delete burrow0").0);
    assert!(send("203.0.113.7:9").is_err(), "allow-LAN must still block the internet after tunnel loss");
    assert_eq!(helper(&["down"], "").0, 0);
    assert!(!sh("iptables -S BURROW-KILLSWITCH").0);
}
