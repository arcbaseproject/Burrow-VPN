#!/bin/sh
# Rootless end-to-end test: runs burrow-helper against the real kernel WireGuard
# module inside private user, network and mount namespaces. The host's network
# and /etc are untouched. Needs the wireguard kernel module (Linux >= 5.6).
set -e
cd "$(dirname "$0")/.."
bin=$(cargo test -p burrow-helper --test netns --no-run --message-format=json 2>/dev/null |
  sed -n 's/.*"executable":"\([^"]*netns[^"]*\)".*/\1/p' | tail -1)
[ -n "$bin" ] || { echo "build failed" >&2; exit 1; }
# Fresh /run (xtables lock), this namespace's /sys, and a private copy of /etc (resolv.conf gets rewritten).
exec unshare -rnm sh -c '
  mount -t tmpfs none /run && mount -t sysfs none /sys
  tmp=$(mktemp -d) && cp -a /etc/. "$tmp" 2>/dev/null; mount --bind "$tmp" /etc
  # A namespace cannot load the legacy iptables modules; use the nft backend if present.
  for t in iptables ip6tables iptables-restore ip6tables-restore; do
    case $t in
      *-restore) nft=${t%-restore}-nft-restore ;;
      *) nft=$t-nft ;;
    esac
    p=$(command -v $t) && n=$(command -v $nft) && mount --bind "$n" "$p"
  done
  exec "$0" --ignored --nocapture --test-threads=1
' "$bin"
