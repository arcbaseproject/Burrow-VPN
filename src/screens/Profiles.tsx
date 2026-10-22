import { useRef, useState } from "react";
import { api, countryName, Profile, profileCountry, seenCountries, Status } from "../api";

interface Props {
  profiles: Profile[];
  status: Status;
  selected: string | null;
  onSelect: (id: string) => void;
  reload: () => Promise<void>;
}

interface Draft {
  id: string | null;
  name: string;
  text: string;
}

// Keys and endpoints are per account and per server, so the provider fills them in.
const TEMPLATES: Record<string, string> = {
  Blank: `[Interface]
PrivateKey =
Address = 10.0.0.2/32
DNS = 1.1.1.1

[Peer]
PublicKey =
AllowedIPs = 0.0.0.0/0, ::/0
Endpoint = vpn.example.com:51820
PersistentKeepalive = 25
`,
  Mullvad: `[Interface]
PrivateKey =
Address = 10.64.0.2/32
DNS = 10.64.0.1

[Peer]
PublicKey =
AllowedIPs = 0.0.0.0/0, ::/0
Endpoint = se-got-wg-001.relays.mullvad.net:51820
`,
  "Proton VPN": `[Interface]
PrivateKey =
Address = 10.2.0.2/32
DNS = 10.2.0.1

[Peer]
PublicKey =
AllowedIPs = 0.0.0.0/0, ::/0
Endpoint = 0.0.0.0:51820
`,
};

