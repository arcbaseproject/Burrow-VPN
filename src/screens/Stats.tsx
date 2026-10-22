import { fmtBytes, fmtDuration, fmtRate, Profile, Sample, Settings, Status, useElapsed } from "../api";
import { Ip } from "../App";

interface Props {
  status: Status;
  history: Sample[];
  profiles: Profile[];
  settings: Settings;
}

export default function Stats({ status, history, profiles, settings }: Props) {
  const on = status.state === "connected";
  const elapsed = useElapsed(on ? status.connected_at : null);
  const profile = profiles.find((p) => p.id === status.profile_id);
  const peak = history.reduce((m, s) => Math.max(m, s.rx, s.tx), 0);

  return (
    <div className="flex flex-col gap-6 px-4 pt-4 pb-4">
      {!on ? (
        <p className="text-[15px] text-fog">Connect to see this session's traffic.</p>
      ) : (
        <>
          <dl className="grid grid-cols-2 gap-x-4 gap-y-5">
            <Figure big label="Received" value={fmtBytes(status.rx)} />
            <Figure big label="Sent" value={fmtBytes(status.tx)} />
            <Figure label="Download now" value={fmtRate(status.rx_rate)} />
            <Figure label="Upload now" value={fmtRate(status.tx_rate)} />
          </dl>

          <figure className="rounded-md bg-surface p-4">
            <Spark history={history} peak={peak} />
            <figcaption className="mt-3 flex flex-wrap gap-x-5 gap-y-1 text-sm text-fog">
              <span className="flex items-center gap-2"><span className="h-1 w-4 rounded-full bg-ink" />Download</span>
              <span className="flex items-center gap-2"><span className="h-1 w-4 rounded-full bg-sodium" />Upload</span>
              <span>Last minute, peak {fmtRate(peak)}</span>
            </figcaption>
          </figure>

          <dl className="flex flex-col gap-3 text-[15px]">
            <Row k="Connected for" v={<span className="t-num">{fmtDuration(elapsed)}</span>} />
            <Row k="Server" v={profile?.name ?? "Unknown"} />
            <Row k="Endpoint" v={profile?.endpoints.join(", ") || "None"} />
            <Row k="Exit address" v={status.ip ? <Ip ip={status.ip.ip} hide={settings.hide_ips} className="t-num" /> : "Not checked"} />
            {status.ip?.org && <Row k="Network" v={status.ip.org} />}
          </dl>
          <p className="text-sm text-fog">Totals come from the tunnel interface's counters since it came up.</p>
        </>
      )}
    </div>
  );
}

function Figure({ label, value, big }: { label: string; value: string; big?: boolean }) {
  return (
    <div className="flex flex-col-reverse">
      <dt className="mt-1 text-sm text-fog">{label}</dt>
      <dd className={`t-figure ${big ? "text-[34px]" : "text-[22px]"}`}>{value}</dd>
    </div>
  );
}

function Row({ k, v }: { k: string; v: React.ReactNode }) {
  return (
    <div className="flex items-baseline justify-between gap-4">
      <dt className="shrink-0 text-fog">{k}</dt>
      <dd className="min-w-0 truncate text-right">{v}</dd>
    </div>
  );
}

function Spark({ history, peak }: { history: Sample[]; peak: number }) {
  const W = 300, H = 88;
  const line = (key: keyof Sample) =>
    history.map((s, i) => `${(i / 59) * W},${H - (peak ? (s[key] / peak) * (H - 6) : 0) - 3}`).join(" ");
  return (
    <svg viewBox={`0 0 ${W} ${H}`} className="h-22 w-full" preserveAspectRatio="none" role="img" aria-label="Download and upload speed over the last minute">
      <polyline points={line("rx")} fill="none" stroke="var(--ink)" strokeWidth={2.5} strokeLinejoin="round" vectorEffect="non-scaling-stroke" />
      <polyline points={line("tx")} fill="none" stroke="var(--sodium)" strokeWidth={2} strokeLinejoin="round" vectorEffect="non-scaling-stroke" />
    </svg>
  );
}
