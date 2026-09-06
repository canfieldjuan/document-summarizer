pub mod connect;
mod desktop;
pub mod pipeline;

use connect::entitlement::{
    EntitlementDecision, EntitlementGate, EntitlementInstallError, EntitlementStatus,
};
use connect::provider::ConnectProvider;
use desktop::{BackgroundRunAccepted, DesktopJobError, DesktopJobManager};
use pipeline::chunk::{
    chunk_document as chunk_pipeline_document, ChunkPipelineError, DeterministicDocumentChunker,
};
use pipeline::contracts::{
    ChunkedDocument, IngestedDocument, ModelRuntimeFailure, NormalizedDocument, ParsedDocument,
    PipelineRun, StructuredDocument,
};
use pipeline::db::{init_db, StoreError};
use pipeline::ingest::{ingest_pdf, IngestError};
use pipeline::llama_cpp::{prune_idle_managed_runtimes, shutdown_managed_runtimes};
use pipeline::model_settings::{
    catalog as load_model_catalog, register_gguf, save_selected_preset,
    settings_path as model_settings_path, ModelCatalog,
};
use pipeline::normalize::{
    normalize_document as normalize_pipeline_document, CanonicalNormalizer, NormalizePipelineError,
};
use pipeline::parser::{
    parse_document as parse_pipeline_document, ParsePipelineError, PdfExtractParser,
};
use pipeline::recovery::reconcile_interrupted_runs;
use pipeline::service::DocumentServiceError;
use pipeline::structure::{
    structure_document as structure_pipeline_document, DeterministicStructureInterpreter,
    StructurePipelineError,
};
use pipeline::workspace::{
    get_persisted_summary as load_persisted_summary, get_run as load_run,
    list_recent_runs as load_recent_runs, ollama_runtime_status, PersistedSummary, RunHistoryItem,
    RuntimeStatus, WorkspaceError,
};
use rusqlite::Connection;
use serde::Serialize;
use std::error::Error;
use std::path::Path;
use tauri::{Manager, State};

#[cfg(unix)]
fn ensure_private_app_data_directory(path: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let before = std::fs::symlink_metadata(path)?;
    if !before.file_type().is_dir() || before.uid() != unsafe { libc::geteuid() } {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "application data directory is not owned by the effective user",
        ));
    }
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    let after = std::fs::symlink_metadata(path)?;
    if !after.file_type().is_dir()
        || after.uid() != before.uid()
        || after.dev() != before.dev()
        || after.ino() != before.ino()
        || after.mode() & 0o7777 != 0o700
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "application data directory identity or permissions are unsafe",
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_private_app_data_directory(_path: &Path) -> Result<(), std::io::Error> {
    Ok(())
}

