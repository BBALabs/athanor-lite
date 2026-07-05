/**
 * Athanor icon set — drawn in-house on a 24px grid, 1.7px round strokes.
 * The keystone mark generates the drawing language; motifs are instrument-
 * grade, never stock (no bolts, no stacks, no pulses).
 */

import type { SVGProps } from "react";

type IconProps = SVGProps<SVGSVGElement> & { size?: number };

function base({ size = 18, ...rest }: IconProps): SVGProps<SVGSVGElement> {
  return {
    width: size,
    height: size,
    viewBox: "0 0 24 24",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: 1.7,
    strokeLinecap: "round",
    strokeLinejoin: "round",
    ...rest,
  };
}

/** Brand mark: a keystone — the block that locks the arch. athanor: to found, to build. */
export function MarkIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <path d="M9 4h6l4 7-7 9-7-9 4-7Z" />
      <path d="M9 4l3 7m3-7l-3 7m0 0v9" strokeOpacity={0.55} />
    </svg>
  );
}

/** Knowledge: an open book — the workspace's documents. */
export function KnowledgeIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <path d="M12 6.5c-1.8-1.4-4-1.6-6.5-1.4v11c2.5-.2 4.7 0 6.5 1.4 1.8-1.4 4-1.6 6.5-1.4v-11c-2.5-.2-4.7 0-6.5 1.4Z" />
      <path d="M12 6.5v11" strokeOpacity={0.55} />
    </svg>
  );
}

/** Chat: a console line — the prompt caret and the machine's reply. */
export function ChatIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <path d="M5 7.5 9 11l-4 3.5" />
      <path d="M12 15h7" strokeOpacity={0.75} />
      <path d="M4 20h16" strokeOpacity={0.35} />
    </svg>
  );
}

/** System: a calibrated dial — 270° sweep and a needle. */
export function SystemIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <path d="M6.34 17.66A8 8 0 1 1 17.66 17.66" />
      <path d="M12 12l3.6-4.6" />
      <circle cx="12" cy="12" r="1.1" fill="currentColor" stroke="none" />
    </svg>
  );
}

/** Models: strata — a ledger of weights, not a stacked cube. */
export function ModelsIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <path d="M4 6.5h16" />
      <path d="M4 12h11.5" strokeOpacity={0.75} />
      <path d="M4 17.5h7" strokeOpacity={0.5} />
      <circle cx="19.5" cy="12" r="0.9" fill="currentColor" stroke="none" opacity={0.75} />
      <circle cx="15" cy="17.5" r="0.9" fill="currentColor" stroke="none" opacity={0.5} />
    </svg>
  );
}

/** Library: a platter — the models resident on this disk. */
export function LibraryIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <circle cx="12" cy="12" r="8" />
      <circle cx="12" cy="12" r="2.2" strokeOpacity={0.75} />
      <path d="M12 4a8 8 0 0 1 6.9 4" strokeOpacity={0.45} />
    </svg>
  );
}

/** Workspaces: rooms — one lit, three at rest. */
export function SpacesIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <rect x="4" y="4" width="7" height="7" rx="2" fill="currentColor" stroke="none" opacity={0.9} />
      <rect x="13.5" y="4" width="7" height="7" rx="2" strokeOpacity={0.5} />
      <rect x="4" y="13.5" width="7" height="7" rx="2" strokeOpacity={0.5} />
      <rect x="13.5" y="13.5" width="7" height="7" rx="2" strokeOpacity={0.5} />
    </svg>
  );
}

/** Settings: three calibration sliders. */
export function SettingsIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <path d="M4 7h16M4 12h16M4 17h16" strokeOpacity={0.45} />
      <circle cx="9" cy="7" r="2" fill="var(--field, #0a070e)" />
      <circle cx="9" cy="7" r="2" />
      <circle cx="15" cy="12" r="2" fill="var(--field, #0a070e)" />
      <circle cx="15" cy="12" r="2" />
      <circle cx="7" cy="17" r="2" fill="var(--field, #0a070e)" />
      <circle cx="7" cy="17" r="2" />
    </svg>
  );
}

export function AlertIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <path d="M12 3.5 2.5 19.5h19L12 3.5Z" />
      <path d="M12 10v4.5M12 17.4v.1" />
    </svg>
  );
}

export function PlusIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <path d="M12 5v14M5 12h14" />
    </svg>
  );
}

/** Compare: two panels, side by side — A against B. */
export function CompareIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <path d="M4.5 5.5h6v13h-6zM13.5 5.5h6v13h-6z" />
      <path d="M12 3.5v17" strokeOpacity={0.5} />
    </svg>
  );
}

/** Tune: adjustment tracks with knobs — shaping a model to your data. */
export function TuneIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <path d="M4 8h9M19 8h1M4 16h1M11 16h9" />
      <circle cx="16" cy="8" r="2.3" />
      <circle cx="8" cy="16" r="2.3" />
    </svg>
  );
}

/** Search: a lens over the record. */
export function SearchIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <circle cx="11" cy="11" r="6.5" />
      <path d="M16 16l3.5 3.5" />
    </svg>
  );
}

/** Export: lift a copy out to the file system. */
export function ExportIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <path d="M12 15V4M8 8l4-4 4 4" />
      <path d="M5 14v4.5c0 .8.7 1.5 1.5 1.5h11c.8 0 1.5-.7 1.5-1.5V14" strokeOpacity={0.6} />
    </svg>
  );
}

export function TrashIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <path d="M4.5 6.5h15M9.5 6.5v-2h5v2M6.5 6.5 7.5 20h9l1-13.5" />
      <path d="M10 10.5v6M14 10.5v6" strokeOpacity={0.6} />
    </svg>
  );
}

export function MinimizeIcon(props: IconProps) {
  return (
    <svg {...base({ size: 12, ...props })}>
      <path d="M5 12h14" />
    </svg>
  );
}

export function MaximizeIcon(props: IconProps) {
  return (
    <svg {...base({ size: 12, ...props })}>
      <rect x="5.5" y="5.5" width="13" height="13" rx="1.5" />
    </svg>
  );
}

export function RestoreIcon(props: IconProps) {
  return (
    <svg {...base({ size: 12, ...props })}>
      <rect x="5" y="8" width="11" height="11" rx="1.5" />
      <path d="M8.5 5.5H17A2 2 0 0 1 19 7.5v8.5" strokeOpacity={0.7} />
    </svg>
  );
}

export function CloseIcon(props: IconProps) {
  return (
    <svg {...base({ size: 12, ...props })}>
      <path d="m6 6 12 12M18 6 6 18" />
    </svg>
  );
}