export default function Profiles({ profiles, status, selected, onSelect, reload }: Props) {
  const [draft, setDraft] = useState<Draft | null>(null);
  const [error, setError] = useState<string | null>(null);
  const file = useRef<HTMLInputElement>(null);
  const locked = status.state !== "disconnected";

  const run = async (f: () => Promise<unknown>) => {
    setError(null);
    try {
      await f();
      await reload();
      return true;
    } catch (e) {
      setError(String(e));
      return false;
    }
  };

  const importFiles = async (files: FileList | null) => {
    for (const f of Array.from(files ?? [])) {
      if (f.size > 64 * 1024) {
        setError(`${f.name} is larger than 64 KB, so it can't be a WireGuard config.`);
        continue;
      }
      const name = f.name.replace(/\.conf$/i, "");
      const ok = await run(async () => onSelect((await api.saveProfile(null, name, await f.text())).id));
      if (!ok) break;
    }
    if (file.current) file.current.value = "";
  };

  const edit = async (p: Profile) => {
    setError(null);
    try {
      setDraft({ id: p.id, name: p.name, text: await api.profileConfig(p.id) });
    } catch (e) {
      setError(String(e));
    }
  };

  const seen = seenCountries();
  const byCountry = new Map<string, Profile[]>();
  for (const p of profiles) {
    const c = countryName(profileCountry(p, seen)) || "Other";
    byCountry.set(c, [...(byCountry.get(c) ?? []), p]);
  }
  const groups = [...byCountry].sort(([a], [b]) => (a === "Other" ? 1 : b === "Other" ? -1 : a.localeCompare(b)));

  if (draft) {
    return (
      <div className="flex min-h-full flex-col gap-4 px-4 pt-4 pb-4">
        <h2 className="text-[17px] font-semibold">{draft.id ? "Edit server" : "New server"}</h2>
        <div>
          <label htmlFor="name" className="text-sm text-fog">Name</label>
          <input
            id="name"
            value={draft.name}
            onChange={(e) => setDraft({ ...draft, name: e.target.value })}
            className="mt-1 h-11 w-full rounded-md bg-surface px-4 text-ink outline-none"
          />
        </div>
        {!draft.id && (
          <div>
            <label htmlFor="template" className="text-sm text-fog">Template</label>
            <select
              id="template"
              onChange={(e) => setDraft({ ...draft, text: TEMPLATES[e.target.value] })}
              aria-describedby="template-hint"
              className="mt-1 h-11 w-full rounded-md bg-surface px-4 text-ink outline-none"
            >
              {Object.keys(TEMPLATES).map((t) => <option key={t}>{t}</option>)}
            </select>
            <p id="template-hint" className="mt-2 text-sm text-fog">
              Fill in your private key, the server's public key and endpoint from your provider's WireGuard config page.
            </p>
          </div>
        )}
        <div className="flex flex-1 flex-col">
          <label htmlFor="conf" className="text-sm text-fog">WireGuard configuration</label>
          <textarea
            id="conf"
            value={draft.text}
            onChange={(e) => setDraft({ ...draft, text: e.target.value })}
            spellCheck={false}
            aria-describedby="conf-hint"
            className="mt-1 min-h-64 flex-1 resize-none rounded-md bg-surface p-4 font-mono text-[13px] leading-relaxed text-ink outline-none" /* taste-skill-ignore: the config is literal code */
          />
          {draft.id && (
            <p id="conf-hint" className="mt-2 text-sm text-fog">
              Saved keys show as [hidden]. Leave them as they are to keep them, or paste a new key.
            </p>
          )}
        </div>
        {error && <p role="alert" className="text-[15px] text-alert">{error}</p>}
        <div className="flex gap-3">
          <button onClick={() => (setDraft(null), setError(null))} className="h-11 flex-1 rounded-md bg-surface font-medium hover:brightness-110">
            Cancel
          </button>
          <button
            onClick={async () => (await run(() => api.saveProfile(draft.id, draft.name, draft.text))) && setDraft(null)}
            className="h-11 flex-1 rounded-md bg-shade font-semibold text-on-shade hover:brightness-110"
          >
            Save server
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-4 px-4 pt-4 pb-4">
      <div className="flex gap-3">
        <button onClick={() => file.current?.click()} className="h-11 flex-1 rounded-md bg-shade font-semibold text-on-shade hover:brightness-110">
          Import .conf file
        </button>
        <button onClick={() => setDraft({ id: null, name: "", text: TEMPLATES.Blank })} className="h-11 flex-1 rounded-md bg-surface font-medium hover:brightness-110">
          Paste config
        </button>
        <input ref={file} type="file" accept=".conf,text/plain" multiple hidden onChange={(e) => importFiles(e.target.files)} />
      </div>
      {error && <p role="alert" className="text-[15px] text-alert">{error}</p>}
      {!profiles.length && (
        <p className="max-w-[40ch] text-[15px] text-fog">
          Import a WireGuard config from your VPN provider or your own server. Burrow has no servers of its own.
        </p>
      )}
      <div role="radiogroup" aria-label="Servers" className="flex flex-col gap-5">
        {groups.map(([country, list]) => (
          <section key={country}>
            <h2 className="mb-2 font-semibold">{country}</h2>
            <ul className="flex flex-col gap-0.5 overflow-hidden rounded-md">
              {list.map((p) => {
                const isSel = p.id === selected;
                const isLive = p.id === status.profile_id && status.state !== "disconnected";
                return (
                  <li key={p.id} className={`px-4 py-3 transition-colors duration-150 ${isSel ? "bg-surface-2" : "bg-surface"}`}>
                    <div
                      role="radio"
                      aria-checked={isSel}
                      aria-disabled={locked}
                      tabIndex={0}
                      onClick={() => !locked && onSelect(p.id)}
                      onKeyDown={(e) => (e.key === " " || e.key === "Enter") && !locked && onSelect(p.id)}
                      className="flex items-center gap-3 outline-offset-4"
                    >
                      <span className={`grid size-5 shrink-0 place-items-center rounded-full ${isSel ? "bg-shade" : "bg-surface-2"}`}>
                        {isSel && <span className="size-2 rounded-full bg-on-shade" />}
                      </span>
                      <span className="min-w-0 flex-1">
                        <span className="block truncate font-semibold">{p.name}</span>
                        <span className="block truncate text-sm text-fog">{p.endpoints.join(", ") || "No endpoint"}</span>
                      </span>
                    </div>
                    <div className="flex items-center gap-4 pl-8 text-sm">
                      <span className="flex-1 text-fog">
                        {isLive ? "In use" : p.full_tunnel ? "All traffic" : "Some traffic"}
                        {p.key_file && ". Key in a file, no system keyring"}
                      </span>
                      <button onClick={() => edit(p)} disabled={isLive} className="min-h-11 font-medium hover:underline disabled:cursor-default disabled:opacity-40">
                        Edit
                      </button>
                      <button
                        onClick={() => confirm(`Delete ${p.name}? Its keys are removed from this computer.`) && run(() => api.deleteProfile(p.id))}
                        disabled={isLive}
                        className="min-h-11 font-medium text-alert hover:underline disabled:cursor-default disabled:opacity-40"
                      >
                        Delete
                      </button>
                    </div>
                  </li>
                );
              })}
            </ul>
          </section>
        ))}
      </div>
    </div>
  );
}
