import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";

export type State = "disconnected" | "connecting" | "connected" | "reconnecting" | "disconnecting";

export interface IpInfo {
  ip: string;
  city?: string;
  region?: string;
  country?: string;
  org?: string;
  loc?: string;
}

export interface Status {
  state: State;
  profile_id: string | null;
  connected_at: number | null;
  rx: number;
  tx: number;
  rx_rate: number;
  tx_rate: number;
  stalled: boolean;
  error: string | null;
  ip: IpInfo | null;
  ip_before: IpInfo | null;
  ip_pending: boolean;
}

export interface Profile {
  id: string;
  name: string;
  endpoints: string[];
  full_tunnel: boolean;
  key_file: boolean;
}

export interface Settings {
  auto_connect: boolean;
  auto_reconnect: boolean;
  kill_switch: boolean;
  allow_lan: boolean;
  launch_at_login: boolean;
  ip_lookup: boolean;
  ip_check_exposed: boolean;
  hide_ips: boolean;
  dns: string[];
  theme: "dark" | "light";
  last_profile: string | null;
}

export const api = {
  status: () => invoke<Status>("status"),
  profiles: () => invoke<Profile[]>("profiles"),
  profileConfig: (id: string) => invoke<string>("profile_config", { id }),
  saveProfile: (id: string | null, name: string, text: string) => invoke<Profile>("save_profile", { id, name, text }),
  deleteProfile: (id: string) => invoke<void>("delete_profile", { id }),
  settings: () => invoke<Settings>("settings"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),
  connect: (id: string) => invoke<void>("connect", { id }),
  disconnect: () => invoke<void>("disconnect"),
  refreshIp: () => invoke<void>("refresh_ip"),
};

export interface Sample {
  rx: number;
  tx: number;
}

/** Live status from the backend's 1 Hz monitor, plus a 60 s rate history. */
export function useStatus() {
  const [status, setStatus] = useState<Status | null>(null);
  const [history, setHistory] = useState<Sample[]>([]);
  useEffect(() => {
    api.status().then(setStatus);
    const off = listen<Status>("status", (e) => {
      setStatus(e.payload);
      if (e.payload.state === "connected") {
        setHistory((h) => [...h.slice(-59), { rx: e.payload.rx_rate, tx: e.payload.tx_rate }]);
      } else if (e.payload.state === "disconnected") {
        setHistory([]);
      }
    });
    return () => void off.then((f) => f());
  }, []);
  return { status, history };
}

/** Seconds since `since` (unix secs), ticking once a second. */
export function useElapsed(since: number | null) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    if (!since) return;
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [since]);
  return since ? Math.max(0, Math.floor(now / 1000 - since)) : 0;
}

export function fmtBytes(n: number) {
  const u = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  while (n >= 1024 && i < u.length - 1) {
    n /= 1024;
    i++;
  }
  return `${n.toFixed(i && n < 100 ? 1 : 0)} ${u[i]}`;
}

export const fmtRate = (n: number) => `${fmtBytes(n)}/s`;

export function fmtDuration(s: number) {
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const pad = (x: number) => String(x).padStart(2, "0");
  return `${pad(h)}:${pad(m)}:${pad(s % 60)}`;
}

export function fmtPlace(ip: IpInfo | null) {
  if (!ip) return "";
  return [ip.city, countryName(ip.country)].filter(Boolean).join(", ");
}

const regions = new Intl.DisplayNames(["en"], { type: "region" });
/** "CA" to "Canada"; ipinfo returns ISO country codes. */
export function countryName(code?: string) {
  try {
    return code ? regions.of(code) ?? code : "";
  } catch {
    return code ?? "";
  }
}

const SEEN = "burrow.countries";
/** Exit country last seen per profile, since configs carry no location. */
export function seenCountries(): Record<string, string> {
  try {
    return JSON.parse(localStorage.getItem(SEEN) ?? "{}");
  } catch {
    return {};
  }
}
export function rememberCountry(id: string, code: string) {
  try {
    localStorage.setItem(SEEN, JSON.stringify({ ...seenCountries(), [id]: code }));
  } catch {
    // shortcut: storage blocked means grouping falls back to the name prefix
  }
}

/** Country code for a profile: its last exit, else a provider-style prefix ("se-got-wg-001", "CH-12") on its name or endpoint. */
export function profileCountry(p: Profile, seen = seenCountries()): string | undefined {
  if (seen[p.id]) return seen[p.id];
  for (const s of [p.name, p.endpoints[0] ?? ""]) {
    const m = /^(?:mlvd-|wg-)?([a-z]{2})[-_#]/i.exec(s);
    const code = m && (m[1].toUpperCase() === "UK" ? "GB" : m[1].toUpperCase());
    const name = code && countryName(code);
    if (code && name && name !== code && name !== "Unknown Region") return code;
  }
}

/** Keeps the shape of the address, hides every digit. */
export const maskIp = (ip: string) => ip.replace(/[0-9a-f]/gi, "•");
