mod benchmark;
mod chat;
mod downloads;
mod error;
mod hardware;
mod metrics;
mod models;
mod ops;
mod portable;
mod preferences;
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

#[tauri::command]
async fn regenerate_reply(
    app: tauri::AppHandle,
    workspace_id: String,
    conversation_id: String,
) -> Result<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let llm = app.state::<Llm>();
        let ops = app.state::<Ops>();
        chat::regenerate(&app, &llm, &ops, &workspace_id, &conversation_id)
    })
    .await
    .map_err(|e| error::AthanorError::Chat(format!("regenerate task failed: {e}")))?
}

#[tauri::command]
async fn edit_and_resend(
    app: tauri::AppHandle,
    workspace_id: String,
    conversation_id: String,
    message_index: usize,
    content: String,
) -> Result<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let llm = app.state::<Llm>();
        let ops = app.state::<Ops>();
        chat::edit_and_resend(&app, &llm, &ops, &workspace_id, &conversation_id, message_index, content)
    })
    .await
    .map_err(|e| error::AthanorError::Chat(format!("edit task failed: {e}")))?
}

#[tauri::command]
fn fork_conversation(
    app: tauri::AppHandle,
    workspace_id: String,
    conversation_id: String,
    upto: usize,
) -> Result<String> {
    chat::fork(&app, &workspace_id, &conversation_id, upto)
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
fn rename_conversation(
    app: tauri::AppHandle,
    workspace_id: String,
    conversation_id: String,
    title: String,
) -> Result<Vec<chat::ConversationMeta>> {
    chat::rename(&app, &workspace_id, &conversation_id, &title)
}

#[tauri::command]
async fn search_conversations(
    app: tauri::AppHandle,
    workspace_id: String,
    query: String,
) -> Result<Vec<chat::SearchHit>> {
    // Off the UI thread — a big workspace's scan shouldn't stutter typing.
    tauri::async_runtime::spawn_blocking(move || chat::search(&app, &workspace_id, &query))
        .await
        .map_err(|e| error::AthanorError::Chat(format!("search task failed: {e}")))?
}

#[tauri::command]
fn export_conversation(
    app: tauri::AppHandle,
    workspace_id: String,
    conversation_id: String,
    format: String,
    dest: String,
) -> Result<()> {
    chat::export(&app, &workspace_id, &conversation_id, &format, &dest)
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

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateCheck {
    current_version: String,
    available: Option<String>,
    note: String,
}

/// Update check via the signed-updater plugin. Until the release endpoint is
/// live this reports honestly instead of erroring cryptically.
#[tauri::command]
async fn check_for_update(app: tauri::AppHandle) -> UpdateCheck {
    use tauri_plugin_updater::UpdaterExt;
    let current_version = app.package_info().version.to_string();
    match app.updater() {
        Ok(updater) => match updater.check().await {
            Ok(Some(update)) => UpdateCheck {
                current_version,
                available: Some(update.version.clone()),
                note: "an update is available".into(),
            },
            Ok(None) => UpdateCheck {
                current_version,
                available: None,
                note: "you are on the latest version".into(),
            },
            Err(e) => {
                log::info!(target: "updater", "check failed (endpoint likely not live yet): {e}");
                UpdateCheck {
                    current_version,
                    available: None,
                    note: "update service not reachable yet — this build predates the release channel".into(),
                }
            }
        },
        Err(e) => UpdateCheck {
            current_version,
            available: None,
            note: format!("updater unavailable: {e}"),
        },
    }
}

#[tauri::command]
fn get_preferences(app: tauri::AppHandle) -> Result<preferences::Preferences> {
    preferences::load(&app)
}

#[tauri::command]
fn set_accent(app: tauri::AppHandle, accent: String) -> Result<preferences::Preferences> {
    preferences::set_accent(&app, &accent)
}

#[tauri::command]
fn get_data_root(app: tauri::AppHandle) -> Result<String> {
    Ok(workspaces::data_root(&app)?.to_string_lossy().to_string())
}

#[tauri::command]
fn is_portable() -> bool {
    portable::is_portable()
}

// ── Speed benchmark ───────────────────────────────────────────

#[tauri::command]
async fn run_benchmark(
    app: tauri::AppHandle,
    model_sha: String,
    model_name: String,
) -> Result<benchmark::BenchResult> {
    tauri::async_runtime::spawn_blocking(move || {
        let llm = app.state::<Llm>();
        let ops = app.state::<Ops>();
        benchmark::run(&app, &llm, &ops, &model_sha, &model_name)
    })
    .await
    .map_err(|e| error::AthanorError::Chat(format!("benchmark task failed: {e}")))?
}

#[tauri::command]
fn list_benchmarks(app: tauri::AppHandle) -> Result<Vec<benchmark::BenchResult>> {
    benchmark::list(&app)
}

/// Open the app's data folder in the OS file manager. Cross-platform.
#[tauri::command]
fn reveal_data_root(app: tauri::AppHandle) -> Result<()> {
    let dir = workspaces::data_root(&app)?;
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = std::process::Command::new("explorer");
        c.arg(&dir);
        c
    };
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = std::process::Command::new("open");
        c.arg(&dir);
        c
    };
    #[cfg(target_os = "linux")]
    let mut cmd = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(&dir);
        c
    };
    // explorer.exe returns a non-zero status even on success; just launch it.
    let _ = cmd.spawn();
    Ok(())
}

