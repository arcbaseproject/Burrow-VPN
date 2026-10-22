import { useState } from "react";
import { api, Settings } from "../api";
import { Icon, icons, type Screen } from "../App";

type Flag = { [K in keyof Settings]: Settings[K] extends boolean ? K : never }[keyof Settings];
interface Toggle {
  key: Flag;
  label: string;
  hint: string;
  needs?: Flag;
}

const groups: { title: string; toggles: Toggle[] }[] = [
  {
    title: "Protection",
    toggles: [
      { key: "kill_switch", label: "Kill switch", hint: "Block all traffic outside the tunnel, even if the tunnel drops. Applies when you next connect." },
      { key: "allow_lan", label: "Allow local network", hint: "Let printers, file shares and other devices on your network through the kill switch. Linux only.", needs: "kill_switch" },
      { key: "auto_reconnect", label: "Reconnect automatically", hint: "Bring the tunnel back if it drops. Your system may ask for your password." },
    ],
  },
  {
    title: "Privacy",
    toggles: [
      { key: "hide_ips", label: "Hide IP addresses", hint: "Show addresses as dots until you press Show. Useful when you share your screen." },
      { key: "ip_lookup", label: "Check public IP", hint: "Ask ipinfo.io for your address and location while protected. The request goes through the tunnel." },
      { key: "ip_check_exposed", label: "Check while exposed", hint: "Also check while you're not connected. This sends your real IP to ipinfo.io.", needs: "ip_lookup" },
    ],
  },
  {
    title: "Startup",
    toggles: [
      { key: "auto_connect", label: "Connect on startup", hint: "Connect to the last server when Burrow opens." },
      { key: "launch_at_login", label: "Launch at login", hint: "Start in the tray when you log in." },
    ],
  },
];

export default function SettingsScreen({ settings, onChange, goTo }: { settings: Settings; onChange: (s: Settings) => void; goTo: (s: Screen) => void }) {
  const [dns, setDns] = useState(settings.dns.join(", "));
  const [error, setError] = useState<{ field: "dns" | "other"; text: string } | null>(null);

  const save = async (next: Settings, field: "dns" | "other" = "other") => {
    setError(null);
    try {
      await api.saveSettings(next);
      onChange(next);
      if (next.ip_lookup !== settings.ip_lookup || next.ip_check_exposed !== settings.ip_check_exposed) api.refreshIp();
    } catch (e) {
      setError({ field, text: String(e) });
    }
  };

  return (
    <div className="flex flex-col gap-6 px-4 pt-4 pb-6">
      {error?.field === "other" && <p role="alert" className="text-[15px] text-alert">{error.text}</p>}

      {groups.map((g) => (
        <section key={g.title} aria-labelledby={`g-${g.title}`}>
          <h2 id={`g-${g.title}`} className="mb-2 font-semibold">{g.title}</h2>
          <div className="flex flex-col gap-0.5 overflow-hidden rounded-md">
            {g.toggles.map((t) => {
              const disabled = t.needs ? !settings[t.needs] : false;
              const on = settings[t.key] && !disabled;
              return (
                <button
                  key={t.key}
                  role="switch"
                  aria-checked={on}
                  disabled={disabled}
                  onClick={() => save({ ...settings, [t.key]: !settings[t.key] })}
                  className="flex w-full items-center gap-4 bg-surface px-4 py-3 text-left transition-colors duration-150 hover:bg-surface-2 disabled:cursor-default disabled:opacity-50 disabled:hover:bg-surface"
                >
                  <span className="flex-1">
                    <span className="block text-[15px] font-medium">{t.label}</span>
                    <span className="mt-0.5 block text-sm leading-snug text-fog">{t.hint}</span>
                  </span>
                  <span className={`flex h-7 w-12 shrink-0 items-center rounded-full p-1 transition-colors duration-200 ${on ? "bg-shade" : "bg-surface-2"}`}>
                    <span className={`size-5 rounded-full transition-transform duration-200 ease-drape ${on ? "translate-x-5 bg-on-shade" : "bg-fog"}`} />
                  </span>
                </button>
              );
            })}
          </div>
        </section>
      ))}

      <section aria-labelledby="g-dns">
        <h2 id="g-dns" className="mb-2 font-semibold">DNS</h2>
        <label htmlFor="dns" className="text-sm text-fog">DNS servers</label>
        <input
          id="dns"
          value={dns}
          onChange={(e) => setDns(e.target.value)}
          onBlur={() => save({ ...settings, dns: dns.split(",").map((d) => d.trim()).filter(Boolean) }, "dns")}
          placeholder="Use the server's DNS"
          aria-describedby="dns-hint"
          aria-invalid={error?.field === "dns"}
          className="mt-1 h-11 w-full rounded-md bg-surface px-4 text-ink outline-none placeholder:text-fog"
        />
        {error?.field === "dns" ? (
          <p role="alert" className="mt-2 text-sm text-alert">{error.text}</p>
        ) : (
          <p id="dns-hint" className="mt-2 text-sm text-fog">
            Replaces the server's DNS. Separate addresses with commas, for example 9.9.9.9, 149.112.112.112. Applies when you next connect.
          </p>
        )}
      </section>

      <section aria-labelledby="g-look">
        <h2 id="g-look" className="mb-2 font-semibold">Appearance</h2>
        <div role="radiogroup" aria-label="Theme" className="flex rounded-md bg-surface p-1">
          {(["dark", "light"] as const).map((t) => (
            <button
              key={t}
              role="radio"
              aria-checked={settings.theme === t}
              onClick={() => save({ ...settings, theme: t })}
              className={`h-9 flex-1 rounded text-[15px] capitalize transition-colors duration-150 ${settings.theme === t ? "bg-surface-2 font-semibold" : "text-fog"}`}
            >
              {t}
            </button>
          ))}
        </div>
      </section>

      <button onClick={() => goTo("about")} className="flex h-11 items-center rounded-md bg-surface px-4 text-left font-medium hover:bg-surface-2">
        <span className="flex-1">About Burrow</span>
        <Icon d={icons.chevron} className="size-4" />
      </button>
    </div>
  );
}
