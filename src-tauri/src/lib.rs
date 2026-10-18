#[cfg(test)]
#[path = "../tests/support/store.rs"]
mod test_store;

pub mod connect;
mod desktop;
mod desktop_startup;
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
    ChunkedDocument, DocumentNormalizer, DocumentParser, IngestedDocument, ModelRuntime,
    ModelRuntimeFailure, NormalizedDocument, ParsedDocument, PipelineFailure, PipelineRun,
    StructureInterpreter, StructuredDocument, SummaryProfile,
};
use pipeline::corrections::{self, CorrectionError, OcrReview, SaveCorrection};
use pipeline::db::{get_or_create_profile_suggestion_owner, StoreError};
use pipeline::ingest::{ingest_pdf, prepare_pdf_ingestion, IngestError};
use pipeline::llama_cpp::{prune_idle_managed_runtimes, shutdown_managed_runtimes};
use pipeline::model_settings::{
    catalog as load_model_catalog, configure_gateway, register_gguf, runtime_from_settings,
    save_selected_preset, select_gateway, settings_path as model_settings_path, InferenceSource,
    ModelCatalog,
};
use pipeline::normalize::{
    normalize_document as normalize_pipeline_document, CanonicalNormalizer, NormalizePipelineError,
};
use pipeline::parser::{
    parse_document as parse_pipeline_document, ParsePipelineError, PdfExtractParser,
    SourceParserSet,
};
use pipeline::profile_suggestion::{
    suggest_summary_profile as suggest_profile_from_document, SummaryProfileSuggestion,
    PROFILE_SUGGESTION_TASK_CONTRACT_VERSION,
};
use pipeline::service::DocumentServiceError;
use pipeline::structure::{
    structure_document as structure_pipeline_document, DeterministicStructureInterpreter,
    StructurePipelineError,
};
use pipeline::workspace::{
    get_persisted_summary as load_persisted_summary, get_run as load_run,
    list_recent_runs as load_recent_runs, PersistedSummary, RunHistoryItem, RuntimeStatus,
    WorkspaceError,
};
use rusqlite::Connection;
use serde::Serialize;
use std::error::Error;
use std::path::Path;
use std::sync::Mutex;
use tauri::{Manager, State};

