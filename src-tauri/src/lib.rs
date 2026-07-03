mod error;
mod hardware;
mod models;
mod workspaces;

use error::Result;
use hardware::HardwareReport;
use models::recommend::RecommendationSet;
use models::Catalog;
use tauri::Manager;
use workspaces::{Workspace, WorkspaceList, WsLock};

#[tauri::command]
async fn detect_hardware() -> Result<HardwareReport> {
    tauri::async_runtime::spawn_blocking(hardware::detect)
        .await
        .map_err(|e| error::CondereError::Hardware(format!("probe task failed: {e}")))?
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
    let _guard = lock.0.lock().unwrap();
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
    let _guard = lock.0.lock().unwrap();
    workspaces::create(&app, &name, &purpose, accent_hue, &glyph)
}

#[tauri::command]
fn activate_workspace(
    app: tauri::AppHandle,
    lock: tauri::State<'_, WsLock>,
    id: String,
) -> Result<Workspace> {
    let _guard = lock.0.lock().unwrap();
    workspaces::activate(&app, &id)
}

#[tauri::command]
fn delete_workspace(
    app: tauri::AppHandle,
    lock: tauri::State<'_, WsLock>,
    id: String,
) -> Result<WorkspaceList> {
    let _guard = lock.0.lock().unwrap();
    workspaces::delete(&app, &id)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir {
                        file_name: Some("condere".into()),
                    }),
                ])
                .build(),
        )
        .manage(WsLock::default())
        .setup(|app| {
            let handle = app.handle().clone();
            std::thread::Builder::new()
                .name("hw-telemetry".into())
                .spawn(move || hardware::telemetry::run(handle))
                .expect("telemetry thread must spawn");

            log::info!(
                "Condere {} online (data root: {:?})",
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
            delete_workspace
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
