pub mod connect;
pub mod pipeline;

use connect::provider::ConnectProvider;
use pipeline::chunk::{
    chunk_document as chunk_pipeline_document, ChunkPipelineError, DeterministicDocumentChunker,
};
use pipeline::contracts::{
    ChunkedDocument, IngestedDocument, ModelRuntimeFailure, NormalizedDocument, ParsedDocument,
    PipelineRun, StructuredDocument,
};
use pipeline::db::init_db;
use pipeline::ingest::{ingest_pdf, IngestError};
use pipeline::model::OllamaRuntime;
use pipeline::normalize::{
    normalize_document as normalize_pipeline_document, CanonicalNormalizer, NormalizePipelineError,
};
use pipeline::parser::{
    parse_document as parse_pipeline_document, ParsePipelineError, PdfExtractParser,
};
use pipeline::recovery::reconcile_interrupted_runs;
use pipeline::service::{
    continuation_plan, continue_run_to_summary, process_pdf_to_summary,
    retry_failed_run_to_summary, ContinuationComponents, DocumentServiceError, SummaryComponents,
};
use pipeline::structure::{
    structure_document as structure_pipeline_document, DeterministicStructureInterpreter,
    StructurePipelineError,
};
use pipeline::workspace::{
    get_persisted_summary as load_persisted_summary, list_recent_runs as load_recent_runs,
    ollama_runtime_status, CompletedSummaryView, PersistedSummary, RunHistoryItem, RuntimeStatus,
    WorkspaceError,
};
use rusqlite::Connection;
use serde::Serialize;
use std::error::Error;
use std::sync::Mutex;
use tauri::{Manager, State};

