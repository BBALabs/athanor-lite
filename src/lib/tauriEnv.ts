/** True when running inside the Tauri webview (vs a plain browser tab). */
export const IN_TAURI =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
