/**
 * NavRail — icons floating on the glass, no divider (the spine carries the
 * edge). Workspace monograms rest at the bottom like garage bays.
 */

import { useStore, type View } from "../state/store";
import { monogram } from "../lib/format";
import { LITE } from "../lib/edition";
import { ChatIcon, CompareIcon, KnowledgeIcon, ModelsIcon, SettingsIcon, SpacesIcon, SystemIcon, TuneIcon } from "./Icons";

const SECTIONS: { view: View; label: string; icon: typeof SystemIcon }[] = LITE
  ? [
      { view: "home", label: "Your machine", icon: SystemIcon },
      { view: "chat", label: "Chat", icon: ChatIcon },
    ]
  : [
      { view: "chat", label: "Chat", icon: ChatIcon },
      { view: "knowledge", label: "Knowledge", icon: KnowledgeIcon },
      { view: "models", label: "Models", icon: ModelsIcon },
      { view: "compare", label: "Compare", icon: CompareIcon },
      { view: "training", label: "Tune", icon: TuneIcon },
      { view: "dashboard", label: "System", icon: SystemIcon },
      { view: "workspaces", label: "Workspaces", icon: SpacesIcon },
    ];

export function NavRail({ onSettings }: { onSettings: () => void }) {
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

      <div className="rail__bottom">
        {!LITE && workspaces.length > 0 && (
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
        <button
          className="rail__item rail__settings"
          onClick={onSettings}
          title="Settings"
          aria-label="Settings"
        >
          <SettingsIcon size={18} />
        </button>
      </div>
    </nav>
  );
}