struct AppState {
    jobs: DesktopJobManager,
    model_settings_path: std::path::PathBuf,
    entitlement: Option<EntitlementGate>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CommandError {
    code: String,
    message: String,
}

impl CommandError {
    fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl From<IngestError> for CommandError {
    fn from(error: IngestError) -> Self {
        Self::new(error.code(), error.to_string())
    }
}

impl From<ParsePipelineError> for CommandError {
    fn from(error: ParsePipelineError) -> Self {
        let code = error.code().to_string();
        Self::new(code, error.to_string())
    }
}

impl From<NormalizePipelineError> for CommandError {
    fn from(error: NormalizePipelineError) -> Self {
        let code = error.code().to_string();
        Self::new(code, error.to_string())
    }
}

impl From<StructurePipelineError> for CommandError {
    fn from(error: StructurePipelineError) -> Self {
        let code = error.code().to_string();
        Self::new(code, error.to_string())
    }
}

impl From<ChunkPipelineError> for CommandError {
    fn from(error: ChunkPipelineError) -> Self {
        let code = error.code().to_string();
        Self::new(code, error.to_string())
    }
}

impl From<DocumentServiceError> for CommandError {
    fn from(error: DocumentServiceError) -> Self {
        let code = error.code().to_string();
        Self::new(code, error.to_string())
    }
}

impl From<ModelRuntimeFailure> for CommandError {
    fn from(error: ModelRuntimeFailure) -> Self {
        Self::new(error.code, error.message)
    }
}

impl From<WorkspaceError> for CommandError {
    fn from(error: WorkspaceError) -> Self {
        Self::new(error.code(), error.to_string())
    }
}

impl From<StoreError> for CommandError {
    fn from(error: StoreError) -> Self {
        Self::new("PIPELINE_STORE_ERROR", error.to_string())
    }
}

impl From<DesktopJobError> for CommandError {
    fn from(error: DesktopJobError) -> Self {
        let code = error.code().to_string();
        Self::new(code, error.to_string())
    }
}

impl From<EntitlementInstallError> for CommandError {
    fn from(error: EntitlementInstallError) -> Self {
        Self::new(error.code(), error.to_string())
    }
}

fn open_database(state: &AppState) -> Result<Connection, CommandError> {
    init_db(state.jobs.db_path()).map_err(CommandError::from)
}

#[tauri::command]
fn ingest_document(
    state: State<'_, AppState>,
    file_path: String,
) -> Result<(IngestedDocument, PipelineRun), CommandError> {
    let mut conn = open_database(&state)?;
    ingest_pdf(&mut conn, &file_path).map_err(CommandError::from)
}

#[tauri::command]
fn parse_document(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<ParsedDocument, CommandError> {
    let mut conn = open_database(&state)?;
    parse_pipeline_document(&mut conn, &PdfExtractParser::new(), &run_id)
        .map_err(CommandError::from)
}

#[tauri::command]
fn normalize_document(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<NormalizedDocument, CommandError> {
    let mut conn = open_database(&state)?;
    normalize_pipeline_document(&mut conn, &CanonicalNormalizer::new(), &run_id)
        .map_err(CommandError::from)
}

#[tauri::command]
fn structure_document(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<StructuredDocument, CommandError> {
    let mut conn = open_database(&state)?;
    structure_pipeline_document(
        &mut conn,
        &DeterministicStructureInterpreter::new(),
        &run_id,
    )
    .map_err(CommandError::from)
}

#[tauri::command]
fn chunk_document(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<ChunkedDocument, CommandError> {
    let mut conn = open_database(&state)?;
    chunk_pipeline_document(&mut conn, &DeterministicDocumentChunker::new(), &run_id)
        .map_err(CommandError::from)
}

#[tauri::command]
fn summarize_document(
    state: State<'_, AppState>,
    file_path: String,
) -> Result<BackgroundRunAccepted, CommandError> {
    state.jobs.start_pdf(&file_path).map_err(CommandError::from)
}

#[tauri::command]
fn retry_document(
    state: State<'_, AppState>,
    run_id: String,
    expected_state_version: u32,
) -> Result<BackgroundRunAccepted, CommandError> {
    state
        .jobs
        .start_retry(&run_id, expected_state_version)
        .map_err(CommandError::from)
}

#[tauri::command]
fn continue_document(
    state: State<'_, AppState>,
    run_id: String,
    expected_state_version: u32,
) -> Result<BackgroundRunAccepted, CommandError> {
    state
        .jobs
        .start_continuation(&run_id, expected_state_version)
        .map_err(CommandError::from)
}

#[tauri::command]
fn cancel_document(
    state: State<'_, AppState>,
    run_id: String,
    expected_state_version: u32,
) -> Result<RunHistoryItem, CommandError> {
    state
        .jobs
        .request_cancellation(&run_id, expected_state_version)?;
    get_run_status(state, run_id)
}

#[tauri::command]
async fn get_runtime_status(state: State<'_, AppState>) -> Result<RuntimeStatus, CommandError> {
    let settings_path = state.model_settings_path.clone();
    tauri::async_runtime::spawn_blocking(move || ollama_runtime_status(&settings_path))
        .await
        .map_err(|_| CommandError::new("MODEL_RUNTIME_STATUS_FAILED", "Runtime check stopped"))
}

#[tauri::command]
async fn get_model_catalog(state: State<'_, AppState>) -> Result<ModelCatalog, CommandError> {
    let settings_path = state.model_settings_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        load_model_catalog(&settings_path).map_err(CommandError::from)
    })
    .await
    .map_err(|_| CommandError::new("MODEL_CATALOG_FAILED", "Model discovery stopped"))?
}

#[tauri::command]
async fn select_model_preset(
    state: State<'_, AppState>,
    preset_id: String,
) -> Result<ModelCatalog, CommandError> {
    let settings_path = state.model_settings_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let current = load_model_catalog(&settings_path).map_err(CommandError::from)?;
        if !current
            .presets
            .iter()
            .any(|preset| preset.preset_id == preset_id)
        {
            return Err(CommandError::new(
                "MODEL_PRESET_UNAVAILABLE",
                "Selected model preset is not installed and qualified",
            ));
        }
        if current.selected_preset_id == preset_id {
            return Ok(current);
        }
        prune_idle_managed_runtimes().map_err(CommandError::from)?;
        save_selected_preset(&settings_path, &preset_id).map_err(CommandError::from)?;
        load_model_catalog(&settings_path).map_err(CommandError::from)
    })
    .await
    .map_err(|_| CommandError::new("MODEL_SELECTION_FAILED", "Model selection stopped"))?
}

#[tauri::command]
async fn register_gguf_model(
    state: State<'_, AppState>,
    file_path: String,
) -> Result<ModelCatalog, CommandError> {
    let settings_path = state.model_settings_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        register_gguf(&settings_path, Path::new(&file_path)).map_err(CommandError::from)?;
        load_model_catalog(&settings_path).map_err(CommandError::from)
    })
    .await
    .map_err(|_| CommandError::new("MODEL_REGISTRATION_FAILED", "GGUF registration stopped"))?
}