struct AppState {
    db: Mutex<Connection>,
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

#[tauri::command]
fn ingest_document(
    state: State<'_, AppState>,
    file_path: String,
) -> Result<(IngestedDocument, PipelineRun), CommandError> {
    let mut conn = state.db.lock().map_err(|_| {
        CommandError::new(
            "DATABASE_LOCK_UNAVAILABLE",
            "The local database lock is unavailable",
        )
    })?;
    ingest_pdf(&mut conn, &file_path).map_err(CommandError::from)
}

#[tauri::command]
fn parse_document(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<ParsedDocument, CommandError> {
    let mut conn = state.db.lock().map_err(|_| {
        CommandError::new(
            "DATABASE_LOCK_UNAVAILABLE",
            "The local database lock is unavailable",
        )
    })?;
    parse_pipeline_document(&mut conn, &PdfExtractParser::new(), &run_id)
        .map_err(CommandError::from)
}

#[tauri::command]
fn normalize_document(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<NormalizedDocument, CommandError> {
    let mut conn = state.db.lock().map_err(|_| {
        CommandError::new(
            "DATABASE_LOCK_UNAVAILABLE",
            "The local database lock is unavailable",
        )
    })?;
    normalize_pipeline_document(&mut conn, &CanonicalNormalizer::new(), &run_id)
        .map_err(CommandError::from)
}

#[tauri::command]
fn structure_document(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<StructuredDocument, CommandError> {
    let mut conn = state.db.lock().map_err(|_| {
        CommandError::new(
            "DATABASE_LOCK_UNAVAILABLE",
            "The local database lock is unavailable",
        )
    })?;
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
    let mut conn = state.db.lock().map_err(|_| {
        CommandError::new(
            "DATABASE_LOCK_UNAVAILABLE",
            "The local database lock is unavailable",
        )
    })?;
    chunk_pipeline_document(&mut conn, &DeterministicDocumentChunker::new(), &run_id)
        .map_err(CommandError::from)
}

#[tauri::command]
fn summarize_document(
    state: State<'_, AppState>,
    file_path: String,
) -> Result<CompletedSummaryView, CommandError> {
    let runtime = OllamaRuntime::from_environment().map_err(CommandError::from)?;
    let mut conn = state.db.lock().map_err(|_| {
        CommandError::new(
            "DATABASE_LOCK_UNAVAILABLE",
            "The local database lock is unavailable",
        )
    })?;
    let completed = process_pdf_to_summary(
        &mut conn,
        &file_path,
        SummaryComponents {
            parser: &PdfExtractParser::new(),
            normalizer: &CanonicalNormalizer::new(),
            interpreter: &DeterministicStructureInterpreter::new(),
            chunker: &DeterministicDocumentChunker::new(),
            runtime: &runtime,
        },
    )
    .map_err(CommandError::from)?;
    let persisted = load_persisted_summary(&conn, &completed.run_id).map_err(CommandError::from)?;
    Ok(CompletedSummaryView::from(persisted))
}

#[tauri::command]
fn retry_document(
    state: State<'_, AppState>,
    run_id: String,
    expected_state_version: u32,
) -> Result<CompletedSummaryView, CommandError> {
    let runtime = OllamaRuntime::from_environment().map_err(CommandError::from)?;
    let mut conn = state.db.lock().map_err(|_| {
        CommandError::new(
            "DATABASE_LOCK_UNAVAILABLE",
            "The local database lock is unavailable",
        )
    })?;
    let completed = retry_failed_run_to_summary(
        &mut conn,
        &run_id,
        expected_state_version,
        SummaryComponents {
            parser: &PdfExtractParser::new(),
            normalizer: &CanonicalNormalizer::new(),
            interpreter: &DeterministicStructureInterpreter::new(),
            chunker: &DeterministicDocumentChunker::new(),
            runtime: &runtime,
        },
    )
    .map_err(CommandError::from)?;
    let persisted = load_persisted_summary(&conn, &completed.run_id).map_err(CommandError::from)?;
    Ok(CompletedSummaryView::from(persisted))
}

#[tauri::command]
fn continue_document(
    state: State<'_, AppState>,
    run_id: String,
    expected_state_version: u32,
) -> Result<CompletedSummaryView, CommandError> {
    let mut conn = state.db.lock().map_err(|_| {
        CommandError::new(
            "DATABASE_LOCK_UNAVAILABLE",
            "The local database lock is unavailable",
        )
    })?;
    let plan = continuation_plan(&conn, &run_id, expected_state_version)
        .map_err(DocumentServiceError::from)
        .map_err(CommandError::from)?;
    let runtime = if plan.requires_runtime {
        Some(OllamaRuntime::from_environment().map_err(CommandError::from)?)
    } else {
        None
    };
    let parser = PdfExtractParser::new();
    let normalizer = CanonicalNormalizer::new();
    let interpreter = DeterministicStructureInterpreter::new();
    let chunker = DeterministicDocumentChunker::new();
    let completed = continue_run_to_summary(
        &mut conn,
        &run_id,
        expected_state_version,
        ContinuationComponents {
            parser: &parser,
            normalizer: &normalizer,
            interpreter: &interpreter,
            chunker: &chunker,
            runtime: runtime
                .as_ref()
                .map(|runtime| runtime as &dyn pipeline::contracts::ModelRuntime),
        },
    )
    .map_err(CommandError::from)?;
    let persisted = load_persisted_summary(&conn, &completed.run_id).map_err(CommandError::from)?;
    Ok(CompletedSummaryView::from(persisted))
}

#[tauri::command]
fn get_runtime_status() -> RuntimeStatus {
    ollama_runtime_status()
}

#[tauri::command]
fn list_recent_runs(state: State<'_, AppState>) -> Result<Vec<RunHistoryItem>, CommandError> {
    let conn = state.db.lock().map_err(|_| {
        CommandError::new(
            "DATABASE_LOCK_UNAVAILABLE",
            "The local database lock is unavailable",
        )
    })?;
    load_recent_runs(&conn).map_err(CommandError::from)
}

#[tauri::command]
fn get_persisted_summary(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<PersistedSummary, CommandError> {
    let conn = state.db.lock().map_err(|_| {
        CommandError::new(
            "DATABASE_LOCK_UNAVAILABLE",
            "The local database lock is unavailable",
        )
    })?;
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

    builder
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&app_data_dir)?;
            let db_path = app_data_dir.join("summarizer.db");
            let mut conn = init_db(&db_path)?;
            let recovered = reconcile_interrupted_runs(&mut conn)?;
            if !recovered.is_empty() {
                eprintln!(
                    "Reconciled {} interrupted pipeline run(s) after restart",
                    recovered.len()
                );
            }

            app.manage(AppState {
                db: Mutex::new(conn),
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
            get_runtime_status,
            list_recent_runs,
            get_persisted_summary
        ])
        .run(tauri::generate_context!())?;
    Ok(())
}

#[cfg(test)]
mod capability_tests {
    use serde_json::Value;

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
}
