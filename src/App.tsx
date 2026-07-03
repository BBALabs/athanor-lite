import { useEffect, useRef } from "react";
import { useStore, useLatestSample } from "./state/store";
import { Titlebar } from "./components/Titlebar";
import { NavRail } from "./components/NavRail";
import { BootSequence } from "./components/BootSequence";
import { Chat } from "./views/Chat";
import { Dashboard } from "./views/Dashboard";
import { Models } from "./views/Models";
import { Workspaces } from "./views/Workspaces";
import { AlertIcon, CloseIcon } from "./components/Icons";
import { monogram } from "./lib/format";

/**
 * The Ambient Spine — the app's one light source. It breathes, flares once
 * on each view change, and drifts warm when the machine is under load.
 */
function Spine() {
  const view = useStore((s) => s.view);
  const sample = useLatestSample();

  const gpu = sample?.gpus[0];
  const load = Math.max(
    (sample?.cpuUsagePct ?? 0) / 100,
    gpu ? gpu.vramUsedBytes / Math.max(1, gpu.vramTotalBytes) : 0,
  );

  return (
    <div className="spine" style={{ ["--spine-c" as string]: load > 0.85 ? "var(--warn)" : "var(--lume)" }}>
      <div className="spine__halo" />
      <div className="spine__band" />
      <div className="spine__flare" key={view} />
    </div>
  );
}

function StatusBar() {
  const sample = useLatestSample();
  const { workspaces, activeId } = useStore((s) => s.workspaces);
  const downloads = useStore((s) => s.downloads);
  const active = workspaces.find((w) => w.id === activeId);

  // One live download owns the center of the status line while it runs.
  const liveDl = Object.values(downloads).find(
    (d) => d.state === "downloading" || d.state === "verifying" || d.state === "starting",
  );

  return (
    <footer className="statusbar">
      <div className="statusbar__cell">
        <span className={`statusbar__beat${sample ? " statusbar__beat--live" : ""}`} />
        <span className="t-quiet tnum" title="Local monitor — nothing leaves this machine">
          local monitor {sample ? "1 Hz" : "—"}
        </span>
      </div>
      <div className="statusbar__cell statusbar__cell--center">
        {liveDl ? (
          <span className="t-quiet tnum statusbar__dl">
            {liveDl.state === "verifying"
              ? `verifying ${liveDl.fileName}`
              : `${liveDl.fileName} · ${liveDl.totalBytes ? ((liveDl.receivedBytes / liveDl.totalBytes) * 100).toFixed(0) : 0}%`}
          </span>
        ) : (
          active && (
            <>
              <span className="statusbar__monogram" style={{ ["--ws-hue" as string]: active.accentHue }}>
                {monogram(active.name)}
              </span>
              <span className="t-quiet">{active.name}</span>
            </>
          )
        )}
      </div>
      <div className="statusbar__cell statusbar__cell--right">
        <span className="t-quiet">Black Box Analytics · 0.1.0</span>
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
      <AlertIcon size={14} />
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
        <Spine />
        <main className="shell__main" key={view}>
          {boot === "ready" && (
            <>
              {view === "chat" && <Chat />}
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
