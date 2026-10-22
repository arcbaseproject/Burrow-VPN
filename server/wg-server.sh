#!/usr/bin/env bash
# Minimal WireGuard server for a VPS you control (Debian/Ubuntu/Fedora).
#
#   sudo ./wg-server.sh init            install WireGuard, create server, enable NAT
#   sudo ./wg-server.sh add <name>      create a client and print its .conf
#
# Env: WG_PORT (default 51820), WG_DNS (default 1.1.1.1, 2606:4700:4700::1111),
#      WG_HOST (public IP or DNS name; default autodetected).
set -euo pipefail

PORT=${WG_PORT:-51820}
DNS=${WG_DNS:-"1.1.1.1, 2606:4700:4700::1111"}
NET4=10.66.0
NET6=fd66::
DIR=/etc/wireguard
CONF=$DIR/wg0.conf

die() { echo "error: $*" >&2; exit 1; }
[[ $EUID -eq 0 ]] || die "run as root"

init() {
  [[ -e $CONF ]] && die "$CONF already exists"
  if command -v apt-get >/dev/null; then apt-get update && apt-get install -y wireguard iptables qrencode
  elif command -v dnf >/dev/null; then dnf install -y wireguard-tools iptables qrencode
  else die "install wireguard-tools and iptables manually"; fi

  local wan; wan=$(ip -4 route show default | awk '{print $5; exit}')
  [[ -n $wan ]] || die "no default route"
  umask 077
  local key; key=$(wg genkey)
  cat > "$CONF" <<EOF
[Interface]
Address = $NET4.1/24, ${NET6}1/64
ListenPort = $PORT
PrivateKey = $key
PostUp = iptables -t nat -A POSTROUTING -o $wan -j MASQUERADE; ip6tables -t nat -A POSTROUTING -o $wan -j MASQUERADE || true
PostDown = iptables -t nat -D POSTROUTING -o $wan -j MASQUERADE; ip6tables -t nat -D POSTROUTING -o $wan -j MASQUERADE || true
EOF
  printf 'net.ipv4.ip_forward=1\nnet.ipv6.conf.all.forwarding=1\n' > /etc/sysctl.d/99-wireguard.conf
  sysctl --system >/dev/null
  systemctl enable --now wg-quick@wg0
  command -v ufw >/dev/null && ufw allow "$PORT"/udp || true
  command -v firewall-cmd >/dev/null && firewall-cmd --permanent --add-port="$PORT"/udp && firewall-cmd --reload || true
  echo "Server up on UDP $PORT. Open that port in your cloud provider's firewall too."
}

add() {
  local name=${1:-}
  [[ $name =~ ^[A-Za-z0-9_-]{1,32}$ ]] || die "client name: letters, digits, _ or -"
  [[ -e $CONF ]] || die "run init first"
  grep -q "^# client $name\$" "$CONF" && die "client $name exists"

  local n=2
  while grep -q "AllowedIPs = $NET4.$n/32" "$CONF"; do n=$((n + 1)); done
  ((n < 255)) || die "subnet full"
  local host=${WG_HOST:-$(curl -4fsS https://ifconfig.me || true)}
  [[ -n $host ]] || die "could not detect public IP; set WG_HOST"
  umask 077
  local key pub psk spub
  key=$(wg genkey); pub=$(wg pubkey <<<"$key"); psk=$(wg genpsk)
  spub=$(wg show wg0 public-key)

  printf '\n# client %s\n[Peer]\nPublicKey = %s\nPresharedKey = %s\nAllowedIPs = %s.%s/32, %s%s/128\n' \
    "$name" "$pub" "$psk" "$NET4" "$n" "$NET6" "$n" >> "$CONF"
  wg syncconf wg0 <(wg-quick strip wg0)

  local out=$DIR/client-$name.conf
  cat > "$out" <<EOF
[Interface]
PrivateKey = $key
Address = $NET4.$n/32, ${NET6}$n/128
DNS = $DNS

[Peer]
PublicKey = $spub
PresharedKey = $psk
AllowedIPs = 0.0.0.0/0, ::/0
Endpoint = $host:$PORT
PersistentKeepalive = 25
EOF
  echo "Client config written to $out (import it in Burrow, then delete it from the server):"
  echo; cat "$out"
}

case ${1:-} in
  init) init ;;
  add) add "${2:-}" ;;
  *) die "usage: $0 init | add <name>" ;;
esac
