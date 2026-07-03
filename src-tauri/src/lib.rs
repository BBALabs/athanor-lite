mod chat;
mod downloads;
mod error;
mod hardware;
mod metrics;
mod models;
mod runtime;
mod workspaces;

use chat::ChatCancels;
use downloads::{Downloads, LibraryModel};
use error::Result;
use hardware::HardwareReport;
use models::recommend::RecommendationSet;
use models::Catalog;
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
        let cancels = app.state::<ChatCancels>();
        chat::send(&app, &llm, &cancels, &workspace_id, conversation_id, message)
    })
    .await
    .map_err(|e| error::AthanorError::Chat(format!("chat task failed: {e}")))?
}

#[tauri::command]
fn cancel_generation(cancels: tauri::State<'_, ChatCancels>, conversation_id: String) {
    chat::cancel(&cancels, &conversation_id);
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
fn start_download(
    app: tauri::AppHandle,
    registry: tauri::State<'_, Downloads>,
    entry_id: String,
    quant: String,
) -> Result<()> {
    downloads::start(app, &registry, &entry_id, &quant)
}

#[tauri::command]
fn cancel_download(registry: tauri::State<'_, Downloads>, sha256: String) {
    downloads::cancel(&registry, &sha256);
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
    let cancels = app.state::<ChatCancels>();
    let conv_id = chat::send(
        app,
        &llm,
        &cancels,
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
        .manage(Downloads::default())
        .manage(Llm::default())
        .manage(ChatCancels::default())
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
                .name("trash-purge".into())
                .spawn(move || workspaces::purge_trash(&handle))
                .ok();

            // Headless full-pipeline self-test (dev builds only):
            // ATHANOR_SELFTEST=chat -> install small model, start engine,
            // generate, log SELFTEST PASS/FAIL, exit.
            #[cfg(debug_assertions)]
            if std::env::var("ATHANOR_SELFTEST").as_deref() == Ok("chat") {
                let handle = app.handle().clone();
                std::thread::Builder::new()
                    .name("selftest".into())
                    .spawn(move || {
                        std::thread::sleep(std::time::Duration::from_secs(2));
                        let code = match selftest_chat(&handle) {
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
            get_metrics_sample
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
