mod chat;
mod downloads;
mod error;
mod hardware;
mod metrics;
mod models;
mod ops;
mod runtime;
mod workspaces;

use downloads::LibraryModel;
use error::Result;
use hardware::HardwareReport;
use models::recommend::RecommendationSet;
use models::Catalog;
use ops::Ops;
use runtime::server::Llm;
use tauri::Manager;
use workspaces::{Workspace, WorkspaceList, WsLock};

#[tauri::command]
async fn chat_send(
    app: tauri::AppHandle,
    workspace_id: String,
    conversation_id: Option<String>,
    message: String,
) -> Result<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let llm = app.state::<Llm>();
        let ops = app.state::<Ops>();
        chat::send(&app, &llm, &ops, &workspace_id, conversation_id, message)
    })
    .await
    .map_err(|e| error::AthanorError::Chat(format!("chat task failed: {e}")))?
}

#[tauri::command]
fn cancel_generation(ops: tauri::State<'_, Ops>, conversation_id: String) {
    chat::cancel(&ops, &conversation_id);
}

// ── Operations registry surface ───────────────────────────────

#[tauri::command]
fn list_operations(ops: tauri::State<'_, Ops>) -> Vec<ops::Operation> {
    ops.snapshot()
}

#[tauri::command]
fn cancel_operation(
    app: tauri::AppHandle,
    ops: tauri::State<'_, Ops>,
    llm: tauri::State<'_, Llm>,
    id: String,
) {
    // The engine "cancels" by stopping (it isn't waiting on anything);
    // everything else observes its cancel flag and winds down.
    if ops.kind_of(&id) == Some(ops::OpKind::Engine) {
        runtime::server::stop(&app, &llm);
    } else {
        ops.request_cancel(&id);
    }
}

#[tauri::command]
fn dismiss_operation(app: tauri::AppHandle, ops: tauri::State<'_, Ops>, id: String) {
    ops.dismiss(&app, &id);
}

#[tauri::command]
fn retry_operation(app: tauri::AppHandle, ops: tauri::State<'_, Ops>, id: String) -> Result<()> {
    let Some(retry) = ops.get_retry(&id) else {
        return Err(error::AthanorError::Chat("this operation has no retry".into()));
    };
    ops.dismiss(&app, &id);
    match retry {
        ops::RetrySpec::Download { entry_id, quant } => downloads::start(app, &entry_id, &quant),
    }
}

#[tauri::command]
fn list_conversations(
    app: tauri::AppHandle,
    workspace_id: String,
) -> Result<Vec<chat::ConversationMeta>> {
    chat::list(&app, &workspace_id)
}

#[tauri::command]
fn get_conversation(
    app: tauri::AppHandle,
    workspace_id: String,
    conversation_id: String,
) -> Result<chat::Conversation> {
    chat::load(&app, &workspace_id, &conversation_id)
}

#[tauri::command]
fn delete_conversation(
    app: tauri::AppHandle,
    workspace_id: String,
    conversation_id: String,
) -> Result<Vec<chat::ConversationMeta>> {
    chat::delete(&app, &workspace_id, &conversation_id)
}

#[tauri::command]
fn stop_engine(app: tauri::AppHandle, llm: tauri::State<'_, Llm>) {
    runtime::server::stop(&app, &llm);
}

#[tauri::command]
fn get_metrics_settings(app: tauri::AppHandle) -> metrics::MetricsSettings {
    metrics::get_settings(&app)
}

#[tauri::command]
fn set_metrics_share(app: tauri::AppHandle, share: bool) -> Result<metrics::MetricsSettings> {
    metrics::set_share(&app, share)
}

#[tauri::command]
fn get_metrics_history(app: tauri::AppHandle, limit: usize) -> Result<Vec<metrics::MetricsRecord>> {
    metrics::history(&app, limit.min(500))
}

#[tauri::command]
fn get_metrics_sample(app: tauri::AppHandle) -> Result<serde_json::Value> {
    metrics::sample_shared_payload(&app)
}

#[tauri::command]
fn get_ollama_status() -> downloads::ollama::OllamaStatus {
    downloads::ollama::status()
}

