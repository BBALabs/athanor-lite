/**
 * Edition gate. Lite is a build-time cut of the same app: hardware scan,
 * ranked recommendations, one-click model download + chat — nothing else
 * mounted. The full feature set stays in the codebase and in the binary's
 * backend; only the UI narrows. Build with `--mode lite` (npm run dev:lite /
 * build:lite). Same identifier and data root as the full app, so a Lite
 * install upgraded to full Athanor keeps every model and conversation.
 */
export const LITE = import.meta.env.MODE === "lite";

/** The one external link Lite is allowed to open (mirrored in lib.rs). */
export const BBA_URL = "https://bbasecure.com";
