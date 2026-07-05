/**
 * NavRail — icons floating on the glass, no divider (the spine carries the
 * edge). Settings rests alone at the bottom.
 */

import { useStore, type View } from "../state/store";
import { ChatIcon, LibraryIcon, ModelsIcon, SettingsIcon, SystemIcon } from "./Icons";

const SECTIONS: { view: View; label: string; icon: typeof SystemIcon }[] = [
  { view: "dashboard", label: "Your machine", icon: SystemIcon },
  { view: "models", label: "Models", icon: ModelsIcon },
  { view: "library", label: "Library", icon: LibraryIcon },
  { view: "chat", label: "Chat", icon: ChatIcon },
];

export function NavRail({ onSettings }: { onSettings: () => void }) {
  const view = useStore((s) => s.view);
  const setView = useStore((s) => s.setView);

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