type ManagedConnectProvider = Mutex<Option<ConnectProvider>>;

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

    // Preserve the availability owner's signal in every IPC error projection.
    // Other storage errors retain their operation-specific categories.
    fn store_state_code(error: &StoreError) -> Option<&'static str> {
        if pipeline::db::is_contention(error) {
            Some("PROVIDER_STORE_BUSY")
        } else if matches!(error, StoreError::Unavailable) {
            Some("DOCUMENT_STORE_UNAVAILABLE")
        } else {
            None
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

impl From<PipelineFailure> for CommandError {
    fn from(error: PipelineFailure) -> Self {
        Self::new(error.code, error.message)
    }
}

impl From<WorkspaceError> for CommandError {
    fn from(error: WorkspaceError) -> Self {
        Self::new(error.code(), error.to_string())
    }
}

impl From<CorrectionError> for CommandError {
    fn from(error: CorrectionError) -> Self {
        Self::new(error.code(), error.to_string())
    }
}

impl From<StoreError> for CommandError {
    fn from(error: StoreError) -> Self {
        Self::new(
            Self::store_state_code(&error).unwrap_or("PIPELINE_STORE_ERROR"),
            error.to_string(),
        )
    }
}

impl From<DesktopJobError> for CommandError {
    fn from(error: DesktopJobError) -> Self {
        let code = match &error {
            DesktopJobError::Store(store) => {
                Self::store_state_code(store).unwrap_or_else(|| error.code())
            }
            _ => error.code(),
        }
        .to_string();
        Self::new(code, error.to_string())
    }
}

impl From<EntitlementInstallError> for CommandError {
    fn from(error: EntitlementInstallError) -> Self {
        Self::new(error.code(), error.to_string())
    }
}

fn open_database(state: &AppState) -> Result<Connection, CommandError> {
    state.jobs.store()?.writer().map_err(CommandError::from)
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
    let run = pipeline::db::get_pipeline_run(&conn, &run_id)
        .map_err(CommandError::from)?
        .ok_or_else(|| CommandError::from(StoreError::RunNotFound(run_id.clone())))?;
    let document = pipeline::db::get_document(&conn, &run.document_id)
        .map_err(CommandError::from)?
        .ok_or_else(|| CommandError::from(StoreError::DocumentNotFound(run.document_id)))?;
    let parsers = SourceParserSet::new();
    parse_pipeline_document(&mut conn, parsers.select(document.source_type), &run_id)
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

fn bind_profile_suggestion_request_owner(
    runtime: &mut dyn ModelRuntime,
    store: &pipeline::db::Store,
    source_content_hash: &str,
) -> Result<String, CommandError> {
    let contract = runtime.request_owner_contract(PROFILE_SUGGESTION_TASK_CONTRACT_VERSION);
    let mut conn = store.writer().map_err(CommandError::from)?;
    let owner_id =
        get_or_create_profile_suggestion_owner(&mut conn, source_content_hash, &contract)
            .map_err(CommandError::from)?;
    runtime.bind_request_owner(&owner_id);
    Ok(owner_id)
}

#[tauri::command]
async fn suggest_summary_profile(
    state: State<'_, AppState>,
    file_path: String,
) -> Result<SummaryProfileSuggestion, CommandError> {
    let settings_path = state.model_settings_path.clone();
    let store = state.jobs.store()?;
    let db_path = store.path().to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        let (document, _) = prepare_pdf_ingestion(&file_path, None).map_err(CommandError::from)?;
        let parsed = PdfExtractParser::new()
            .parse(&document)
            .map_err(CommandError::from)?;
        let normalized = CanonicalNormalizer::new()
            .normalize(&parsed)
            .map_err(CommandError::from)?;
        let structured = DeterministicStructureInterpreter::new()
            .interpret(&normalized)
            .map_err(CommandError::from)?;
        let mut runtime =
            runtime_from_settings(&settings_path, &db_path).map_err(CommandError::from)?;
        bind_profile_suggestion_request_owner(runtime.as_mut(), &store, &document.content_hash)?;
        runtime.health().map_err(CommandError::from)?;
        suggest_profile_from_document(
            runtime.as_ref(),
            &normalized,
            &structured,
            &document.content_hash,
        )
        .map_err(CommandError::from)
    })
    .await
    .map_err(|_| {
        CommandError::new(
            "PROFILE_SUGGESTION_STOPPED",
            "Automatic profile suggestion stopped unexpectedly",
        )
    })?
}

#[tauri::command]
fn summarize_document(
    state: State<'_, AppState>,
    file_path: String,
    summary_profile: SummaryProfile,
    expected_content_hash: Option<String>,
) -> Result<BackgroundRunAccepted, CommandError> {
    state
        .jobs
        .start_pdf(
            &file_path,
            summary_profile,
            expected_content_hash.as_deref(),
        )
        .map_err(CommandError::from)
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
    let jobs = state.jobs.clone();
    tauri::async_runtime::spawn_blocking(move || {
        desktop_startup::runtime_status(&jobs, &settings_path)
    })
    .await
    .map_err(|_| CommandError::new("MODEL_RUNTIME_STATUS_FAILED", "Runtime check stopped"))
}

#[tauri::command]
async fn configure_inference_gateway(
    state: State<'_, AppState>,
    base_url: String,
    token_file: String,
    ca_file: String,
) -> Result<ModelCatalog, CommandError> {
    let settings_path = state.model_settings_path.clone();
    let db_path = state.jobs.db_path().to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        configure_gateway(
            &settings_path,
            &db_path,
            &base_url,
            Path::new(&token_file),
            Path::new(&ca_file),
        )
        .map_err(CommandError::from)?;
        load_model_catalog(&settings_path).map_err(CommandError::from)
    })
    .await
    .map_err(|_| {
        CommandError::new(
            "MODEL_GATEWAY_CONFIGURATION_FAILED",
            "Gateway configuration stopped",
        )
    })?
}

#[tauri::command]
async fn select_inference_gateway(
    state: State<'_, AppState>,
) -> Result<ModelCatalog, CommandError> {
    let settings_path = state.model_settings_path.clone();
    let db_path = state.jobs.db_path().to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        select_gateway(&settings_path, &db_path).map_err(CommandError::from)?;
        load_model_catalog(&settings_path).map_err(CommandError::from)
    })
    .await
    .map_err(|_| {
        CommandError::new(
            "MODEL_GATEWAY_SELECTION_FAILED",
            "Gateway selection stopped",
        )
    })?
}

