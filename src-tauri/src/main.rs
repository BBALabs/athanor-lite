// Windows GUI subsystem: NO console/terminal window ever appears — debug or release.
// DO NOT REMOVE. When launched from a terminal or the Tauri CLI, piped stdout is
// still relayed (self-tests read it), and tauri-plugin-log always writes the log file,
// so this costs no observability. Double-clicked, the app opens as a clean native window.
#![windows_subsystem = "windows"]

fn main() {
    athanor_lib::run()
}
