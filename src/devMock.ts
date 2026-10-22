// Dev-only fake backend for previewing the UI in a plain browser.
// Query: ?state=disconnected|connecting|connected&theme=light&exposedIp=1&screen=stats
import { mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import type { Profile, Settings, Status } from "./api";

const q = new URLSearchParams(location.search);
const state = (q.get("state") ?? "disconnected") as Status["state"];
const now = Math.floor(Date.now() / 1000);

const profiles: Profile[] = [
  { id: "a", name: "Cloudflare WARP", endpoints: ["engage.cloudflareclient.com:2408"], full_tunnel: true, key_file: false },
  { id: "b", name: "Home VPS, Frankfurt", endpoints: ["203.0.113.41:51820"], full_tunnel: true, key_file: false },
];
let settings: Settings = {
  auto_connect: false, auto_reconnect: true, kill_switch: true, allow_lan: false, launch_at_login: false,
  ip_lookup: true, ip_check_exposed: q.has("exposedIp"), hide_ips: !q.has("showIps"), dns: [], theme: (q.get("theme") as "light") ?? "dark", last_profile: "a",
};
const status: Status = {
  state, profile_id: state === "disconnected" ? null : "a", connected_at: now - 754, rx: 48_211_000, tx: 3_904_000,
  rx_rate: 0, tx_rate: 0, stalled: false, error: null,
  ip: state === "connected" ? { ip: "104.28.214.37", city: "Toronto", country: "CA", org: "AS13335 Cloudflare", loc: "43.7001,-79.4163" }
    : state === "disconnected" && q.has("exposedIp") ? { ip: "65.87.198.12", city: "Calgary", country: "CA", loc: "51.0501,-114.0853" } : null,
  ip_before: state === "connected" ? { ip: "65.87.198.12" } : null,
  ip_pending: false,
};

mockIPC(
  (cmd, args) => {
    const a = args as Record<string, unknown>;
    switch (cmd) {
      case "status": return status;
      case "profiles": return profiles;
      case "settings": return settings;
      case "save_settings": settings = a.settings as Settings; return null;
      case "profile_config": return "[Interface]\nPrivateKey = [hidden]\nAddress = 172.16.0.2/32\nDNS = 1.1.1.1\n\n[Peer]\nPublicKey = bmXOC+F1FxEMF9dyiK2H5/1SUtzH0JuVo51h2wPfgyo=\nAllowedIPs = 0.0.0.0/0, ::/0\nEndpoint = engage.cloudflareclient.com:2408\n";
      default: return null;
    }
  },
  { shouldMockEvents: true },
);

if (state === "connected") {
  setInterval(() => {
    status.rx_rate = 200_000 + Math.random() * 1_400_000;
    status.tx_rate = 20_000 + Math.random() * 90_000;
    status.rx += status.rx_rate;
    status.tx += status.tx_rate;
    emit("status", { ...status });
  }, 250);
}
