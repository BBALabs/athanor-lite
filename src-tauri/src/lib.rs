mod benchmark;
mod chat;
mod downloads;
mod error;
mod hardware;
mod mcp;
mod metrics;
mod models;
mod ops;
mod portable;
mod preferences;
mod prompts;
mod rag;
mod share;
mod runtime;
mod training;
mod uistate;
mod workspaces;

use downloads::LibraryModel;
use error::Result;
use hardware::HardwareReport;
use mcp::McpManager;
use models::recommend::RecommendationSet;
use models::Catalog;
use ops::Ops;
use rag::embed::Embedder;
use runtime::server::Llm;
use tauri::Manager;
use workspaces::{Workspace, WorkspaceList, WsLock};

// ── Knowledge base (RAG) commands ─────────────────────────────

#[tauri::command]
fn get_knowledge_base(app: tauri::AppHandle, workspace_id: String) -> Result<rag::KnowledgeBase> {
    rag::knowledge_base(&app, &workspace_id)
}

#[tauri::command]
async fn add_documents(
    app: tauri::AppHandle,
    workspace_id: String,
    paths: Vec<String>,
) -> Result<()> {
    tauri::async_runtime::spawn_blocking(move || {
        let embedder = app.state::<Embedder>();
        for path in paths {
            // Each document is its own registered, cancellable operation.
            if let Err(e) = rag::add_document(&app, &embedder, &workspace_id, &path) {
                log::warn!(target: "rag", "index of {path} ended: {e}");
            }
        }
        Ok(())
    })
    .await
    .map_err(|e| error::AthanorError::Rag(format!("indexing task failed: {e}")))?
}

#[tauri::command]
fn cancel_indexing(app: tauri::AppHandle, workspace_id: String, doc_id: String) {
    rag::cancel_indexing(&app, &workspace_id, &doc_id);
}

#[tauri::command]
fn remove_document(
    app: tauri::AppHandle,
    workspace_id: String,
    doc_id: String,
) -> Result<rag::KnowledgeBase> {
    rag::remove_document(&app, &workspace_id, &doc_id)
}

#[tauri::command]
fn set_retrieval_enabled(
    app: tauri::AppHandle,
    workspace_id: String,
    enabled: bool,
) -> Result<rag::KnowledgeBase> {
    rag::set_retrieval_enabled(&app, &workspace_id, enabled)
}

#[tauri::command]
async fn preview_chunks(
    app: tauri::AppHandle,
    workspace_id: String,
    doc_id: String,
) -> Result<Vec<rag::Source>> {
    tauri::async_runtime::spawn_blocking(move || rag::preview_chunks(&app, &workspace_id, &doc_id))
        .await
        .map_err(|e| error::AthanorError::Rag(format!("preview task failed: {e}")))?
}

#[tauri::command]
fn stop_embedder(app: tauri::AppHandle, embedder: tauri::State<'_, Embedder>) {
    rag::embed::stop(&app, &embedder);
}

// ── MCP commands ──────────────────────────────────────────────

#[tauri::command]
fn list_mcp_servers(
    app: tauri::AppHandle,
    workspace_id: String,
) -> Result<Vec<mcp::McpServerView>> {
    mcp::list_servers(&app, &workspace_id)
}

#[tauri::command]
fn save_mcp_server(
    app: tauri::AppHandle,
    workspace_id: String,
    config: mcp::McpServerConfig,
) -> Result<Vec<mcp::McpServerView>> {
    mcp::save_server(&app, &workspace_id, config)
}

#[tauri::command]
fn remove_mcp_server(
    app: tauri::AppHandle,
    mgr: tauri::State<'_, McpManager>,
    workspace_id: String,
    server_id: String,
) -> Result<Vec<mcp::McpServerView>> {
    mcp::remove_server(&app, &mgr, &workspace_id, &server_id)
}

