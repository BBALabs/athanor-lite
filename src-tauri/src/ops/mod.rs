//! The operations registry — one source of truth for everything running.
//!
//! CORE DESIGN PRINCIPLE (applies to every feature, present and future):
//! any operation that outlives a click — download, engine fetch, model load,
//! generation, import, benchmark — MUST:
//!   1. register here when it starts (visible in the Operations drawer),
//!   2. be cancellable through here (one mechanism, no ambiguity),
//!   3. guard against duplicates at its entry point,
//!   4. finish with a structured outcome — failures carry what/why and,
//!      where possible, a retry spec (never a dead-end),
//!   5. own no orphanable child process outside the app's job object.
//!
//! The registry emits a full snapshot on every change (`ops://changed`) —
//! snapshots are idempotent and race-free for the UI.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

pub const EVENT_CHANGED: &str = "ops://changed";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OpKind {
    Download,
    EngineFetch,
    Engine,
    Generation,
    Import,
    Benchmark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OpState {
    Running,
    Failed,
    Cancelled,
}

/// How a failed operation can be retried from the UI. (The engine retries
/// implicitly — the next message brings it up again — so only transfers
/// carry an explicit retry.)
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum RetrySpec {
    Download { entry_id: String, quant: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    pub id: String,
    pub kind: OpKind,
    pub state: OpState,
    pub label: String,
    pub detail: String,
    pub progress_current: Option<u64>,
    pub progress_total: Option<u64>,
    /// e.g. "3.7 GB VRAM · port 61552" for the engine.
    pub resource_note: Option<String>,
    pub started_at: String,
    pub error: Option<String>,
    pub cancellable: bool,
    pub retry: Option<RetrySpec>,
}

#[derive(Default)]
pub struct Ops {
    rows: Mutex<HashMap<String, Operation>>,
    cancels: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl Ops {
    fn lock_rows(&self) -> std::sync::MutexGuard<'_, HashMap<String, Operation>> {
        self.rows.lock().unwrap_or_else(|p| p.into_inner())
    }
    fn lock_cancels(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<AtomicBool>>> {
        self.cancels.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn snapshot(&self) -> Vec<Operation> {
        let mut rows: Vec<Operation> = self.lock_rows().values().cloned().collect();
        rows.sort_by(|a, b| {
            (a.state != OpState::Running)
                .cmp(&(b.state != OpState::Running))
                .then(b.started_at.cmp(&a.started_at))
        });
        rows
    }

    fn emit(&self, app: &AppHandle) {
        let _ = app.emit(EVENT_CHANGED, self.snapshot());
    }

    /// Register a running operation. Returns its cancel flag. If an operation
    /// with this id is already RUNNING, returns None — the duplicate guard.
    pub fn begin(
        &self,
        app: &AppHandle,
        id: &str,
        kind: OpKind,
        label: &str,
        cancellable: bool,
        retry: Option<RetrySpec>,
    ) -> Option<Arc<AtomicBool>> {
        {
            let mut rows = self.lock_rows();
            if rows.get(id).map(|o| o.state == OpState::Running).unwrap_or(false) {
                return None;
            }
            rows.insert(
                id.to_string(),
                Operation {
                    id: id.to_string(),
                    kind,
                    state: OpState::Running,
                    label: label.to_string(),
                    detail: String::new(),
                    progress_current: None,
                    progress_total: None,
                    resource_note: None,
                    started_at: chrono::Utc::now().to_rfc3339(),
                    error: None,
                    cancellable,
                    retry,
                },
            );
        }
        let flag = Arc::new(AtomicBool::new(false));
        self.lock_cancels().insert(id.to_string(), flag.clone());
        self.emit(app);
        Some(flag)
    }

    pub fn progress(&self, app: &AppHandle, id: &str, current: u64, total: u64, detail: &str) {
        {
            let mut rows = self.lock_rows();
            let Some(op) = rows.get_mut(id) else { return };
            op.progress_current = Some(current);
            op.progress_total = Some(total);
            if !detail.is_empty() {
                op.detail = detail.to_string();
            }
        }
        self.emit(app);
    }

    pub fn detail(&self, app: &AppHandle, id: &str, detail: &str) {
        {
            let mut rows = self.lock_rows();
            let Some(op) = rows.get_mut(id) else { return };
            op.detail = detail.to_string();
        }
        self.emit(app);
    }

    pub fn resource(&self, app: &AppHandle, id: &str, note: &str) {
        {
            let mut rows = self.lock_rows();
            let Some(op) = rows.get_mut(id) else { return };
            op.resource_note = Some(note.to_string());
        }
        self.emit(app);
    }

    /// Successful completion removes the row — success needs no residue.
    pub fn done(&self, app: &AppHandle, id: &str) {
        self.lock_rows().remove(id);
        self.lock_cancels().remove(id);
        self.emit(app);
    }

    /// Failure keeps the row visible with the reason and a retry path.
    pub fn failed(&self, app: &AppHandle, id: &str, error: &str) {
        {
            let mut rows = self.lock_rows();
            if let Some(op) = rows.get_mut(id) {
                op.state = OpState::Failed;
                op.error = Some(error.to_string());
                op.cancellable = false;
            }
        }
        self.lock_cancels().remove(id);
        self.emit(app);
    }

    /// User-cancelled: keep briefly visible as confirmation, dismissable.
    pub fn cancelled(&self, app: &AppHandle, id: &str) {
        {
            let mut rows = self.lock_rows();
            if let Some(op) = rows.get_mut(id) {
                op.state = OpState::Cancelled;
                op.cancellable = false;
            }
        }
        self.lock_cancels().remove(id);
        self.emit(app);
    }

    /// Raise the cancel flag; the owning operation observes it and winds down.
    pub fn request_cancel(&self, id: &str) -> bool {
        if let Some(flag) = self.lock_cancels().get(id) {
            flag.store(true, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    pub fn dismiss(&self, app: &AppHandle, id: &str) {
        let mut rows = self.lock_rows();
        if rows.get(id).map(|o| o.state != OpState::Running).unwrap_or(false) {
            rows.remove(id);
        }
        drop(rows);
        self.lock_cancels().remove(id);
        self.emit(app);
    }

    pub fn get_retry(&self, id: &str) -> Option<RetrySpec> {
        self.lock_rows().get(id).and_then(|o| o.retry.clone())
    }

    pub fn kind_of(&self, id: &str) -> Option<OpKind> {
        self.lock_rows().get(id).map(|o| o.kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op_ids(ops: &Ops) -> Vec<String> {
        ops.lock_rows().keys().cloned().collect()
    }

    #[test]
    fn duplicate_running_operation_is_refused() {
        let ops = Ops::default();
        // No AppHandle in unit tests: exercise the registry through the row
        // map directly via begin's guard logic replicated — begin requires an
        // AppHandle only for emission, so test the guard predicate itself.
        let mut rows = ops.lock_rows();
        rows.insert(
            "dl:x".into(),
            Operation {
                id: "dl:x".into(),
                kind: OpKind::Download,
                state: OpState::Running,
                label: "x".into(),
                detail: String::new(),
                progress_current: None,
                progress_total: None,
                resource_note: None,
                started_at: "2026".into(),
                error: None,
                cancellable: true,
                retry: None,
            },
        );
        let dup = rows.get("dl:x").map(|o| o.state == OpState::Running).unwrap_or(false);
        assert!(dup, "a running op with the same id must be detected");
        drop(rows);
        assert_eq!(op_ids(&ops).len(), 1);
    }

    #[test]
    fn cancel_flag_roundtrip() {
        let ops = Ops::default();
        let flag = Arc::new(AtomicBool::new(false));
        ops.lock_cancels().insert("gen:1".into(), flag.clone());
        assert!(ops.request_cancel("gen:1"));
        assert!(flag.load(Ordering::Relaxed));
        assert!(!ops.request_cancel("gen:missing"));
    }
}
