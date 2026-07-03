//! Zero-orphan guarantees for engine child processes.
//!
//! Two independent layers:
//! 1. **Job object (Windows):** every spawned engine is assigned to a job
//!    with KILL_ON_JOB_CLOSE. If Athanor dies — cleanly, by crash, or by
//!    `taskkill /F` — the OS closes the job handle and kills the children.
//!    No zombie llama-server holding VRAM, ever.
//! 2. **Startup sweep:** on boot, any process whose executable lives inside
//!    OUR runtimes directory is an orphan from a previous life (pre-job-
//!    object builds, or a machine crash mid-write) and is terminated.
//!    Path-scoped, so it can never touch a user's own llama.cpp installs.

use std::path::Path;
use std::process::Child;

#[cfg(windows)]
mod job {
    use std::os::windows::io::AsRawHandle;
    use std::sync::OnceLock;

    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    struct JobHandle(HANDLE);
    // The handle is only ever used for AssignProcessToJobObject; the OS side
    // is thread-safe.
    unsafe impl Send for JobHandle {}
    unsafe impl Sync for JobHandle {}

    fn job() -> Option<&'static JobHandle> {
        static JOB: OnceLock<Option<JobHandle>> = OnceLock::new();
        JOB.get_or_init(|| unsafe {
            let handle = CreateJobObjectW(None, None).ok()?;
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .is_err()
            {
                log::error!(target: "rt", "job object configuration failed; orphan guard degraded");
                return None;
            }
            Some(JobHandle(handle))
        })
        .as_ref()
    }

    pub fn adopt(child: &std::process::Child) {
        match job() {
            Some(j) => unsafe {
                let raw = HANDLE(child.as_raw_handle());
                if let Err(e) = AssignProcessToJobObject(j.0, raw) {
                    log::error!(target: "rt", "failed to adopt child into job object: {e}");
                }
            },
            None => log::error!(target: "rt", "no job object; child not orphan-guarded"),
        }
    }
}

/// Bind a child's lifetime to the app's. Best-effort on non-Windows.
pub fn adopt(child: &Child) {
    #[cfg(windows)]
    job::adopt(child);
    #[cfg(not(windows))]
    let _ = child;
}

/// Kill any process running from inside `runtimes_dir` — an orphan by
/// definition (only Athanor's own children execute from there).
pub fn sweep_orphans(runtimes_dir: &Path) {
    if !runtimes_dir.exists() {
        return;
    }
    let mut sys = sysinfo::System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let me = sysinfo::get_current_pid().ok();
    for (pid, process) in sys.processes() {
        if Some(*pid) == me {
            continue;
        }
        let Some(exe) = process.exe() else { continue };
        if exe.starts_with(runtimes_dir) {
            log::warn!(
                target: "rt",
                "orphaned engine from a previous session (pid {pid}, {exe:?}) — terminating"
            );
            process.kill();
        }
    }
}