#[tauri::command]
async fn import_ollama(app: tauri::AppHandle) -> Result<downloads::ollama::ImportReport> {
    tauri::async_runtime::spawn_blocking(move || {
        let ops = app.state::<Ops>();
        let _ = ops
            .begin(&app, "import:ollama", ops::OpKind::Import, "Import from Ollama", false, None)
            .ok_or_else(|| error::AthanorError::Download("import already running".into()))?;
        let result = downloads::ollama::import(&app);
        match &result {
            Ok(_) => ops.done(&app, "import:ollama"),
            Err(e) => ops.failed(&app, "import:ollama", &e.to_string()),
        }
        result
    })
    .await
    .map_err(|e| error::AthanorError::Download(format!("import task failed: {e}")))?
}

#[tauri::command]
fn get_api_info(app: tauri::AppHandle, llm: tauri::State<'_, Llm>) -> Result<runtime::api::ApiInfo> {
    runtime::api::info(&app, &llm)
}

#[tauri::command]
fn set_api_expose(
    app: tauri::AppHandle,
    llm: tauri::State<'_, Llm>,
    expose: bool,
) -> Result<runtime::api::ApiInfo> {
    runtime::api::set_expose(&app, expose)?;
    // A running engine keeps its current binding; the new setting applies at
    // the next engine start. Stop it so the next chat restarts on the stable
    // port — least surprise for "expose it now".
    runtime::server::stop(&app, &llm);
    runtime::api::info(&app, &llm)
}

#[tauri::command]
async fn start_engine(app: tauri::AppHandle, workspace_id: String) -> Result<()> {
    tauri::async_runtime::spawn_blocking(move || {
        let ws_list = workspaces::list(&app)?;
        let ws = ws_list
            .workspaces
            .iter()
            .find(|w| w.id == workspace_id)
            .ok_or_else(|| error::AthanorError::Workspace("workspace not found".into()))?;
        let sha = ws
            .active_model
            .clone()
            .ok_or_else(|| error::AthanorError::Chat("no model selected for this workspace".into()))?;
        let llm = app.state::<Llm>();
        runtime::server::ensure(&app, &llm, &sha).map(|_| ())
    })
    .await
    .map_err(|e| error::AthanorError::Runtime(format!("engine task failed: {e}")))?
}

#[tauri::command]
fn onboarding_needed(app: tauri::AppHandle) -> Result<bool> {
    Ok(!workspaces::data_root(&app)?.join(".onboarded").exists())
}

#[tauri::command]
fn set_onboarded(app: tauri::AppHandle) -> Result<()> {
    std::fs::write(workspaces::data_root(&app)?.join(".onboarded"), b"1")?;
    Ok(())
}

#[tauri::command]
fn start_download(app: tauri::AppHandle, entry_id: String, quant: String) -> Result<()> {
    downloads::start(app, &entry_id, &quant)
}

#[tauri::command]
fn cancel_download(ops: tauri::State<'_, Ops>, sha256: String) {
    ops.request_cancel(&downloads::op_id(&sha256));
}

#[tauri::command]
fn list_library(app: tauri::AppHandle) -> Result<Vec<LibraryModel>> {
    downloads::list_library(&app)
}

#[tauri::command]
fn delete_model(app: tauri::AppHandle, sha256: String) -> Result<Vec<LibraryModel>> {
    downloads::delete_model(&app, &sha256)
}

#[tauri::command]
async fn detect_hardware() -> Result<HardwareReport> {
    tauri::async_runtime::spawn_blocking(hardware::detect)
        .await
        .map_err(|e| error::AthanorError::Hardware(format!("probe task failed: {e}")))?
}

#[tauri::command]
fn get_recommendations(report: HardwareReport) -> Result<RecommendationSet> {
    models::recommend::recommend(&report)
}

#[tauri::command]
fn get_model_catalog() -> Result<Catalog> {
    models::catalog().cloned()
}

#[tauri::command]
fn list_workspaces(app: tauri::AppHandle, lock: tauri::State<'_, WsLock>) -> Result<WorkspaceList> {
    let _guard = lock.acquire();
    workspaces::list(&app)
}