/// The only external pages the app will ever open — a hard allowlist, not a
/// general-purpose opener. Anything else is refused.
const ALLOWED_LINKS: &[&str] = &["https://bbasecure.com"];

/// Open an allowlisted page in the system browser (Lite's maker's-mark link).
#[tauri::command]
fn open_link(url: String) -> Result<()> {
    if !ALLOWED_LINKS.contains(&url.as_str()) {
        return Err(error::AthanorError::Runtime(format!(
            "refusing to open non-allowlisted url: {url}"
        )));
    }
    #[cfg(target_os = "windows")]
    let mut cmd = {
        use std::os::windows::process::CommandExt;
        // `start` needs an explicit (empty) window title before the url.
        let mut c = std::process::Command::new("cmd");
        c.args(["/c", "start", "", &url]);
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        c
    };
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = std::process::Command::new("open");
        c.arg(&url);
        c
    };
    #[cfg(target_os = "linux")]
    let mut cmd = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(&url);
        c
    };
    let _ = cmd.spawn();
    Ok(())
}

#[tauri::command]
fn rotate_api_key(app: tauri::AppHandle, llm: tauri::State<'_, Llm>) -> Result<runtime::api::ApiInfo> {
    runtime::api::rotate_key(&app, &llm)
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

/// Benchmark self-test: run the real speed suite against the installed model
/// and confirm it produces measured (non-zero) throughput on this hardware.
#[cfg(debug_assertions)]
fn selftest_benchmark(app: &tauri::AppHandle) -> Result<String> {
    let model = downloads::ensure_installed(app, "llama-3.2-3b-instruct", "Q4_K_M")?;
    let llm = app.state::<Llm>();
    let ops = app.state::<Ops>();
    let r = benchmark::run(app, &llm, &ops, &model.sha256, &model.display_name)?;
    if r.gen_tps <= 0.0 || r.prompts == 0 {
        return Err(error::AthanorError::Chat(format!("benchmark produced no throughput: {r:?}")));
    }
    Ok(format!(
        "gen={:.1} tok/s · prompt={:.1} tok/s · ttft={} ms · gpu={} · prompts={}",
        r.gen_tps, r.prompt_tps, r.ttft_ms, r.gpu_active, r.prompts
    ))
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
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .max_file_size(1_000_000)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepAll)
                .targets({
                    // Portable installs keep logs beside the executable; a normal
                    // install uses the OS log directory.
                    let file_target = match portable::portable_root() {
                        Some(root) => tauri_plugin_log::TargetKind::Folder {
                            path: root.join("logs"),
                            file_name: Some("athanor".into()),
                        },
                        None => tauri_plugin_log::TargetKind::LogDir {
                            file_name: Some("athanor".into()),
                        },
                    };
                    [
                        tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                        tauri_plugin_log::Target::new(file_target),
                    ]
                })
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
                            "benchmark" => selftest_benchmark(&handle),
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
                "Athanor {} online ({} mode, data root: {:?})",
                env!("CARGO_PKG_VERSION"),
                if portable::is_portable() { "portable" } else { "installed" },
                workspaces::data_root(app.handle()).ok()
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
            start_download,
            cancel_download,
            list_library,
            delete_model,
            chat_send,
            cancel_generation,
            list_conversations,
            get_conversation,
            delete_conversation,
            rename_conversation,
            search_conversations,
            export_conversation,
            regenerate_reply,
            edit_and_resend,
            fork_conversation,
            get_metrics_settings,
            set_metrics_share,
            get_metrics_history,
            get_metrics_sample,
            get_ollama_status,
            import_ollama,
            get_api_info,
            set_api_expose,
            start_engine,
            get_preferences,
            set_accent,
            get_data_root,
            reveal_data_root,
            open_link,
            is_portable,
            run_benchmark,
            list_benchmarks,
            rotate_api_key,
            check_for_update,
            list_operations,
            cancel_operation,
            dismiss_operation,
            retry_operation
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // No child engine outlives the app.
            // (The job object is the hard guarantee; this is the clean path.)
            if let tauri::RunEvent::Exit = event {
                let llm = app.state::<Llm>();
                // Take the engine out of the slot first so the lock guard drops
                // before the state handle does.
                let active = llm.lock().take();
                if let Some(mut active) = active {
                    log::info!(target: "rt", "app exit: stopping llama-server");
                    let _ = active.child.kill();
                    let _ = active.child.wait();
                }
            }
        });
}
