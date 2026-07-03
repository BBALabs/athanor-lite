/**
 * Titlebar — transparent glass. A small warm wordmark, a drag surface, and
 * window controls that stay ghosted until approached.
 */

import { useCallback, useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { IN_TAURI } from "../lib/tauriEnv";
import { CloseIcon, MarkIcon, MaximizeIcon, MinimizeIcon, RestoreIcon } from "./Icons";

export function Titlebar() {
  const [maximized, setMaximized] = useState(false);
  const win = IN_TAURI ? getCurrentWindow() : null;

  useEffect(() => {
    if (!win) return;
    let cancelled = false;
    const sync = () => {
      win.isMaximized().then((m) => {
        if (!cancelled) setMaximized(m);
      }).catch(() => {});
    };
    sync();
    const unlisten = win.onResized(sync);
    return () => {
      cancelled = true;
      unlisten.then((f) => f()).catch(() => {});
    };
  }, [win]);

  const minimize = useCallback(() => void win?.minimize().catch(() => {}), [win]);
  const toggle = useCallback(() => void win?.toggleMaximize().catch(() => {}), [win]);
  const close = useCallback(() => void win?.close().catch(() => {}), [win]);

  return (
    <header className="titlebar" data-tauri-drag-region>
      <div className="titlebar__brand" data-tauri-drag-region>
        <MarkIcon size={15} className="titlebar__mark" />
        <span className="titlebar__wordmark" data-tauri-drag-region>
          CONDERE
        </span>
        {!IN_TAURI && (
          <span className="titlebar__harness" title="Browser design harness — hardware, telemetry, and workspaces shown here are synthetic. Run the desktop app for real data.">
            design harness · synthetic data
          </span>
        )}
      </div>

      <div className="titlebar__controls">
        <button className="titlebar__btn" onClick={minimize} aria-label="Minimize" title="Minimize">
          <MinimizeIcon />
        </button>
        <button
          className="titlebar__btn"
          onClick={toggle}
          aria-label={maximized ? "Restore" : "Maximize"}
          title={maximized ? "Restore" : "Maximize"}
        >
          {maximized ? <RestoreIcon /> : <MaximizeIcon />}
        </button>
        <button
          className="titlebar__btn titlebar__btn--close"
          onClick={close}
          aria-label="Close"
          title="Close"
        >
          <CloseIcon />
        </button>
      </div>
    </header>
  );
}
