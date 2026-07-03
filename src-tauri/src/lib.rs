mod downloads;
mod error;
mod hardware;
mod models;
mod workspaces;

use downloads::{Downloads, LibraryModel};
use error::Result;
use hardware::HardwareReport;
use models::recommend::RecommendationSet;
use models::Catalog;
use tauri::Manager;
use workspaces::{Workspace, WorkspaceList, WsLock};

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
            delete_model
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