#[tauri::command]
async fn retry_document_store(state: State<'_, AppState>) -> Result<(), CommandError> {
    let jobs = state.jobs.clone();
    let app_data = jobs
        .db_path()
        .parent()
        .ok_or_else(|| {
            CommandError::new(
                "DOCUMENT_STORE_UNAVAILABLE",
                "Document storage path is unavailable",
            )
        })?
        .to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        jobs.retry_store(app_data).map_err(CommandError::from)
    })
    .await
    .map_err(|_| CommandError::new("DOCUMENT_STORE_UNAVAILABLE", "Storage retry stopped"))?
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
        if direct_preset_selection_is_noop(
            current.selected_source,
            &current.selected_preset_id,
            &preset_id,
        ) {
            return Ok(current);
        }
        prune_idle_managed_runtimes().map_err(CommandError::from)?;
        save_selected_preset(&settings_path, &preset_id).map_err(CommandError::from)?;
        load_model_catalog(&settings_path).map_err(CommandError::from)
    })
    .await
    .map_err(|_| CommandError::new("MODEL_SELECTION_FAILED", "Model selection stopped"))?
}

fn direct_preset_selection_is_noop(
    selected_source: InferenceSource,
    selected_preset_id: &str,
    requested_preset_id: &str,
) -> bool {
    selected_source == InferenceSource::Direct && selected_preset_id == requested_preset_id
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
    let conn = open_database(&state)?;
    let mut run = load_run(&conn, &run_id).map_err(CommandError::from)?;
    let (background_active, projected_active) = state
        .jobs
        .status_activity(&run_id, &run.run_id)
        .map_err(CommandError::from)?;
    run.background_active = background_active;
    run.can_cancel = run.state.can_request_cancellation() && projected_active;
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

#[tauri::command]
fn get_ocr_review(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<Option<OcrReview>, CommandError> {
    let conn = open_database(&state)?;
    let review = corrections::get_review(&conn, &run_id)?;
    if let Some(review) = &review {
        if state.jobs.status_activity(&run_id, &review.run_id)?.0 {
            return Ok(None);
        }
    }
    Ok(review)
}

#[tauri::command]
fn save_ocr_correction(
    state: State<'_, AppState>,
    request: SaveCorrection,
) -> Result<RunHistoryItem, CommandError> {
    if state.jobs.is_active(&request.run_id)? {
        return Err(CorrectionError::NotAllowed.into());
    }
    let mut conn = open_database(&state)?;
    let run = corrections::save(&mut conn, &request)?;
    load_run(&conn, &run.run_id).map_err(CommandError::from)
}

#[tauri::command]
fn open_ocr_source(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    run_id: String,
) -> Result<(), CommandError> {
    use tauri_plugin_opener::OpenerExt;
    let conn = open_database(&state)?;
    let path = corrections::verified_source_path(&conn, &run_id)?;
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|_| CorrectionError::SourceUnavailable.into())
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
            let db_path = app_data_dir.join("summarizer.db");
            let settings_path = model_settings_path(&app_data_dir);

            let entitlement = match EntitlementGate::from_installation() {
                Ok(gate) => Some(gate),
                Err(error) => {
                    eprintln!("Connect entitlement authority unavailable: {error}");
                    None
                }
            };
            let retry_db_path = db_path.clone();
            let retry_app_data = app_data_dir.clone();
            let services = desktop_startup::prepare(
                &app_data_dir,
                settings_path.clone(),
                |path| ConnectProvider::start(path.to_path_buf(), app_data_dir.clone()),
                std::sync::Arc::new(move || {
                    connect::provider::prepare_standalone_store(&retry_db_path, &retry_app_data)
                }),
            )?;
            let desktop_startup::Services { provider, jobs } = services;
            app.manage(AppState {
                jobs,
                model_settings_path: settings_path,
                entitlement,
            });
            app.manage(Mutex::new(provider));
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
            suggest_summary_profile,
            summarize_document,
            retry_document,
            continue_document,
            cancel_document,
            get_runtime_status,
            retry_document_store,
            get_model_catalog,
            configure_inference_gateway,
            select_inference_gateway,
            select_model_preset,
            register_gguf_model,
            get_connect_entitlement_status,
            install_connect_entitlement,
            list_recent_runs,
            get_run_status,
            get_persisted_summary,
            get_ocr_review,
            save_ocr_correction,
            open_ocr_source
        ])
        .build(tauri::generate_context!())?;
    app.run(|app_handle, event| {
        if matches!(event, tauri::RunEvent::Exit) {
            if let Some(provider) = app_handle.try_state::<ManagedConnectProvider>() {
                let provider = provider
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .take();
                drop(provider);
            }
            shutdown_managed_runtimes();
        }
    });
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn run_background_connect_provider() -> Result<(), Box<dyn Error>> {
    connect::lifecycle::run().map_err(Into::into)
}

