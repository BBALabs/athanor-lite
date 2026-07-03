/**
 * NavRail — icons floating on the glass, no divider (the spine carries the
 * edge). Workspace monograms rest at the bottom like garage bays.
 */

import { useStore, type View } from "../state/store";
import { monogram } from "../lib/format";
import { ModelsIcon, SpacesIcon, SystemIcon } from "./Icons";

const SECTIONS: { view: View; label: string; icon: typeof SystemIcon }[] = [
  { view: "dashboard", label: "System", icon: SystemIcon },
  { view: "models", label: "Models", icon: ModelsIcon },
  { view: "workspaces", label: "Workspaces", icon: SpacesIcon },
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
            title={label}
          >
            <Icon size={21} />
            <span className="rail__item-light" />
          </button>
        ))}
      </div>

      {workspaces.length > 0 && (
        <div className="rail__spaces">
          {workspaces.slice(0, 5).map((ws) => (
            <button
              key={ws.id}
              className={`rail__space${ws.id === activeId ? " rail__space--active" : ""}`}
              style={{ ["--ws-hue" as string]: ws.accentHue }}
              onClick={() => void activate(ws.id)}
              title={ws.name}
              aria-label={`Switch to workspace ${ws.name}`}
            >
              {monogram(ws.name)}
            </button>
          ))}
        </div>
      )}
    </nav>
  );
}