#[tauri::command]
fn create_workspace(
    app: tauri::AppHandle,
    lock: tauri::State<'_, WsLock>,
    name: String,
    purpose: String,
    accent_hue: u16,
    glyph: String,
) -> Result<Workspace> {
    let _guard = lock.acquire();
    workspaces::create(&app, &name, &purpose, accent_hue, &glyph)
}

#[tauri::command]
fn activate_workspace(
    app: tauri::AppHandle,
    lock: tauri::State<'_, WsLock>,
    id: String,
) -> Result<Workspace> {
    let _guard = lock.acquire();
    workspaces::activate(&app, &id)
}

#[tauri::command]
fn set_workspace_model(
    app: tauri::AppHandle,
    lock: tauri::State<'_, WsLock>,
    id: String,
    sha256: Option<String>,
) -> Result<Workspace> {
    let _guard = lock.acquire();
    workspaces::set_active_model(&app, &id, sha256)
}

#[tauri::command]
fn delete_workspace(
    app: tauri::AppHandle,
    lock: tauri::State<'_, WsLock>,
    id: String,
) -> Result<WorkspaceList> {
    let _guard = lock.acquire();
    workspaces::delete(&app, &id)
}

/// The dev self-test: the exact production code path from empty machine to
/// streamed generation, with no UI interaction.
#[cfg(debug_assertions)]
fn selftest_chat(app: &tauri::AppHandle) -> Result<String> {
    let model = downloads::ensure_installed(app, "llama-3.2-3b-instruct", "Q4_K_M")?;
    log::info!("SELFTEST: model ready ({})", model.display_name);

    let ws_list = workspaces::list(app)?;
    let ws = match ws_list.workspaces.iter().find(|w| w.name == "Self Test") {
        Some(w) => w.clone(),
        None => workspaces::create(app, "Self Test", "pipeline verification", 275, "S")?,
    };
    workspaces::set_active_model(app, &ws.id, Some(model.sha256.clone()))?;

    let llm = app.state::<Llm>();
    let ops = app.state::<Ops>();
    let conv_id = chat::send(
        app,
        &llm,
        &ops,
        &ws.id,
        None,
        "Reply with exactly the two words: IGNITION CONFIRMED".into(),
    )?;
    let conv = chat::load(app, &ws.id, &conv_id)?;
    let last = conv
        .messages
        .last()
        .ok_or_else(|| error::AthanorError::Chat("no messages saved".into()))?;
    if last.role != "assistant" || last.content.trim().is_empty() {
        return Err(error::AthanorError::Chat("assistant reply missing or empty".into()));
    }
    Ok(format!(
        "reply={:?} stats={:?}",
        last.content.trim().chars().take(80).collect::<String>(),
        last.stats
    ))
}

/// Serve self-test: engine up, then hold forever (killed externally).
#[cfg(debug_assertions)]
fn selftest_serve(app: &tauri::AppHandle) -> Result<String> {
    let model = downloads::ensure_installed(app, "llama-3.2-3b-instruct", "Q4_K_M")?;
    let ws_list = workspaces::list(app)?;
    let ws = match ws_list.workspaces.iter().find(|w| w.name == "Self Test") {
        Some(w) => w.clone(),
        None => workspaces::create(app, "Self Test", "pipeline verification", 275, "S")?,
    };
    workspaces::set_active_model(app, &ws.id, Some(model.sha256.clone()))?;
    let llm = app.state::<Llm>();
    let port = runtime::server::ensure(app, &llm, &model.sha256)?;
    Ok(format!("engine on port {port}"))
}