#[cfg(target_os = "linux")]
pub fn run_background_control(command: &str) -> Result<&'static str, Box<dyn Error>> {
    use connect::lifecycle_control::{run_control, ControlAction, ControlStatus};
    let action = match command {
        "enable" => ControlAction::Enable,
        "disable" => ControlAction::Disable,
        "recover" => ControlAction::Recover,
        "status" => ControlAction::Status,
        _ => return Err("unknown Connect background control command".into()),
    };
    let status = run_control(action)?;
    Ok(match status {
        ControlStatus::Enabled => "enabled",
        ControlStatus::Disabled => "disabled",
        ControlStatus::TransitionPending => "transition-pending",
    })
}

#[cfg(target_os = "linux")]
pub fn run_package_control(arguments: &[String]) -> Result<(), Box<dyn Error>> {
    use connect::package_control::{self, PackageAction};

    let action = match arguments {
        [command] if command == "initialize" => PackageAction::Initialize,
        [command, source, target] if command == "prepare-upgrade" => {
            PackageAction::PrepareUpgrade { source, target }
        }
        [command, target] if command == "finish-upgrade" => PackageAction::FinishUpgrade { target },
        [command, target] if command == "prepare-remove" => PackageAction::PrepareRemove { target },
        [command, target] if command == "abort-remove" => PackageAction::AbortRemove { target },
        [command, target] if command == "finish-remove" => PackageAction::FinishRemove { target },
        [command, target] if command == "finish-purge" => PackageAction::FinishPurge { target },
        [command, target] if command == "recover-install" => {
            PackageAction::RecoverInstall { target }
        }
        [command, target] if command == "prepare-reinstall" => {
            PackageAction::PrepareReinstall { target }
        }
        [command] if command == "adopt-bootstrap" => PackageAction::AdoptBootstrap,
        _ => return Err("invalid package lifecycle control command".into()),
    };
    package_control::run(action).map_err(Into::into)
}

#[cfg(target_os = "linux")]
pub fn run_package_user_stop(app_data: &str, runtime_root: &str) -> Result<(), Box<dyn Error>> {
    connect::provider::stop_and_cleanup_registered_provider(
        std::path::Path::new(app_data),
        std::path::Path::new(runtime_root),
        std::time::Instant::now() + std::time::Duration::from_secs(45),
    )
    .map_err(Into::into)
}

#[cfg(test)]
mod storage_error_tests {
    use super::*;
    use crate::pipeline::db;
    use std::sync::Arc;

    #[test]
    fn departed_primary_reaches_both_command_projections_as_unavailable() {
        let file = crate::test_store::DatabaseFixture::new();
        let primary = db::Store::initialize(&file.0).unwrap();
        let access = db::StoreAccess::prepared(db::Store::attach(&file.0).unwrap());
        assert!(access.get().is_ok());
        drop(primary);
        let direct = CommandError::from(access.get().unwrap_err());
        let background = CommandError::from(DesktopJobError::from(access.get().unwrap_err()));
        assert_eq!(direct.code, "DOCUMENT_STORE_UNAVAILABLE");
        assert_eq!(background.code, direct.code);
        let jobs = DesktopJobManager::with_store_access(access, file.0.with_extension("settings"));
        let status = desktop_startup::runtime_status(&jobs, &file.0.with_extension("settings"));
        assert_eq!(status.code.as_deref(), Some(direct.code.as_str()));
        assert!(!status.ready);
        assert!(status.recoverable);
    }

