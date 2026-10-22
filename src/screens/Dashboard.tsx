import { api, countryName, fmtDuration, fmtRate, Profile, Settings, Status, useElapsed } from "../api";
import { Icon, icons, Ip, type Screen } from "../App";
import { LAND } from "../world";
import { timeZoneLoc } from "../tz";
import logo from "../../logo.png";

const word: Record<Status["state"], string> = {
  disconnected: "Exposed",
  connecting: "Connecting",
  connected: "Protected",
  reconnecting: "Reconnecting",
  disconnecting: "Disconnecting",
};

interface Props {
  status: Status;
  profiles: Profile[];
  selected: string | null;
  settings: Settings;
  onSettings: (s: Settings) => void;
  goTo: (s: Screen) => void;
}

export default function Dashboard({ status, profiles, selected, settings, onSettings, goTo }: Props) {
  const s = status.state;
  const on = s === "connected";
  const busy = s !== "connected" && s !== "disconnected";
  const elapsed = useElapsed(on ? status.connected_at : null);
  const profile = profiles.find((p) => p.id === selected);
  const ip = busy ? null : status.ip;
  // Without a looked-up address, place an exposed user by time zone: offline and approximate.
  const tzLoc = !on && !ip ? timeZoneLoc() : undefined;
  const zoneCity = tzLoc ? Intl.DateTimeFormat().resolvedOptions().timeZone.split("/").pop()!.replaceAll("_", " ") : null;

  let note: string;
  if (busy) note = s === "disconnecting" ? "Closing the tunnel." : "Setting up the tunnel. Your system may ask for your password.";
  else if (on) note = ip ? "Websites see this address." : !settings.ip_lookup ? "IP checks are off." : status.ip_pending ? "Checking your new address." : "Couldn't check your new address.";
  else if (ip) note = "Your real address. Websites can see it.";
  else if (zoneCity) note = "Approximate, from your time zone. Burrow doesn't send your real IP to a lookup service.";
  else note = "Burrow doesn't send your real IP to a lookup service. You can allow it in Settings.";

  const act = () => {
    if (!profile) return goTo("profiles");
    if (s === "disconnected") api.connect(profile.id);
    else if (s === "connected" || s === "reconnecting") api.disconnect();
  };
  const actLabel = !profile ? "Add a server" : s === "disconnected" ? "Connect" : s === "connecting" ? "Connecting" : s === "disconnecting" ? "Disconnecting" : "Disconnect";
  const exposedBar = s === "disconnected";
  const toggleKill = () => {
    const next = { ...settings, kill_switch: !settings.kill_switch };
    api.saveSettings(next).then(() => onSettings(next), () => {});
  };
  const killTip = `Kill switch ${settings.kill_switch ? "on" : "off"}.${s === "disconnected" ? "" : " Applies when you next connect."}`;
  const actTone = busy ? "bg-surface-2 text-ink" : s === "disconnected" ? "bg-shade text-on-shade" : "bg-alert text-ground";

  return (
    <div className="flex h-full flex-col">
      <header className={`flex h-12 shrink-0 items-center gap-1 px-4 transition-colors duration-500 ${s === "disconnected" ? "bg-sodium text-on-sodium" : "bg-shade text-on-shade"}`}>
        <span className="flex flex-1 items-center gap-2">
          <img src={logo} alt="" className="size-7" />
          <span className="t-state text-[17px]">Burrow</span>
        </span>
        <button
          onClick={toggleKill}
          aria-pressed={settings.kill_switch}
          aria-label="Kill switch"
          title={killTip}
          className={`mr-1 grid size-9 place-items-center rounded-full transition-colors duration-200 ${
            settings.kill_switch ? (exposedBar ? "bg-on-sodium text-sodium" : "bg-on-shade text-shade") : "opacity-70 hover:bg-black/10 hover:opacity-100"
          }`}
        >
          <Icon d={icons.power} />
        </button>
        <button onClick={() => goTo("stats")} aria-label="Statistics" className="grid size-9 place-items-center rounded-md hover:bg-black/10">
          <Icon d={icons.stats} />
        </button>
        <button onClick={() => goTo("settings")} aria-label="Settings" className="grid size-9 place-items-center rounded-md hover:bg-black/10">
          <Icon d={icons.settings} />
        </button>
      </header>

      <div className="relative min-h-0 flex-1">
        <WorldMap loc={ip?.loc ?? tzLoc} state={s} />
        <div className="absolute inset-x-0 bottom-0 px-5 pb-4" aria-live="polite">
          <p className={`font-semibold ${on ? "text-safe" : busy ? "text-ink" : "text-warn"}`}>{word[s]}</p>
          <h1 className="t-figure mt-1 truncate text-[40px]">
            {busy ? profile?.name ?? "Server" : ip ? ip.city || countryName(ip.country) || "Unknown place" : on ? "Location unknown" : zoneCity ?? "Location not checked"}
          </h1>
          {ip?.city && <p className="text-[17px]">{countryName(ip.country)}</p>}
          {ip && (
            <div className="mt-2 text-[15px]">
              <Ip ip={ip.ip} hide={settings.hide_ips} className="t-num font-medium" />
            </div>
          )}
          <p className="mt-1 max-w-[38ch] text-sm text-fog">
            {note}
            {on && !ip && settings.ip_lookup && !status.ip_pending && (
              <button onClick={() => api.refreshIp()} className="ml-2 font-medium text-ink underline underline-offset-4">
                Check again
              </button>
            )}
          </p>
          {on && status.ip_before && (
            <p className="mt-1 text-sm text-fog">
              Before: <Ip ip={status.ip_before.ip} hide={settings.hide_ips} className="t-num" />
            </p>
          )}
        </div>
      </div>

      <div className="flex shrink-0 flex-col gap-2 bg-surface px-4 pt-3 pb-4">
        {status.error && <p role="alert" className="pb-1 text-sm text-alert">{status.error}</p>}
        {status.stalled && <p role="status" className="pb-1 text-sm">The server hasn't answered for 2 minutes. It may be down or blocked.</p>}
        {on && (
          <dl className="grid grid-cols-3 pb-1 text-sm">
            <Figure label="Down" value={fmtRate(status.rx_rate)} />
            <Figure label="Up" value={fmtRate(status.tx_rate)} />
            <Figure label="Time" value={fmtDuration(elapsed)} />
          </dl>
        )}
        <button
          onClick={() => goTo("profiles")}
          disabled={s !== "disconnected" && profiles.length > 0}
          className="flex h-11 items-center gap-3 rounded-md bg-surface-2 px-4 text-left hover:brightness-110 disabled:cursor-default disabled:hover:brightness-100"
        >
          <span className="min-w-0 flex-1 truncate font-medium">{profile ? profile.name : "Choose a server"}</span>
          {s === "disconnected" && <Icon d={icons.chevron} className="size-4" />}
        </button>
        <button
          onClick={act}
          disabled={s === "connecting" || s === "disconnecting"}
          className={`h-11 rounded-md font-semibold transition-[filter] duration-150 hover:brightness-110 disabled:cursor-default ${actTone}`}
        >
          {actLabel}
        </button>
      </div>
    </div>
  );
}

function Figure({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex flex-col-reverse">
      <dt className="text-fog">{label}</dt>
      <dd className="t-num font-semibold">{value}</dd>
    </div>
  );
}

/** World map panned to the address's location. */
function WorldMap({ loc, state }: { loc?: string; state: Status["state"] }) {
  const [lat, lon] = (loc ?? "").split(",").map(Number);
  const known = loc !== undefined && Number.isFinite(lat) && Number.isFinite(lon);
  // Equirectangular on a 1000x500 grid, same projection as LAND.
  const x = known ? ((lon + 180) / 360) * 1000 : 470;
  const y = known ? ((90 - lat) / 180) * 500 : 140;
  const marker = state === "connected" ? "var(--safe)" : "var(--warn)";
  // The svg is 4x the panel width; translate so (x, y) sits centred, above the text.
  const tx = `calc(50cqw - ${(x / 1000) * 400}cqw)`;
  const ty = `calc(32cqh - ${(y / 500) * 200}cqw)`;
  return (
    <div className="absolute inset-0 overflow-hidden" style={{ containerType: "size" }} aria-hidden>
      <svg
        viewBox="0 0 1000 500"
        className="absolute top-0 left-0 transition-transform duration-1000 ease-drape"
        style={{ width: "400cqw", height: "200cqw", transform: `translate(${tx}, ${ty})` }}
      >
        <path d={LAND} fill="var(--surface-2)" />
        {known && (
          <g>
            <circle cx={x} cy={y} r={7} fill={marker} opacity={0.25} />
            <circle cx={x} cy={y} r={3} fill={marker} />
          </g>
        )}
      </svg>
    </div>
  );
}
