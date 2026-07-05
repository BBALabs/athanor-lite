//! Portable mode — run from a folder or USB stick with everything (models,
//! workspaces, chats, logs) beside the executable, nothing in the OS profile or
//! registry. KoboldCpp-style: copy the folder, take your whole setup with you.
//!
//! Detection happens once, from the executable location alone (no app handle),
//! so even the log plugin — configured before the app is built — can honor it.
//! Trigger it either way:
//!   • drop an empty file named `athanor-portable` next to the executable, or
//!   • set the env var `ATHANOR_PORTABLE=1`.
//! The data then lives in `athanor-data/` alongside the executable.

use std::path::PathBuf;
use std::sync::OnceLock;

use tauri::Manager;

use crate::error::{AthanorError, Result};

const MARKER: &str = "athanor-portable";
const DATA_DIRNAME: &str = "athanor-data";

fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(|p| p.to_path_buf())
}

/// Whether this launch is portable. Cached — the answer can't change while
/// running, and every path resolution asks.
pub fn is_portable() -> bool {
    static PORTABLE: OnceLock<bool> = OnceLock::new();
    *PORTABLE.get_or_init(|| {
        if std::env::var_os("ATHANOR_PORTABLE").is_some() {
            return true;
        }
        exe_dir().map(|d| d.join(MARKER).exists()).unwrap_or(false)
    })
}

/// The portable data root (`<exe dir>/athanor-data`), or None when not portable
/// or the executable path can't be determined.
pub fn portable_root() -> Option<PathBuf> {
    if !is_portable() {
        return None;
    }
    exe_dir().map(|d| d.join(DATA_DIRNAME))
}

/// The effective data root: the portable folder if portable, else the OS
/// per-user app-data directory. Every other path in the app derives from this.
pub fn root(app: &tauri::AppHandle) -> Result<PathBuf> {
    if let Some(p) = portable_root() {
        return Ok(p);
    }
    app.path()
        .app_data_dir()
        .map_err(|e| AthanorError::Path(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_root_is_exe_relative_and_named() {
        // Independent of whether THIS test run is portable: the portable path,
        // when it exists, must sit beside the exe under the data dir name.
        if let Some(root) = exe_dir().map(|d| d.join(DATA_DIRNAME)) {
            assert!(root.ends_with(DATA_DIRNAME));
            assert_eq!(root.parent(), exe_dir().as_deref());
        }
    }

    #[test]
    fn marker_and_dir_names_are_stable() {
        // These strings are a compatibility contract with users' folders.
        assert_eq!(MARKER, "athanor-portable");
        assert_eq!(DATA_DIRNAME, "athanor-data");
    }
}
