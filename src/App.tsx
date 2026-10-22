import { useCallback, useEffect, useState, type ReactNode } from "react";
import { api, maskIp, Profile, rememberCountry, Settings, useStatus } from "./api";
import Dashboard from "./screens/Dashboard";
import Profiles from "./screens/Profiles";
import Stats from "./screens/Stats";
import SettingsScreen from "./screens/Settings";
import About from "./screens/About";

export type Screen = "dashboard" | "profiles" | "stats" | "settings" | "about";

const titles: Record<Exclude<Screen, "dashboard">, string> = {
  profiles: "Servers",
  stats: "Statistics",
  settings: "Settings",
  about: "About",
};
// Where the back button goes from each page.
const parent: Partial<Record<Screen, Screen>> = { about: "settings" };

export const icons = {
  back: "M15 5l-7 7 7 7",
  stats: "M5 20V11M12 20V5M19 20v-6",
  settings: "M4 7h10M18 7h2M4 17h2M10 17h10M14 4.5v5M8 14.5v5",
  chevron: "M9 6l6 6-6 6",
  power: "M12 3v8M6.3 7.3a8 8 0 1 0 11.4 0",
};

export function Icon({ d, className = "size-5" }: { d: string; className?: string }) {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round" className={className} aria-hidden>
      <path d={d} />
    </svg>
  );
}

/** An IP address that stays masked until the viewer asks to see it. */
export function Ip({ ip, hide, className = "" }: { ip: string; hide: boolean; className?: string }) {
  const [shown, setShown] = useState(false);
  const masked = hide && !shown;
  return (
    <span className="inline-flex items-baseline gap-3">
      <span className={className} aria-label={masked ? "IP address hidden" : undefined}>
        {masked ? maskIp(ip) : ip}
      </span>
      {hide && (
        <button onClick={() => setShown(!shown)} className="text-sm font-medium underline underline-offset-4">
          {shown ? "Hide" : "Show"}
        </button>
      )}
    </span>
  );
}

/** Title bar of a secondary page. */
function PageBar({ title, onBack }: { title: string; onBack: () => void }) {
  return (
    <header className="flex h-12 shrink-0 items-center gap-2 bg-surface px-2">
      <button onClick={onBack} aria-label="Back" className="grid size-9 place-items-center rounded-md hover:bg-surface-2">
        <Icon d={icons.back} />
      </button>
      <h1 className="font-semibold">{title}</h1>
    </header>
  );
}

export default function App() {
  const { status, history } = useStatus();
  const [screen, setScreen] = useState<Screen>(
    (import.meta.env.DEV && (new URLSearchParams(location.search).get("screen") as Screen)) || "dashboard",
  );
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [selected, setSelected] = useState<string | null>(null);

  const reload = useCallback(async () => {
    const [p, s] = await Promise.all([api.profiles(), api.settings()]);
    setProfiles(p);
    setSettings(s);
    setSelected((cur) => (cur && p.some((x) => x.id === cur) ? cur : s.last_profile ?? p[0]?.id ?? null));
  }, []);

  useEffect(() => void reload(), [reload]);
  useEffect(() => {
    document.documentElement.dataset.theme = settings?.theme ?? "dark";
  }, [settings?.theme]);
  // While a tunnel is up, the selection follows the active profile.
  useEffect(() => {
    if (status?.profile_id && status.state !== "disconnected") setSelected(status.profile_id);
  }, [status?.profile_id, status?.state]);
  useEffect(() => {
    if (status?.state === "connected" && status.profile_id && status.ip?.country) rememberCountry(status.profile_id, status.ip.country);
  }, [status?.state, status?.profile_id, status?.ip?.country]);
  // Escape goes back, like a desktop app's secondary view.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setScreen((s) => (s === "dashboard" ? s : parent[s] ?? "dashboard"));
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  if (!status || !settings) return null;

  let page: ReactNode;
  switch (screen) {
    case "dashboard":
      return <Dashboard status={status} profiles={profiles} selected={selected} settings={settings} onSettings={setSettings} goTo={setScreen} />;
    case "profiles":
      page = <Profiles profiles={profiles} status={status} selected={selected} onSelect={(id) => (setSelected(id), setScreen("dashboard"))} reload={reload} />;
      break;
    case "stats":
      page = <Stats status={status} history={history} profiles={profiles} settings={settings} />;
      break;
    case "settings":
      page = <SettingsScreen settings={settings} onChange={setSettings} goTo={setScreen} />;
      break;
    case "about":
      page = <About />;
  }

  return (
    <div className="flex h-full flex-col">
      <PageBar title={titles[screen]} onBack={() => setScreen(parent[screen] ?? "dashboard")} />
      <main className="flex-1 overflow-y-auto">{page}</main>
    </div>
  );
}

export type { ReactNode };
