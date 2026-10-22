<p align="center">
  <img src="logo.png" alt="Burrow logo" width="128">
</p>

<h1 align="center">Burrow</h1>

<p align="center">
  A small, privacy-first WireGuard VPN client for Linux and Windows.<br>
  Built with Tauri 2, Rust and React.
</p>

<p align="center">
  <a href="#get-a-config">Get a config</a> ·
  <a href="#build">Build</a> ·
  <a href="#how-it-works">How it works</a> ·
  <a href="#limitations-honest-list">Limitations</a> ·
  <a href="#license">License</a>
</p>

Burrow does not run a VPN network and does not include servers. It connects to WireGuard servers you already have access to, from a VPN provider or your own VPS (see [Get a config](#get-a-config)). Your public IP changes only while you are connected to a remote server.

## Features

- **One-button connect** with your servers grouped by country, the way commercial VPN apps list them.
- **Kill switch on by default**, plus IPv6 and DNS leak protection.
- **Kernel WireGuard**, with no crypto of its own and no `wireguard-tools` needed on Linux.
- **A small privileged helper** that re-validates every config, so a config can never run commands as root.
- **Keys in the OS keyring**, never shown back in the editor.
- **Live stats**: speeds, totals, session time and your exit location.
- **No telemetry, no analytics, no CDN.** The only outside request is an optional IP lookup, made through the tunnel.

## How it works

```
┌──────────── unprivileged ────────────┐        ┌──────── root / admin ────────┐
│ React UI  ⇄  Tauri backend (Rust)    │ pkexec │ burrow-helper                   │
│  · profiles, settings                │ ─────► │  · strict config re-validation │
│  · secrets in OS keyring             │  UAC   │  · Linux: netlink + iproute2   │
│  · 1 Hz interface counters (sysinfo) │        │  · Windows: wireguard.exe      │
│  · public IP lookup (ipinfo.io)      │        │    /installtunnelservice       │
└──────────────────────────────────────┘        └────────────────────────────────┘
```

- **Encryption** comes from the WireGuard implementation in the OS. On Linux, that is the kernel module (built into every kernel since 5.6). The helper configures it directly over netlink via [`defguard_wireguard_rs`](https://github.com/DefGuard/wireguard-rs) (Apache-2.0) and sets up routing with `ip`, using the same scheme as `wg-quick`. `wireguard-tools` is **not** needed. On Windows, it is the official WireGuard for Windows tunnel service (WireGuardNT). Burrow has no crypto of its own.
- **burrow-helper** (`helper/`) is the only code that runs privileged. It takes only `up` or `down`. For `up`, it reads a WireGuard config, parses it into typed fields, rejects anything unexpected, and writes a freshly rendered config. `PostUp`, `PreDown`, `Table` and other unsupported keys are rejected, so a config can't run commands as root. No shell is involved: every command runs with fixed arguments.
- **Stats** (speeds, totals) come from the tunnel interface's kernel byte counters, sampled once a second. Duration counts from the moment the tunnel came up. Public IP and location come from `https://ipinfo.io/json` over verified TLS. You can turn that lookup off.
- **Secrets**: the full config text, private key included, is stored in the OS credential store: Secret Service (GNOME Keyring or KWallet) on Linux, Credential Manager on Windows. If no credential store is available, Burrow falls back to a `0600` file in the config dir and flags the profile in the UI. Error messages from the parser never include field values.

### Leak protection

| | Linux | Windows |
|---|---|---|
| IPv6 | If a profile routes `0.0.0.0/0` but not `::/0`, Burrow adds `::/0` too. IPv6 then enters the tunnel and the server drops it, so it can't leak around the VPN. | Same. |
| DNS | The profile's `DNS =` goes through `resolvconf` (openresolv or systemd-resolved's shim) when it accepts it. Otherwise the helper moves `/etc/resolv.conf` to `/etc/resolv.conf.burrow-backup`, writes its own, and moves the original back on disconnect. A DHCP client may rewrite the file while you're connected; queries then still travel through the tunnel, but go to that resolver. | WireGuard for Windows sets the tunnel's DNS. With the kill switch on, its firewall blocks DNS on all other interfaces. |
| Kill switch | An `iptables`/`ip6tables` chain `BURROW-KILLSWITCH` rejects all output except via `burrow0`, loopback, local addresses and the tunnel's own marked packets (fwmark 51820). The chain stays in place if the tunnel dies, until you disconnect. It blocks LAN access unless you turn on **Allow local network**, which lets private, link-local and multicast ranges through. | WireGuard for Windows' built-in firewall kill switch. It engages for a single-peer config with a `/0` route. With the kill switch off, Burrow splits `/0` into two `/1` routes, as the official client does. |

### Privacy defaults

- The kill switch is on by default.
- Burrow looks up your public IP only while you're protected, so the request travels through the tunnel. To check while exposed, you have to opt in: that request sends your real IP to ipinfo.io.
- IP addresses show as dots until you press Show, so a screen share or screenshot doesn't leak them.
- The profile editor never receives private or preshared keys. It shows `[hidden]`, and the backend puts the saved key back when you save.
- The typeface ships inside the app. The UI makes no font, CDN or analytics requests.

A VPN moves trust from your network to your VPN provider. The provider sees your real IP and every site you reach, and websites can still fingerprint your browser. For anonymity, use Tor Browser.

## Requirements

**Linux**: kernel 5.6 or newer (WireGuard is built in), `iproute2`, `polkit` (with `pkexec` and a running polkit agent, which every desktop has), and `iptables` for the kill switch. Most distros ship all of these by default. No WireGuard packages are needed. If you're missing any:

```sh
sudo apt install iproute2 iptables policykit-1      # Debian/Ubuntu
sudo dnf install iproute iptables polkit            # Fedora
sudo pacman -S iproute2 iptables polkit             # Arch
sudo xbps-install iproute2 iptables polkit          # Void
```

**Windows 10/11**: install [WireGuard for Windows](https://www.wireguard.com/install/). Burrow drives its signed tunnel service rather than shipping a kernel driver.

## Build

Prerequisites: Rust (stable), Node 20+, and the Tauri 2 system dependencies for your OS (<https://v2.tauri.app/start/prerequisites/>). On Linux, those are the WebKitGTK 4.1 dev packages:

```sh
sudo apt install libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev build-essential   # Debian/Ubuntu
sudo dnf install webkit2gtk4.1-devel libappindicator-gtk3-devel librsvg2-devel                       # Fedora
sudo pacman -S webkit2gtk-4.1 libappindicator-gtk3 librsvg                                           # Arch
sudo xbps-install libwebkit2gtk41-devel libayatana-appindicator-devel librsvg-devel                  # Void
```

Then:

```sh
npm install
npm run tauri dev       # development (builds burrow-helper first)
npm run tauri build     # release bundles
```

Bundles land in `target/release/bundle/`. On Linux, that's `.deb`, `.rpm` and `.AppImage`. On Windows (build on Windows), it's an NSIS `-setup.exe`. `scripts/build-helper.mjs` builds `burrow-helper` and places it as a Tauri sidecar, installed next to the main binary.

The `.deb` and `.rpm` install a polkit action (`packaging/linux/app.burrow.helper.policy`) for `/usr/bin/burrow-helper`. With it, the password prompt says what it's for and stays cached for a few minutes (`auth_admin_keep`). For password-free connects, you can change `allow_active` to `yes`. Any locally logged-in user could then bring up a tunnel, but configs still go through the validator. Without the policy (AppImage, dev builds), pkexec falls back to its generic admin prompt.

## Use

1. **Servers → Import .conf file** with a WireGuard config, or **Paste config** to type one in. The paste screen has templates for Mullvad and Proton VPN. Invalid configs are rejected with the line number.
2. **Servers** lists your servers by country. Burrow reads the country from provider-style names (`se-got-wg-001`, `CH-12`), then remembers the real exit country after you first connect.
3. **Home**: pick the server and press the button. You'll get one admin prompt (polkit or UAC).
4. **Settings**: connect on startup, auto-reconnect, kill switch, launch at login (starts minimized to the tray), DNS override, public IP lookup, and theme.

Closing the window hides Burrow to the tray. The tunnel is a system-level interface: it stays up if you quit Burrow, and Burrow adopts it on the next start.

## Get a config

Every WireGuard config holds its own private key, registered with one server, so Burrow can't ship working configs. Get one of these:

- **A VPN provider.** Proton VPN (free plan available) and Mullvad both have a WireGuard configuration page in your account. Download a `.conf` per location and import them all.
- **Your own server**, with the script below.

Don't use configs that strangers share online. Whoever made one holds its private key and runs its server.

### Your own server

`server/wg-server.sh` sets up a minimal WireGuard server on a Debian, Ubuntu or Fedora VPS that you control:

```sh
scp server/wg-server.sh root@your-vps:
ssh root@your-vps
./wg-server.sh init          # installs WireGuard, enables forwarding + NAT, starts wg-quick@wg0
./wg-server.sh add laptop    # prints a client config; save it as laptop.conf and import it in Burrow
```

Also open UDP 51820 in your cloud provider's firewall. Delete `/etc/wireguard/client-*.conf` from the server after importing.

## Limitations (honest list)

- **Admin prompts.** Every connect and disconnect needs root or admin rights, so you get a polkit or UAC prompt. On Linux, polkit caches it for a few minutes. On Windows, each one is a UAC prompt. Auto-reconnect therefore prompts as well. On Windows, reconnect rarely triggers: WireGuard's service keeps the tunnel up across network changes by itself.
- **Dead-server detection is a heuristic.** Unprivileged code can't read WireGuard handshake times. Burrow flags a server as unreachable when the tunnel sends but receives nothing for 2 minutes. Burrow reconnects when the interface disappears.
- **"Server location"** is the geolocation of your exit IP (ipinfo.io), shown only after you connect. Configs carry no location info, so the country grouping guesses from the name until then, and a name like `my-server` may land under Malaysia.
- **One tunnel at a time.** The interface is `burrow0` on Linux and `burrow` on Windows.
- **Linux kill switch uses iptables.** It works with both iptables-legacy and iptables-nft. Pure-nftables systems without the `iptables` command are not supported.
- **Windows error details.** UAC-elevated processes can't pipe output back, so Windows errors are coarse ("WireGuard failed to start the tunnel"). Check the WireGuard for Windows log for details.

## Testing

### Executed (this repo, on Void Linux x86_64)

| Test | Result |
|---|---|
| `cargo test -p burrow-helper`: config parsing, round-trip, hook/unknown-key rejection, invalid keys/CIDRs/endpoints/DNS, no secret echo in errors, IPv6 hardening, Windows `/0` split, DNS override, key masking in the editor | 5 tests pass |
| `burrow-helper` CLI unprivileged: refuses with exit 6; bad args exit 2 | pass |
| `scripts/netns-test.sh`: real kernel WireGuard tunnel to a local WireGuard peer, in rootless user/net/mount namespaces. Covers: bad config rejected before any change; `up --kill-switch`; interface up; wg-quick-style fwmark rules; IPv6 default captured for an IPv4-only profile; resolv.conf swapped; WireGuard handshake and encrypted data reaching the peer; kill switch blocks LAN while up and all traffic after the tunnel is deleted; `down` removes interface, rules and firewall chain, restores resolv.conf, LAN works again; second `down` is a no-op; the allow-LAN variant lets private ranges through and still blocks the internet after tunnel loss | pass |
| `cargo check --target x86_64-pc-windows-msvc -p burrow -p burrow-helper` (whole app incl. Windows UAC/ShellExecuteEx code) | compiles, no warnings* |
| Linux `pkexec` invocation path compiled and run standalone | pass |
| `npm run build` (TypeScript strict + Vite) | pass |
| `bash -n` + `shellcheck` on `server/wg-server.sh` | clean |

\* Ran with TLS temporarily disabled in `ureq`, because cross-compiling `ring` needs MSVC headers. The API surface is identical.

Run the namespace test yourself with `./scripts/netns-test.sh`. It needs no root, and your real network and `/etc` are untouched.

### Not yet executed: needs a real host and a remote server

Run these on Ubuntu, Fedora, Arch and Windows 10/11. Use a server from `server/wg-server.sh`.

| Test | How |
|---|---|
| Full Linux app build and bundles | `npm run tauri build` on a machine with WebKitGTK dev packages |
| Windows build and installer | `npm run tauri build` on Windows |
| Successful connect / IP change | Connect. The dashboard IP should change to the server's IP. "Before" shows the old one. Check with `curl https://ifconfig.me`. |
| Failed connect | Import a config with a wrong `PublicKey`. Connect succeeds locally, then "No data from the server" appears after 2 min. Cancel the auth prompt: you get an error and stay disconnected. |
| Invalid configs | Import a file with `PostUp =`, a bad key, or a missing `[Peer]`. Each is rejected with a line number. |
| DNS | Connected: `resolvectl status burrow0` or `cat /etc/resolv.conf` shows the profile's DNS. Run <https://dnsleaktest.com>. |
| IPv4/IPv6 | With an IPv4-only profile: `curl -6 https://ifconfig.co` must fail (no IPv6 leak). With `::/0` and a v6 server: it shows the server's IPv6. |
| Kill switch | Enable it, connect, then `sudo ip link del burrow0`. `curl https://ifconfig.me` must fail. Disconnect: traffic flows, and `sudo iptables -S BURROW-KILLSWITCH` no longer exists. |
| Reconnect | With auto-reconnect on, `sudo ip link del burrow0`. The state goes to Reconnecting, then Connected (after one auth prompt). |
| Disconnect cleanup | Covered by the namespace test above. On a real host, also check that NetworkManager/dhcpcd keep working after disconnect. |
| Restart | Quit Burrow while connected, then relaunch. It shows Connected with the original duration. |
| Windows tunnel | `Get-Service WireGuardTunnel$burrow` is running while connected and gone after disconnect. |

## License

Burrow is released under the [MIT License](LICENSE).

WireGuard is a registered trademark of Jason A. Donenfeld. Burrow isn't affiliated with the WireGuard project. The bundled Archivo typeface by Omnibus-Type is under the SIL Open Font License.