#[tauri::command]
fn get_connect_entitlement_status(state: State<'_, AppState>) -> EntitlementStatus {
    state.entitlement.as_ref().map_or_else(
        || EntitlementDecision::AuthorityUnavailable.into(),
        EntitlementGate::status,
    )
}

#[tauri::command]
fn install_connect_entitlement(
    state: State<'_, AppState>,
    source_path: String,
) -> Result<EntitlementStatus, CommandError> {
    let gate = state
        .entitlement
        .as_ref()
        .ok_or_else(|| CommandError::from(EntitlementInstallError::AuthorityUnavailable))?;
    gate.install(Path::new(&source_path))
        .map_err(CommandError::from)
}

#[tauri::command]
fn list_recent_runs(state: State<'_, AppState>) -> Result<Vec<RunHistoryItem>, CommandError> {
    let conn = open_database(&state)?;
    let mut runs = load_recent_runs(&conn).map_err(CommandError::from)?;
    for run in &mut runs {
        let active = state
            .jobs
            .is_active(&run.run_id)
            .map_err(CommandError::from)?;
        run.background_active = active;
        run.can_cancel = run.state.can_request_cancellation() && active;
    }
    Ok(runs)
}

#[tauri::command]
fn get_run_status(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<RunHistoryItem, CommandError> {
    let active = state.jobs.is_active(&run_id).map_err(CommandError::from)?;
    let conn = open_database(&state)?;
    let mut run = load_run(&conn, &run_id).map_err(CommandError::from)?;
    run.background_active = active;
    run.can_cancel = run.state.can_request_cancellation() && active;
    Ok(run)
}

#[tauri::command]
fn get_persisted_summary(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<PersistedSummary, CommandError> {
    let conn = open_database(&state)?;
    load_persisted_summary(&conn, &run_id).map_err(CommandError::from)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> Result<(), Box<dyn Error>> {
    let builder = tauri::Builder::default();
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
    }));

    let app = builder
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&app_data_dir)?;
            ensure_private_app_data_directory(&app_data_dir)?;
            let db_path = app_data_dir.join("summarizer.db");
            let settings_path = model_settings_path(&app_data_dir);
            let mut conn = init_db(&db_path)?;
            let recovered = reconcile_interrupted_runs(&mut conn)?;
            if !recovered.is_empty() {
                eprintln!(
                    "Reconciled {} interrupted pipeline run(s) after restart",
                    recovered.len()
                );
            }
            drop(conn);

            let entitlement = match EntitlementGate::from_installation() {
                Ok(gate) => Some(gate),
                Err(error) => {
                    eprintln!("Connect entitlement authority unavailable: {error}");
                    None
                }
            };
            app.manage(AppState {
                jobs: DesktopJobManager::new(db_path.clone(), settings_path.clone()),
                model_settings_path: settings_path,
                entitlement,
            });
            match ConnectProvider::start(db_path, app_data_dir) {
                Ok(provider) => {
                    app.manage(provider);
                }
                Err(error) => {
                    eprintln!("Connect provider unavailable; standalone mode continues: {error}");
                }
            }
            Ok(())
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            ingest_document,
            parse_document,
            normalize_document,
            structure_document,
            chunk_document,
            summarize_document,
            retry_document,
            continue_document,
            cancel_document,
            get_runtime_status,
            get_model_catalog,
            select_model_preset,
            register_gguf_model,
            get_connect_entitlement_status,
            install_connect_entitlement,
            list_recent_runs,
            get_run_status,
            get_persisted_summary
        ])
        .build(tauri::generate_context!())?;
    app.run(|app_handle, event| {
        if matches!(event, tauri::RunEvent::Exit) {
            shutdown_managed_runtimes();
            if let Some(provider) = app_handle.try_state::<ConnectProvider>() {
                provider.unregister();
            }
        }
    });
    Ok(())
}

