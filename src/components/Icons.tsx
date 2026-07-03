/**
 * Condere icon set — drawn in-house on a 24px grid, 1.7px round strokes.
 * No icon library: the set stays small, consistent, and ours.
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

/** Brand mark: a keystone — the block that locks the arch. condere: to found, to build. */
export function MarkIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <path d="M9 4h6l4 7-7 9-7-9 4-7Z" />
      <path d="M9 4l3 7m3-7l-3 7m0 0v9" strokeOpacity={0.55} />
    </svg>
  );
}

export function PulseIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <path d="M3 12h4l2.5-6 4 12L16 12h5" />
    </svg>
  );
}

export function StackIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <path d="M12 3 4 7.5l8 4.5 8-4.5L12 3Z" />
      <path d="m4 12.5 8 4.5 8-4.5" strokeOpacity={0.75} />
      <path d="m4 17 8 4.5L20 17" strokeOpacity={0.45} />
    </svg>
  );
}

export function SpacesIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <rect x="3.5" y="3.5" width="7.5" height="7.5" rx="1.6" />
      <rect x="13" y="3.5" width="7.5" height="7.5" rx="1.6" strokeOpacity={0.55} />
      <rect x="3.5" y="13" width="7.5" height="7.5" rx="1.6" strokeOpacity={0.55} />
      <path d="M16.75 13.6v6.8M13.35 17h6.8" />
    </svg>
  );
}

export function ChipIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <rect x="6.5" y="6.5" width="11" height="11" rx="2" />
      <rect x="10" y="10" width="4" height="4" rx="0.8" strokeOpacity={0.6} />
      <path d="M9 3.5v3M15 3.5v3M9 17.5v3M15 17.5v3M3.5 9h3M3.5 15h3M17.5 9h3M17.5 15h3" />
    </svg>
  );
}

export function GpuIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <rect x="3.5" y="7" width="17" height="9" rx="1.8" />
      <circle cx="10" cy="11.5" r="2.4" />
      <circle cx="16" cy="11.5" r="2.4" strokeOpacity={0.6} />
      <path d="M5.5 16v3M8.5 16v2M3.5 10H2" strokeOpacity={0.7} />
    </svg>
  );
}

export function RamIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <rect x="3" y="8" width="18" height="8" rx="1.4" />
      <path d="M6.5 8v4.5M10 8v4.5M13.5 8v4.5M17 8v4.5" strokeOpacity={0.6} />
      <path d="M5 16v2M9 16v2M15 16v2M19 16v2" />
    </svg>
  );
}

export function DiskIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <ellipse cx="12" cy="6.5" rx="8" ry="3" />
      <path d="M4 6.5v11c0 1.66 3.58 3 8 3s8-1.34 8-3v-11" />
      <path d="M4 12c0 1.66 3.58 3 8 3s8-1.34 8-3" strokeOpacity={0.55} />
    </svg>
  );
}

export function BoltIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <path d="M13 3 5 13.5h5.5L11 21l8-10.5h-5.5L13 3Z" />
    </svg>
  );
}

export function CheckIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <path d="m4.5 12.5 5 5L19.5 7" />
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

export function ArrowRightIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <path d="M4 12h15m0 0-6-6m6 6-6 6" />
    </svg>
  );
}
