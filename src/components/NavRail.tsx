/**
 * NavRail — the app spine. Primary sections up top, the workspace
 * switcher stacked at the bottom like project slots in an IDE.
 */

import { useStore, type View } from "../state/store";
import { PulseIcon, SpacesIcon, StackIcon } from "./Icons";

const SECTIONS: { view: View; label: string; icon: typeof PulseIcon }[] = [
  { view: "dashboard", label: "System", icon: PulseIcon },
  { view: "models", label: "Models", icon: StackIcon },
  { view: "workspaces", label: "Spaces", icon: SpacesIcon },
];

export function NavRail() {
  const view = useStore((s) => s.view);
  const setView = useStore((s) => s.setView);
  const { workspaces, activeId } = useStore((s) => s.workspaces);
  const activate = useStore((s) => s.activateWorkspace);

  return (
    <nav className="rail">
      <div className="rail__sections">
        {SECTIONS.map(({ view: v, label, icon: Icon }) => (
          <button
            key={v}
            className={`rail__item${view === v ? " rail__item--active" : ""}`}
            onClick={() => setView(v)}
            aria-label={label}
          >
            <Icon size={19} />
            <span className="rail__item-label">{label}</span>
          </button>
        ))}
      </div>

      {workspaces.length > 0 && (
        <div className="rail__spaces">
          <div className="rail__spaces-rule" />
          {workspaces.slice(0, 6).map((ws) => (
            <button
              key={ws.id}
              className={`rail__space${ws.id === activeId ? " rail__space--active" : ""}`}
              style={{ ["--ws-hue" as string]: ws.accentHue }}
              onClick={() => void activate(ws.id)}
              title={`${ws.name} — ${ws.purpose || "workspace"}`}
              aria-label={`Switch to workspace ${ws.name}`}
            >
              {ws.glyph || ws.name.slice(0, 1).toUpperCase()}
            </button>
          ))}
        </div>
      )}
    </nav>
  );
}
