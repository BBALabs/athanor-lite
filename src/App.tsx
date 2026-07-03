import { useEffect, useRef } from "react";
import { useStore, useLatestSample } from "./state/store";
import { Titlebar } from "./components/Titlebar";
import { NavRail } from "./components/NavRail";
import { BootSequence } from "./components/BootSequence";
import { Dashboard } from "./views/Dashboard";
import { Models } from "./views/Models";
import { Workspaces } from "./views/Workspaces";
import { AlertIcon, CloseIcon } from "./components/Icons";

function StatusBar() {
  const sample = useLatestSample();
  const { workspaces, activeId } = useStore((s) => s.workspaces);
  const active = workspaces.find((w) => w.id === activeId);

  return (
    <footer className="statusbar">
      <div className="statusbar__cell">
        <span className={`statusbar__beat${sample ? " statusbar__beat--live" : ""}`} />
        <span className="k-num">telemetry {sample ? "1 Hz" : "—"}</span>
      </div>
      <div className="statusbar__cell statusbar__cell--center">
        {active ? (
          <>
            <span className="statusbar__ws-glyph" style={{ ["--ws-hue" as string]: active.accentHue }}>
              {active.glyph}
            </span>
            <span>{active.name}</span>
          </>
        ) : (
          <span className="statusbar__idle">no active workspace</span>
        )}
      </div>
      <div className="statusbar__cell statusbar__cell--right">
        <span className="k-num">CONDERE 0.1.0 · M1</span>
      </div>
    </footer>
  );
}

function OpErrorToast() {
  const err = useStore((s) => s.lastOpError);
  const clear = useStore((s) => s.clearOpError);
  const timer = useRef<number | undefined>(undefined);

  useEffect(() => {
    if (!err) return;
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(clear, 7000);
    return () => window.clearTimeout(timer.current);
  }, [err, clear]);

  if (!err) return null;
  return (
    <div className="toast" role="alert">
      <AlertIcon size={15} />
      <span className="toast__msg">{err}</span>
      <button className="toast__close" onClick={clear} aria-label="Dismiss">
        <CloseIcon size={11} />
      </button>
    </div>
  );
}

export default function App() {
  const boot = useStore((s) => s.boot);
  const view = useStore((s) => s.view);

  return (
    <div className="shell">
      <Titlebar />
      <div className="shell__body">
        <NavRail />
        <main className="shell__main" data-view={view}>
          {boot === "ready" && (
            <>
              {view === "dashboard" && <Dashboard />}
              {view === "models" && <Models />}
              {view === "workspaces" && <Workspaces />}
            </>
          )}
        </main>
      </div>
      <StatusBar />
      <OpErrorToast />
      <BootSequence />
    </div>
  );
}