#[tauri::command]
async fn connect_mcp_server(
    app: tauri::AppHandle,
    workspace_id: String,
    server_id: String,
) -> Result<mcp::McpServerView> {
    tauri::async_runtime::spawn_blocking(move || {
        let mgr = app.state::<McpManager>();
        mcp::connect(&app, &mgr, &workspace_id, &server_id)
    })
    .await
    .map_err(|e| error::AthanorError::Mcp(format!("connect task failed: {e}")))?
}

#[tauri::command]
fn disconnect_mcp_server(
    app: tauri::AppHandle,
    mgr: tauri::State<'_, McpManager>,
    server_id: String,
) {
    mcp::disconnect(&app, &mgr, &server_id);
}

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
fn onboarding_needed(app: tauri::AppHandle) -> Result<bool> {
    Ok(!workspaces::data_root(&app)?.join(".onboarded").exists())
}

#[tauri::command]
fn set_onboarded(app: tauri::AppHandle) -> Result<()> {
    std::fs::write(workspaces::data_root(&app)?.join(".onboarded"), b"1")?;
    Ok(())
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

// ── Fine-tuning: dataset studio ───────────────────────────────

#[tauri::command]
async fn import_dataset(
    app: tauri::AppHandle,
    workspace_id: String,
    name: String,
    path: String,
) -> Result<training::DatasetReport> {
    tauri::async_runtime::spawn_blocking(move || training::import(&app, &workspace_id, &name, &path))
        .await
        .map_err(|e| error::AthanorError::Workspace(format!("import task failed: {e}")))?
}

#[tauri::command]
fn list_datasets(app: tauri::AppHandle, workspace_id: String) -> Result<Vec<training::DatasetMeta>> {
    training::list(&app, &workspace_id)
}

#[tauri::command]
fn delete_dataset(
    app: tauri::AppHandle,
    workspace_id: String,
    id: String,
) -> Result<Vec<training::DatasetMeta>> {
    training::delete(&app, &workspace_id, &id)
}

#[tauri::command]
fn get_trainer_status() -> training::TrainerStatus {
    training::trainer_status()
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

// ── System prompt library ─────────────────────────────────────

#[tauri::command]
fn get_curated_prompts() -> Result<&'static prompts::CuratedSet> {
    prompts::curated()
}

#[tauri::command]
fn list_user_prompts(app: tauri::AppHandle) -> Result<Vec<prompts::UserPrompt>> {
    prompts::list_user(&app)
}

#[tauri::command]
fn save_user_prompt(
    app: tauri::AppHandle,
    id: Option<String>,
    title: String,
    category: String,
    body: String,
) -> Result<Vec<prompts::UserPrompt>> {
    prompts::save_user(&app, id, &title, &category, &body)
}

#[tauri::command]
fn delete_user_prompt(app: tauri::AppHandle, id: String) -> Result<Vec<prompts::UserPrompt>> {
    prompts::delete_user(&app, &id)
}

#[tauri::command]
fn set_workspace_system_prompt(
    app: tauri::AppHandle,
    lock: tauri::State<'_, WsLock>,
    id: String,
    prompt: Option<String>,
) -> Result<Workspace> {
    let _guard = lock.acquire();
    workspaces::set_system_prompt(&app, &id, prompt)
}

// ── Workspace sharing ─────────────────────────────────────────

#[tauri::command]
fn export_workspace_filename(app: tauri::AppHandle, id: String) -> Result<String> {
    let m = share::build_manifest(&app, &id)?;
    Ok(share::export_filename(&m.name))
}

#[tauri::command]
fn export_workspace(app: tauri::AppHandle, id: String, dest: String) -> Result<()> {
    share::export(&app, &id, &dest)
}

#[tauri::command]
fn import_workspace(
    app: tauri::AppHandle,
    lock: tauri::State<'_, WsLock>,
    path: String,
) -> Result<share::ImportResult> {
    let _guard = lock.acquire();
    share::import(&app, &path)
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

#[tauri::command]
fn rotate_api_key(app: tauri::AppHandle, llm: tauri::State<'_, Llm>) -> Result<runtime::api::ApiInfo> {
    runtime::api::rotate_key(&app, &llm)
}

#[tauri::command]
fn get_coach_state(app: tauri::AppHandle) -> Result<uistate::CoachState> {
    uistate::load(&app)
}

#[tauri::command]
fn coach_mark_seen(app: tauri::AppHandle, id: String) -> Result<uistate::CoachState> {
    uistate::mark_seen(&app, &id)
}

#[tauri::command]
fn coach_reset(app: tauri::AppHandle) -> Result<uistate::CoachState> {
    uistate::reset(&app)
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
    template_id: Option<String>,
) -> Result<Workspace> {
    let _guard = lock.acquire();
    workspaces::create(&app, &name, &purpose, accent_hue, &glyph, template_id)
}

#[tauri::command]
fn get_templates() -> Result<&'static models::templates::TemplateSet> {
    models::templates::templates()
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
        None => workspaces::create(app, "Self Test", "pipeline verification", 275, "S", None)?,
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
        None => workspaces::create(app, "Self Test", "pipeline verification", 275, "S", None)?,
    };
    workspaces::set_active_model(app, &ws.id, Some(model.sha256.clone()))?;
    let llm = app.state::<Llm>();
    let port = runtime::server::ensure(app, &llm, &model.sha256)?;
    Ok(format!("engine on port {port}"))
}