    #[test]
    fn primary_exit_between_cached_get_and_connection_open_keeps_unavailable_signal() {
        let file = crate::test_store::DatabaseFixture::new();
        let primary = db::Store::initialize(&file.0).unwrap();
        let access = db::StoreAccess::prepared(db::Store::attach(&file.0).unwrap());
        let command_store = access.get().unwrap();
        drop(primary);
        // The command already passed the cache check. Exercise the actual
        // connection owner, not another call to StoreAccess::get.
        let direct = CommandError::from(command_store.writer().unwrap_err());
        let background =
            CommandError::from(DesktopJobError::from(command_store.writer().unwrap_err()));
        assert_eq!(direct.code, "DOCUMENT_STORE_UNAVAILABLE");
        assert_eq!(background.code, direct.code);
    }

    #[test]
    fn busy_command_projections_preserve_the_cache_and_other_error_categories() {
        let file = crate::test_store::DatabaseFixture::new();
        let primary = db::Store::initialize(&file.0).unwrap();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = Arc::clone(&calls);
        let path = file.0.clone();
        let access = db::StoreAccess::new(
            path.clone(),
            Some(db::Store::attach(&path).unwrap()),
            Arc::new(move || {
                count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                db::Store::attach_wait(&path)
            }),
        );
        let mut name = file.0.as_os_str().to_os_string();
        name.push(".initialize.lock");
        let gate = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(std::path::PathBuf::from(name))
            .unwrap();
        gate.try_lock().unwrap();
        let direct = CommandError::from(access.get().unwrap_err());
        let background = CommandError::from(DesktopJobError::from(access.get().unwrap_err()));
        assert_eq!(direct.code, "PROVIDER_STORE_BUSY");
        assert_eq!(background.code, direct.code);
        let jobs =
            DesktopJobManager::with_store_access(access.clone(), file.0.with_extension("settings"));
        let status = desktop_startup::runtime_status(&jobs, &file.0.with_extension("settings"));
        assert_eq!(status.code.as_deref(), Some(direct.code.as_str()));
        drop(gate);
        assert!(access.retry().unwrap().reader().is_ok());
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(
            CommandError::from(StoreError::RunNotFound("public-run".into())).code,
            "PIPELINE_STORE_ERROR"
        );
        assert_eq!(
            CommandError::from(DesktopJobError::from(StoreError::RunNotFound(
                "public-run".into()
            )))
            .code,
            "BACKGROUND_RUN_NOT_FOUND"
        );
        assert!(primary.reader().is_ok());
    }
}

#[cfg(test)]
mod capability_tests {
    use crate::pipeline::contracts::{
        ModelRequest, ModelResponse, ModelRuntime, ModelRuntimeFailure,
    };
    use serde_json::Value;

    #[test]
    fn retained_direct_preset_is_not_a_noop_while_gateway_is_selected() {
        assert!(!super::direct_preset_selection_is_noop(
            crate::pipeline::model_settings::InferenceSource::Gateway,
            "retained-direct",
            "retained-direct",
        ));
        assert!(super::direct_preset_selection_is_noop(
            crate::pipeline::model_settings::InferenceSource::Direct,
            "retained-direct",
            "retained-direct",
        ));
    }

    #[derive(Default)]
    struct OwnerRecordingRuntime {
        owner_id: Option<String>,
    }

    impl ModelRuntime for OwnerRecordingRuntime {
        fn bind_request_owner(&mut self, owner_id: &str) {
            self.owner_id = Some(owner_id.to_string());
        }

        fn generate(&self, _request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
            unreachable!("owner-binding proof does not generate")
        }

        fn health(&self) -> Result<(), ModelRuntimeFailure> {
            Ok(())
        }

        fn runtime_id(&self) -> &str {
            "owner-recording-runtime"
        }

        fn model_id(&self) -> &str {
            "owner-recording-model"
        }
    }

    #[test]
    fn automatic_profile_owner_is_stable_and_bound_before_inference() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("summarizer.db");
        let store = super::pipeline::db::Store::initialize(&database).unwrap();
        let hash = "a".repeat(64);
        let mut first = OwnerRecordingRuntime::default();
        let first_owner =
            super::bind_profile_suggestion_request_owner(&mut first, &store, &hash).unwrap();
        assert_eq!(first.owner_id.as_deref(), Some(first_owner.as_str()));

        let mut reopened = OwnerRecordingRuntime::default();
        let reopened_owner =
            super::bind_profile_suggestion_request_owner(&mut reopened, &store, &hash).unwrap();
        assert_eq!(reopened_owner, first_owner);
        assert_eq!(reopened.owner_id.as_deref(), Some(first_owner.as_str()));
    }

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