/// Import self-test: scan the machine's real Ollama store, import in place,
/// verify the imported models appear in the library with valid paths.
#[cfg(debug_assertions)]
fn selftest_import(app: &tauri::AppHandle) -> Result<String> {
    let status = downloads::ollama::status();
    let report = downloads::ollama::import(app)?;
    let library = downloads::list_library(app)?;
    let ollama_models: Vec<_> = library.iter().filter(|m| m.source == "ollama").collect();
    for m in &ollama_models {
        if !std::path::Path::new(&m.path).exists() {
            return Err(error::AthanorError::Download(format!(
                "imported model has dangling path: {}",
                m.path
            )));
        }
    }
    Ok(format!(
        "ollama available={} found={} imported={} already={} skipped={:?} in_library={}",
        status.available,
        report.found,
        report.imported,
        report.already_in_library,
        report.skipped,
        ollama_models.len()
    ))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // A second launch focuses the existing window instead of racing the
        // first process on the same data files.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.set_focus();
                let _ = win.unminimize();
            }
        }))
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .max_file_size(1_000_000)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepAll)
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir {
                        file_name: Some("athanor".into()),
                    }),
                ])
                .build(),
        )
        .manage(WsLock::default())
        .manage(Llm::default())
        .manage(Ops::default())
        .setup(|app| {
            let handle = app.handle().clone();
            if let Err(e) = std::thread::Builder::new()
                .name("hw-telemetry".into())
                .spawn(move || hardware::telemetry::run(handle))
            {
                // Telemetry is a degradable subsystem, never a startup failure.
                log::error!("telemetry sampler unavailable: {e}");
            }

            let handle = app.handle().clone();
            std::thread::Builder::new()
                .name("housekeeping".into())
                .spawn(move || {
                    // Zero-orphan sweep first: kill any engine left over from
                    // a previous session (pre-job-object builds or machine
                    // crashes), then purge expired trash.
                    if let Ok(root) = workspaces::data_root(&handle) {
                        runtime::guard::sweep_orphans(&root.join("runtimes"));
                    }
                    workspaces::purge_trash(&handle);
                })
                .ok();

            // Headless full-pipeline self-test (dev builds only):
            // ATHANOR_SELFTEST=chat -> install small model, start engine,
            // generate, log SELFTEST PASS/FAIL, exit.
            #[cfg(debug_assertions)]
            if let Ok(mode) = std::env::var("ATHANOR_SELFTEST") {
                let handle = app.handle().clone();
                std::thread::Builder::new()
                    .name("selftest".into())
                    .spawn(move || {
                        std::thread::sleep(std::time::Duration::from_secs(2));
                        let result = match mode.as_str() {
                            "chat" => selftest_chat(&handle),
                            "import" => selftest_import(&handle),
                            "serve" => {
                                // Bring the engine up and HOLD — used by the
                                // orphan-guard test (hard-kill the app, then
                                // verify no llama-server survives).
                                match selftest_serve(&handle) {
                                    Ok(msg) => {
                                        log::info!("SELFTEST SERVING: {msg}");
                                        loop {
                                            std::thread::sleep(std::time::Duration::from_secs(60));
                                        }
                                    }
                                    Err(e) => Err(e),
                                }
                            }
                            other => Err(error::AthanorError::Chat(format!(
                                "unknown selftest mode {other:?}"
                            ))),
                        };
                        let code = match result {
                            Ok(summary) => {
                                log::info!("SELFTEST PASS: {summary}");
                                0
                            }
                            Err(e) => {
                                log::error!("SELFTEST FAIL: {e}");
                                1
                            }
                        };
                        handle.exit(code);
                    })
                    .ok();
            }

            log::info!(
                "Athanor {} online (data root: {:?})",
                env!("CARGO_PKG_VERSION"),
                app.path().app_data_dir().ok()
            );
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            detect_hardware,
            get_recommendations,
            get_model_catalog,
            list_workspaces,
            create_workspace,
            activate_workspace,
            set_workspace_model,
            delete_workspace,
            start_download,
            cancel_download,
            list_library,
            delete_model,
            chat_send,
            cancel_generation,
            list_conversations,
            get_conversation,
            delete_conversation,
            stop_engine,
            get_metrics_settings,
            set_metrics_share,
            get_metrics_history,
            get_metrics_sample,
            get_ollama_status,
            import_ollama,
            get_api_info,
            set_api_expose,
            start_engine,
            onboarding_needed,
            set_onboarded,
            list_operations,
            cancel_operation,
            dismiss_operation,
            retry_operation
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // The engine must never outlive the app.
            if let tauri::RunEvent::Exit = event {
                let llm = app.state::<Llm>();
                let mut guard = llm.lock();
                if let Some(mut active) = guard.take() {
                    log::info!(target: "rt", "app exit: stopping llama-server");
                    let _ = active.child.kill();
                    let _ = active.child.wait();
                }
            }
        });
}