/// RAG self-test: index a document with a unique fact, retrieve it, and
/// confirm the model answers from the retrieved context with the source.
#[cfg(debug_assertions)]
fn selftest_rag(app: &tauri::AppHandle) -> Result<String> {
    use std::io::Write;
    // A fact the base model cannot know — proves retrieval, not memorization.
    let secret = "The Athanor calibration constant for the Meridian reactor is 8827 kelvin-seconds.";
    let dir = std::env::temp_dir().join("athanor-rag-selftest");
    std::fs::create_dir_all(&dir)?;
    let doc = dir.join("meridian-notes.txt");
    let mut f = std::fs::File::create(&doc)?;
    writeln!(
        f,
        "Meridian Reactor Field Notes\n\nGeneral background about the facility and its history.\n\n{secret}\n\nAdditional unrelated maintenance logs follow."
    )?;

    let model = downloads::ensure_installed(app, "llama-3.2-3b-instruct", "Q4_K_M")?;
    let ws_list = workspaces::list(app)?;
    let ws = match ws_list.workspaces.iter().find(|w| w.name == "RAG Test") {
        Some(w) => w.clone(),
        None => workspaces::create(app, "RAG Test", "Meridian reactor documentation", 200, "R", None)?,
    };
    workspaces::set_active_model(app, &ws.id, Some(model.sha256.clone()))?;

    let embedder = app.state::<Embedder>();
    let indexed = rag::add_document(app, &embedder, &ws.id, &doc.to_string_lossy())?;
    log::info!("SELFTEST RAG: indexed {} chunks", indexed.chunk_count);

    // Retrieval alone must surface the secret chunk.
    let (block, sources) = rag::retrieve(app, &embedder, &ws.id, "What is the calibration constant for the Meridian reactor?")?;
    if !block.contains("8827") {
        return Err(error::AthanorError::Rag(format!(
            "retrieval did not surface the fact; sources={sources:?}"
        )));
    }

    // End to end: the model answers from context.
    let llm = app.state::<Llm>();
    let ops = app.state::<Ops>();
    let conv_id = chat::send(
        app,
        &llm,
        &ops,
        &ws.id,
        None,
        "What is the calibration constant for the Meridian reactor? Answer with the number.".into(),
    )?;
    let conv = chat::load(app, &ws.id, &conv_id)?;
    let last = conv.messages.last().ok_or_else(|| error::AthanorError::Rag("no reply".into()))?;
    let answered = last.content.contains("8827");
    Ok(format!(
        "indexed={} retrieved_sources={} answer_contains_fact={} sources={:?} reply={:?}",
        indexed.chunk_count,
        sources.len(),
        answered,
        sources.iter().map(|s| (&s.doc_name, s.chunk_index, s.score)).collect::<Vec<_>>(),
        last.content.trim().chars().take(120).collect::<String>()
    ))
}

