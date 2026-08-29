pub mod pipeline;

use pipeline::contracts::{
    IngestedDocument, NormalizedDocument, ParsedDocument, PipelineRun, StructuredDocument,
};
use pipeline::db::init_db;
use pipeline::ingest::{ingest_pdf, IngestError};
use pipeline::normalize::{
    normalize_document as normalize_pipeline_document, CanonicalNormalizer, NormalizePipelineError,
};
use pipeline::parser::{
    parse_document as parse_pipeline_document, ParsePipelineError, PdfExtractParser,
};
use pipeline::structure::{
    structure_document as structure_pipeline_document, DeterministicStructureInterpreter,
    StructurePipelineError,
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> Result<(), Box<dyn Error>> {
    tauri::Builder::default()
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&app_data_dir)?;
            let db_path = app_data_dir.join("summarizer.db");
            let conn = init_db(&db_path)?;

            app.manage(AppState {
                db: Mutex::new(conn),
            });
            Ok(())
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            ingest_document,
            parse_document,
            normalize_document,
            structure_document
        ])
        .run(tauri::generate_context!())?;
    Ok(())
}
