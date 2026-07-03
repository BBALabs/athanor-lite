/** StatusPill — subsystem health chip with a heartbeat dot. */

type Tone = "ok" | "warn" | "bad" | "idle" | "info";

interface StatusPillProps {
  tone: Tone;
  label: string;
  /** Pulse the dot — live/streaming states only. */
  live?: boolean;
}

const TONE_VAR: Record<Tone, string> = {
  ok: "var(--ok)",
  warn: "var(--warn)",
  bad: "var(--bad)",
  info: "var(--info)",
  idle: "var(--ink-low)",
};

export function StatusPill({ tone, label, live }: StatusPillProps) {
  return (
    <span className="status-pill" style={{ ["--pill-c" as string]: TONE_VAR[tone] }}>
      <span className={`status-pill__dot${live ? " status-pill__dot--live" : ""}`} />
      {label}
    </span>
  );
}