/// MCP self-test: launch the reference server-everything over stdio, connect,
/// list tools, and call `echo`.
#[cfg(debug_assertions)]
fn selftest_mcp(app: &tauri::AppHandle) -> Result<String> {
    let ws_list = workspaces::list(app)?;
    let ws = match ws_list.workspaces.iter().find(|w| w.name == "MCP Test") {
        Some(w) => w.clone(),
        None => workspaces::create(app, "MCP Test", "tool connectivity", 25, "M", None)?,
    };
    let cfg = mcp::McpServerConfig {
        id: "everything".into(),
        name: "Everything (reference)".into(),
        command: "npx".into(),
        args: vec!["-y".into(), "@modelcontextprotocol/server-everything".into()],
        env: Default::default(),
    };
    mcp::save_server(app, &ws.id, cfg)?;
    let mgr = app.state::<McpManager>();
    let view = mcp::connect(app, &mgr, &ws.id, "everything")?;
    let echo = mcp::call_tool(&mgr, "everything", "echo", serde_json::json!({ "message": "hello" }))?;
    mcp::disconnect(app, &mgr, "everything");
    Ok(format!(
        "server={:?} tools={} echo={:?}",
        view.server_name,
        view.tools.len(),
        echo.trim()
    ))
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

/// Agentic self-test: connect a tool server, then ask the model to add two
/// large numbers. Success requires the model to autonomously call the `add`
/// tool AND fold its result into the final answer — proving the full loop
/// (parse → execute → feed back → continue), not just that a tool is reachable.
#[cfg(debug_assertions)]
fn selftest_agentic(app: &tauri::AppHandle) -> Result<String> {
    let model = downloads::ensure_installed(app, "llama-3.2-3b-instruct", "Q4_K_M")?;
    log::info!("SELFTEST AGENTIC: model ready ({})", model.display_name);

    let ws_list = workspaces::list(app)?;
    let ws = match ws_list.workspaces.iter().find(|w| w.name == "Agentic Test") {
        Some(w) => w.clone(),
        None => workspaces::create(app, "Agentic Test", "autonomous tool use", 155, "A", None)?,
    };
    workspaces::set_active_model(app, &ws.id, Some(model.sha256.clone()))?;

    // Connect the reference server (provides `add`, which sums two numbers).
    let cfg = mcp::McpServerConfig {
        id: "everything".into(),
        name: "Everything (reference)".into(),
        command: "npx".into(),
        args: vec!["-y".into(), "@modelcontextprotocol/server-everything".into()],
        env: Default::default(),
    };
    mcp::save_server(app, &ws.id, cfg)?;
    let mgr = app.state::<McpManager>();
    let view = mcp::connect(app, &mgr, &ws.id, "everything")?;
    let tool_names: Vec<String> = view.tools.iter().map(|t| t.name.clone()).collect();
    log::info!("SELFTEST AGENTIC: {} tools connected: {:?}", view.tools.len(), tool_names);
    // This server build names its arithmetic tool `get-sum` (older builds used
    // `add`). Bind to whichever sum tool the connected server actually exposes.
    let sum_tool = ["get-sum", "add"]
        .into_iter()
        .find(|n| tool_names.iter().any(|t| t == n));
    let Some(sum_tool) = sum_tool else {
        mcp::disconnect(app, &mgr, "everything");
        return Err(error::AthanorError::Chat(format!(
            "reference server exposes no sum tool; got {tool_names:?}"
        )));
    };
    if let Some((_, t)) = mcp::available_tools(&mgr, &ws.id).iter().find(|(_, t)| t.name == sum_tool) {
        log::info!("SELFTEST AGENTIC: `{sum_tool}` schema = {}", t.input_schema);
    }

    // A + B chosen so the base model cannot reliably compute it unaided; the
    // only trustworthy path to the exact answer is the tool.
    let (a, b) = (40217_i64, 58991_i64);
    let expected = a + b; // 99208
    let llm = app.state::<Llm>();
    let ops = app.state::<Ops>();
    let conv_id = chat::send(
        app,
        &llm,
        &ops,
        &ws.id,
        None,
        format!(
            "You have a tool named `{sum_tool}` that returns the sum of two numbers. \
             Call it to add {a} and {b}, then reply with only the resulting number."
        ),
    )?;

    let conv = chat::load(app, &ws.id, &conv_id)?;
    mcp::disconnect(app, &mgr, "everything");

    let last = conv
        .messages
        .last()
        .ok_or_else(|| error::AthanorError::Chat("no reply saved".into()))?;
    log::info!(
        "SELFTEST AGENTIC: tool_steps = {:?}",
        last.tool_steps
            .iter()
            .map(|s| format!("{}({}) ok={} -> {}", s.tool, s.arguments, s.ok, s.result))
            .collect::<Vec<_>>()
    );
    let called_add = last.tool_steps.iter().any(|s| s.tool == sum_tool && s.ok);
    let answered = last.content.contains(&expected.to_string());

    if !called_add {
        return Err(error::AthanorError::Chat(format!(
            "model did not successfully call `{sum_tool}` (tool_steps={:?})",
            last.tool_steps.iter().map(|s| (&s.tool, s.ok, &s.result)).collect::<Vec<_>>()
        )));
    }
    if !answered {
        return Err(error::AthanorError::Chat(format!(
            "tool was called but answer omits {expected}; reply={:?}",
            last.content.trim().chars().take(120).collect::<String>()
        )));
    }
    Ok(format!(
        "tool_calls={} called_add={} answer_correct={} reply={:?}",
        last.tool_steps.len(),
        called_add,
        answered,
        last.content.trim().chars().take(120).collect::<String>()
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
        .manage(Embedder::default())
        .manage(McpManager::default())
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
                            "rag" => selftest_rag(&handle),
                            "mcp" => selftest_mcp(&handle),
                            "agentic" => selftest_agentic(&handle),
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
            rename_conversation,
            search_conversations,
            export_conversation,
            regenerate_reply,
            edit_and_resend,
            fork_conversation,
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
            get_coach_state,
            coach_mark_seen,
            coach_reset,
            get_templates,
            get_preferences,
            set_accent,
            get_data_root,
            reveal_data_root,
            is_portable,
            import_dataset,
            list_datasets,
            delete_dataset,
            get_trainer_status,
            run_benchmark,
            list_benchmarks,
            get_curated_prompts,
            list_user_prompts,
            save_user_prompt,
            delete_user_prompt,
            set_workspace_system_prompt,
            export_workspace_filename,
            export_workspace,
            import_workspace,
            rotate_api_key,
            check_for_update,
            list_operations,
            cancel_operation,
            dismiss_operation,
            retry_operation,
            get_knowledge_base,
            add_documents,
            cancel_indexing,
            remove_document,
            set_retrieval_enabled,
            preview_chunks,
            stop_embedder,
            list_mcp_servers,
            save_mcp_server,
            remove_mcp_server,
            connect_mcp_server,
            disconnect_mcp_server
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // No child — engine, embedder, or MCP server — outlives the app.
            // (The job object is the hard guarantee; this is the clean path.)
            if let tauri::RunEvent::Exit = event {
                let llm = app.state::<Llm>();
                if let Some(mut active) = llm.lock().take() {
                    log::info!(target: "rt", "app exit: stopping llama-server");
                    let _ = active.child.kill();
                    let _ = active.child.wait();
                }
                rag::embed::stop(app, &app.state::<Embedder>());
                mcp::shutdown_all(&app.state::<McpManager>());
            }
        });
}