#[cfg(test)]
mod capability_tests {
    use serde_json::Value;

    #[cfg(unix)]
    #[test]
    fn app_data_privacy_accepts_owned_directory_and_rejects_symlink_or_file() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        super::ensure_private_app_data_directory(directory.path()).unwrap();
        assert_eq!(
            std::fs::symlink_metadata(directory.path())
                .unwrap()
                .permissions()
                .mode()
                & 0o7777,
            0o700
        );

        let parent = tempfile::tempdir().unwrap();
        let target = parent.path().join("target");
        std::fs::create_dir(&target).unwrap();
        let symlink = parent.path().join("app-data-link");
        std::os::unix::fs::symlink(&target, &symlink).unwrap();
        assert_eq!(
            super::ensure_private_app_data_directory(&symlink)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::PermissionDenied
        );

        let file = parent.path().join("app-data-file");
        std::fs::write(&file, b"not a directory").unwrap();
        assert_eq!(
            super::ensure_private_app_data_directory(&file)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::PermissionDenied
        );
    }

    #[test]
    fn main_window_can_open_files_without_broader_dialog_permissions() {
        let capability: Value = serde_json::from_str(include_str!("../capabilities/default.json"))
            .expect("main-window capability should be valid JSON");
        let permissions = capability["permissions"]
            .as_array()
            .expect("main-window permissions should be an array");
        let has_permission = |expected: &str| {
            permissions
                .iter()
                .any(|permission| permission.as_str() == Some(expected))
        };

        assert!(has_permission("dialog:allow-open"));
        assert!(!has_permission("dialog:default"));
        assert!(!has_permission("dialog:allow-save"));
        assert!(!has_permission("dialog:allow-message"));
    }

    #[test]
    fn tauri_cli_controls_protocol_mode_through_the_default_feature() {
        let config: Value = serde_json::from_str(include_str!("../tauri.conf.json"))
            .expect("Tauri configuration should be valid JSON");
        let manifest = include_str!("../Cargo.toml");
        let build = config["build"]
            .as_object()
            .expect("Tauri build configuration should be an object");

        assert_eq!(
            build.get("devUrl").and_then(Value::as_str),
            Some("http://localhost:1420")
        );
        assert_eq!(
            build.get("frontendDist").and_then(Value::as_str),
            Some("../dist")
        );
        assert!(build
            .get("features")
            .and_then(Value::as_array)
            .is_none_or(|features| features
                .iter()
                .all(|feature| feature.as_str() != Some("custom-protocol"))));
        assert!(manifest
            .lines()
            .any(|line| line.trim() == r#"default = ["custom-protocol"]"#));
        assert!(manifest
            .lines()
            .any(|line| line.trim() == r#"custom-protocol = ["tauri/custom-protocol"]"#));
    }
}
