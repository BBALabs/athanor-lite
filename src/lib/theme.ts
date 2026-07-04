/**
 * Accent themes — curated warm-light families only. The Black Glass material
 * (field, surfaces, ink, motion) never changes; only the accent hue family the
 * light glows in. Each is hand-tuned to stay a *warm light* per the design spec
 * (no neon, no cyan, nothing that reads as a threshold-breach semantic).
 */

export interface AccentPreset {
  id: string;
  label: string;
  lume: string;
  lumeWarm: string;
  lumeHot: string;
  lumeDeep: string;
}

export const ACCENT_PRESETS: AccentPreset[] = [
  { id: "violet", label: "Violet", lume: "#a06bff", lumeWarm: "#c99af5", lumeHot: "#efd9ff", lumeDeep: "#4c2b85" },
  { id: "indigo", label: "Indigo", lume: "#7c7bff", lumeWarm: "#a6b0f5", lumeHot: "#dfe4ff", lumeDeep: "#33306f" },
  { id: "orchid", label: "Orchid", lume: "#c774f0", lumeWarm: "#e0a6f5", lumeHot: "#f6e0ff", lumeDeep: "#5e2a7a" },
  { id: "rose", label: "Rose", lume: "#f56ec8", lumeWarm: "#f5a6d8", lumeHot: "#ffe0f4", lumeDeep: "#7a2a5a" },
];

const DEFAULT = ACCENT_PRESETS[0];

/** Paint an accent family onto the document root by overriding the lume tokens.
 *  Everything downstream (spine, glows, jewels, focus) reads these variables. */
export function applyAccent(id: string): void {
  if (typeof document === "undefined") return;
  const p = ACCENT_PRESETS.find((a) => a.id === id) ?? DEFAULT;
  const root = document.documentElement.style;
  root.setProperty("--lume", p.lume);
  root.setProperty("--lume-warm", p.lumeWarm);
  root.setProperty("--lume-hot", p.lumeHot);
  root.setProperty("--lume-deep", p.lumeDeep);
}
