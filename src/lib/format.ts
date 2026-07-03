const GIB = 1024 ** 3;

/** 68719476736 -> "64" (GiB, no unit). */
export function gib(bytes: number, digits = 0): string {
  return (bytes / GIB).toFixed(digits);
}

/** Bytes -> human string in DECIMAL units — matches the catalog's file sizes
 *  (34.3 GB in the row must read 34.3 GB in the progress line). */
export function bytesHuman(bytes: number): string {
  if (bytes >= 1e9) {
    const v = bytes / 1e9;
    return `${v >= 100 ? v.toFixed(0) : v.toFixed(1)} GB`;
  }
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(0)} MB`;
  return `${(bytes / 1e3).toFixed(0)} KB`;
}

export function pct(value: number, digits = 0): string {
  return `${value.toFixed(digits)}%`;
}

export function ghz(mhz: number): string {
  return `${(mhz / 1000).toFixed(2)} GHz`;
}

/** 131072 -> "128K" */
export function ctxHuman(tokens: number): string {
  return tokens >= 1024 ? `${Math.round(tokens / 1024)}K` : `${tokens}`;
}

export function relativeTime(iso: string): string {
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return "—";
  const s = Math.max(0, (Date.now() - then) / 1000);
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
  return `${Math.floor(s / 86400)}d ago`;
}

/** Usage fraction -> semantic color token. */
export function loadColor(frac: number): string {
  if (frac < 0.65) return "var(--ok)";
  if (frac < 0.85) return "var(--warn)";
  return "var(--bad)";
}

export const COMPUTE_CLASS_LABEL: Record<string, { title: string; sub: string }> = {
  CpuOnly: { title: "CPU class", sub: "no dedicated GPU detected" },
  VramLow: { title: "Compact class", sub: "entry-level GPU acceleration" },
  VramMid: { title: "Performance class", sub: "solid mid-range acceleration" },
  VramHigh: { title: "Enthusiast class", sub: "high-end single-GPU territory" },
  VramWorkstation: { title: "Workstation class", sub: "top-tier local inference" },
};

/** Workspace identity: a monogram letterform, never a dingbat. */
export function monogram(name: string): string {
  return (name.trim()[0] ?? "·").toUpperCase();
}
