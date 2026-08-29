use crate::pipeline::contracts::{
    IngestedDocument, NormalizedDocument, ParsedDocument, PipelineEvent, PipelineFailure,
    PipelineRun, PipelineStage, PipelineState, PipelineWarning, StructuredDocument,
};
use crate::pipeline::schema::{self, MigrationError};
use crate::pipeline::state::{StateMachine, TransitionError};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::de::DeserializeOwned;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::Duration;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("JSON persistence error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Invalid persisted timestamp {value}: {source}")]
    InvalidTimestamp {
        value: String,
        #[source]
        source: chrono::ParseError,
    },
    #[error("Pipeline run not found: {0}")]
    RunNotFound(String),
    #[error("Document not found: {0}")]
    DocumentNotFound(String),
    #[error("Parsed artifact not found for pipeline run: {0}")]
    ParsedArtifactNotFound(String),
    #[error("Normalized artifact not found for pipeline run: {0}")]
    NormalizedArtifactNotFound(String),
    #[error("Invalid new ingestion: {0}")]
    InvalidIngestion(String),
    #[error("Parsed artifact document {artifact_document_id} does not match run document {run_document_id}")]
    ArtifactDocumentMismatch {
        artifact_document_id: String,
        run_document_id: String,
    },
    #[error("Normalized artifact document {artifact_document_id} does not match run document {run_document_id}")]
    NormalizedArtifactDocumentMismatch {
        artifact_document_id: String,
        run_document_id: String,
    },
    #[error("Normalized artifact metadata does not match its storage record for run {run_id}")]
    NormalizedArtifactMetadataMismatch { run_id: String },
    #[error("Normalized artifact integrity hash does not match for run {run_id}")]
    NormalizedArtifactIntegrityMismatch { run_id: String },
    #[error("Structured artifact document {artifact_document_id} does not match run document {run_document_id}")]
    StructuredArtifactDocumentMismatch {
        artifact_document_id: String,
        run_document_id: String,
    },
    #[error("Structured artifact metadata does not match its storage record for run {run_id}")]
    StructuredArtifactMetadataMismatch { run_id: String },
    #[error("Structured artifact integrity hash does not match for run {run_id}")]
    StructuredArtifactIntegrityMismatch { run_id: String },
    #[error("Persisted transition lost its expected state/version for run {run_id}")]
    StaleWrite { run_id: String },
    #[error(transparent)]
    Migration(#[from] MigrationError),
    #[error(transparent)]
    Transition(#[from] TransitionError),
}

#[derive(Default)]
struct TransitionPatch {
    warnings: Option<Vec<PipelineWarning>>,
    failure: Option<PipelineFailure>,
}

pub fn init_db(path: impl AsRef<Path>) -> Result<Connection, StoreError> {
    let mut conn = Connection::open(path)?;
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    schema::migrate(&mut conn)?;
    Ok(conn)
}

pub fn schema_version(conn: &Connection) -> Result<u32, StoreError> {
    Ok(schema::version(conn)?)
}

fn to_json<T: Serialize>(value: &T) -> Result<String, StoreError> {
    Ok(serde_json::to_string(value)?)
}

fn from_json<T: DeserializeOwned>(value: &str) -> Result<T, StoreError> {
    Ok(serde_json::from_str(value)?)
}

fn sha256_hex(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}

fn parse_timestamp(value: String) -> Result<DateTime<Utc>, StoreError> {
    DateTime::parse_from_rfc3339(&value)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|source| StoreError::InvalidTimestamp { value, source })
}

fn insert_document(conn: &Connection, document: &IngestedDocument) -> Result<(), StoreError> {
    let byte_size = i64::try_from(document.byte_size).map_err(|_| {
        StoreError::InvalidIngestion("source file exceeds SQLite integer range".to_string())
    })?;
    conn.execute(
        "INSERT INTO documents (
            document_id, original_filename, file_type, byte_size, content_hash,
            local_source_path, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            document.document_id,
            document.original_filename,
            document.file_type,
            byte_size,
            document.content_hash,
            document.local_source_path,
            document.created_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

pub fn get_document(
    conn: &Connection,
    document_id: &str,
) -> Result<Option<IngestedDocument>, StoreError> {
    let row = conn
        .query_row(
            "SELECT document_id, original_filename, file_type, byte_size, content_hash,
                    local_source_path, created_at
             FROM documents WHERE document_id = ?1",
            [document_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                ))
            },
        )
        .optional()?;

    row.map(
        |(
            document_id,
            original_filename,
            file_type,
            byte_size,
            content_hash,
            local_source_path,
            created_at,
        )| {
            let byte_size = u64::try_from(byte_size).map_err(|_| {
                StoreError::InvalidIngestion("persisted byte size is negative".to_string())
            })?;
            Ok(IngestedDocument {
                document_id,
                original_filename,
                file_type,
                byte_size,
                content_hash,
                local_source_path,
                created_at: parse_timestamp(created_at)?,
            })
        },
    )
    .transpose()
}

fn insert_pipeline_run(conn: &Connection, run: &PipelineRun) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO pipeline_runs (
            run_id, document_id, state, state_version, pipeline_version, 
            created_at, started_at, updated_at, completed_at, current_stage, 
            progress, warnings, failure, cancellation_requested, resumable
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            run.run_id,
            run.document_id,
            to_json(&run.state)?,
            run.state_version,
            run.pipeline_version,
            run.created_at.to_rfc3339(),
            run.started_at.map(|t| t.to_rfc3339()),
            run.updated_at.to_rfc3339(),
            run.completed_at.map(|t| t.to_rfc3339()),
            run.current_stage.as_ref().map(to_json).transpose()?,
            to_json(&run.progress)?,
            to_json(&run.warnings)?,
            run.failure.as_ref().map(to_json).transpose()?,
            run.cancellation_requested,
            run.resumable
        ],
    )?;
    Ok(())
}

pub fn get_pipeline_run(
    conn: &Connection,
    run_id: &str,
) -> Result<Option<PipelineRun>, StoreError> {
    let row = conn
        .query_row(
            "SELECT run_id, document_id, state, state_version, pipeline_version,
                    created_at, started_at, updated_at, completed_at, current_stage,
                    progress, warnings, failure, cancellation_requested, resumable
             FROM pipeline_runs WHERE run_id = ?1",
            [run_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, u32>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, String>(11)?,
                    row.get::<_, Option<String>>(12)?,
                    row.get::<_, bool>(13)?,
                    row.get::<_, bool>(14)?,
                ))
            },
        )
        .optional()?;

    row.map(
        |(
            run_id,
            document_id,
            state,
            state_version,
            pipeline_version,
            created_at,
            started_at,
            updated_at,
            completed_at,
            current_stage,
            progress,
            warnings,
            failure,
            cancellation_requested,
            resumable,
        )| {
            Ok(PipelineRun {
                run_id,
                document_id,
                state: from_json(&state)?,
                state_version,
                pipeline_version,
                created_at: parse_timestamp(created_at)?,
                started_at: started_at.map(parse_timestamp).transpose()?,
                updated_at: parse_timestamp(updated_at)?,
                completed_at: completed_at.map(parse_timestamp).transpose()?,
                current_stage: current_stage.as_deref().map(from_json).transpose()?,
                progress: from_json(&progress)?,
                warnings: from_json(&warnings)?,
                failure: failure.as_deref().map(from_json).transpose()?,
                cancellation_requested,
                resumable,
            })
        },
    )
    .transpose()
}

fn insert_pipeline_event(conn: &Connection, event: &PipelineEvent) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO pipeline_events (
            event_id, run_id, sequence_no, previous_state, next_state, timestamp,
            stage, work_unit_id, reason
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            event.event_id,
            event.run_id,
            event.sequence_no,
            event.previous_state.as_ref().map(to_json).transpose()?,
            to_json(&event.next_state)?,
            event.timestamp.to_rfc3339(),
            event.stage.as_ref().map(to_json).transpose()?,
            event.work_unit_id,
            event.reason,
        ],
    )?;
    Ok(())
}

pub fn list_pipeline_events(
    conn: &Connection,
    run_id: &str,
) -> Result<Vec<PipelineEvent>, StoreError> {
    let mut statement = conn.prepare(
        "SELECT event_id, run_id, sequence_no, previous_state, next_state, timestamp,
                stage, work_unit_id, reason
         FROM pipeline_events WHERE run_id = ?1 ORDER BY sequence_no",
    )?;
    let rows = statement.query_map([run_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, u32>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, Option<String>>(8)?,
        ))
    })?;

    let mut events = Vec::new();
    for row in rows {
        let (
            event_id,
            run_id,
            sequence_no,
            previous_state,
            next_state,
            timestamp,
            stage,
            work_unit_id,
            reason,
        ) = row?;
        events.push(PipelineEvent {
            event_id,
            run_id,
            sequence_no,
            previous_state: previous_state.as_deref().map(from_json).transpose()?,
            next_state: from_json(&next_state)?,
            timestamp: parse_timestamp(timestamp)?,
            stage: stage.as_deref().map(from_json).transpose()?,
            work_unit_id,
            reason,
        });
    }
    Ok(events)
}

pub fn get_parsed_document(
    conn: &Connection,
    run_id: &str,
) -> Result<Option<ParsedDocument>, StoreError> {
    let artifact = conn
        .query_row(
            "SELECT parsed_artifact FROM parsed_documents WHERE run_id = ?1",
            [run_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    artifact.as_deref().map(from_json).transpose()
}

pub fn get_normalized_document(
    conn: &Connection,
    run_id: &str,
) -> Result<Option<NormalizedDocument>, StoreError> {
    let row = conn
        .query_row(
            "SELECT normalized_documents.document_id, normalization_version, artifact_hash,
                    normalized_artifact, pipeline_runs.document_id
             FROM normalized_documents
             JOIN pipeline_runs USING (run_id)
             WHERE normalized_documents.run_id = ?1",
            [run_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .optional()?;

    row.map(
        |(document_id, normalization_version, artifact_hash, artifact_json, run_document_id)| {
            if sha256_hex(artifact_json.as_bytes()) != artifact_hash {
                return Err(StoreError::NormalizedArtifactIntegrityMismatch {
                    run_id: run_id.to_string(),
                });
            }
            let artifact: NormalizedDocument = from_json(&artifact_json)?;
            if artifact.document_id != document_id
                || artifact.normalization_version != normalization_version
                || document_id != run_document_id
            {
                return Err(StoreError::NormalizedArtifactMetadataMismatch {
                    run_id: run_id.to_string(),
                });
            }
            Ok(artifact)
        },
    )
    .transpose()
}

pub fn get_structured_document(
    conn: &Connection,
    run_id: &str,
) -> Result<Option<StructuredDocument>, StoreError> {
    let row = conn
        .query_row(
            "SELECT structured_documents.document_id, structure_version, artifact_hash,
                    structured_artifact, pipeline_runs.document_id
             FROM structured_documents
             JOIN pipeline_runs USING (run_id)
             WHERE structured_documents.run_id = ?1",
            [run_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .optional()?;

    row.map(
        |(document_id, structure_version, artifact_hash, artifact_json, run_document_id)| {
            if sha256_hex(artifact_json.as_bytes()) != artifact_hash {
                return Err(StoreError::StructuredArtifactIntegrityMismatch {
                    run_id: run_id.to_string(),
                });
            }
            let artifact: StructuredDocument = from_json(&artifact_json)?;
            if artifact.document_id != document_id
                || artifact.structure_version != structure_version
                || document_id != run_document_id
            {
                return Err(StoreError::StructuredArtifactMetadataMismatch {
                    run_id: run_id.to_string(),
                });
            }
            Ok(artifact)
        },
    )
    .transpose()
}

pub(super) fn persist_ingestion(
    conn: &mut Connection,
    document: &IngestedDocument,
    run: &PipelineRun,
) -> Result<PipelineRun, StoreError> {
    if run.document_id != document.document_id
        || run.state != PipelineState::Received
        || run.state_version != 1
    {
        return Err(StoreError::InvalidIngestion(
            "new run must reference the document at RECEIVED version 1".to_string(),
        ));
    }

    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    insert_document(&tx, document)?;
    insert_pipeline_run(&tx, run)?;
    insert_pipeline_event(
        &tx,
        &PipelineEvent {
            event_id: Uuid::new_v4().to_string(),
            run_id: run.run_id.clone(),
            sequence_no: 0,
            previous_state: None,
            next_state: PipelineState::Received,
            timestamp: run.created_at,
            stage: None,
            work_unit_id: None,
            reason: Some("run_created".to_string()),
        },
    )?;

    let ingesting = transition_in_tx(
        &tx,
        &run.run_id,
        PipelineState::Received,
        1,
        PipelineState::Ingesting,
        Some(PipelineStage::Ingest),
        None,
        TransitionPatch::default(),
    )?;
    let ingested = transition_in_tx(
        &tx,
        &run.run_id,
        PipelineState::Ingesting,
        ingesting.state_version,
        PipelineState::Ingested,
        Some(PipelineStage::Ingest),
        None,
        TransitionPatch::default(),
    )?;
    tx.commit()?;
    Ok(ingested)
}

#[cfg(test)]
fn transition_pipeline_run(
    conn: &mut Connection,
    run_id: &str,
    expected_state: PipelineState,
    expected_version: u32,
    next_state: PipelineState,
    stage: Option<PipelineStage>,
    reason: Option<String>,
) -> Result<PipelineRun, StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let run = transition_in_tx(
        &tx,
        run_id,
        expected_state,
        expected_version,
        next_state,
        stage,
        reason,
        TransitionPatch::default(),
    )?;
    tx.commit()?;
    Ok(run)
}

pub(super) fn start_parsing(
    conn: &mut Connection,
    run_id: &str,
    expected_version: u32,
) -> Result<(PipelineRun, IngestedDocument), StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current_run = get_pipeline_run(&tx, run_id)?
        .ok_or_else(|| StoreError::RunNotFound(run_id.to_string()))?;
    let document = get_document(&tx, &current_run.document_id)?
        .ok_or_else(|| StoreError::DocumentNotFound(current_run.document_id.clone()))?;
    let parsing_run = transition_in_tx(
        &tx,
        run_id,
        PipelineState::Ingested,
        expected_version,
        PipelineState::Parsing,
        Some(PipelineStage::Parse),
        None,
        TransitionPatch::default(),
    )?;
    tx.commit()?;
    Ok((parsing_run, document))
}

pub(super) fn complete_parsing(
    conn: &mut Connection,
    run_id: &str,
    expected_version: u32,
    parsed: &ParsedDocument,
    warnings: Vec<PipelineWarning>,
) -> Result<PipelineRun, StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let run = get_pipeline_run(&tx, run_id)?
        .ok_or_else(|| StoreError::RunNotFound(run_id.to_string()))?;
    if run.document_id != parsed.document_id {
        return Err(StoreError::ArtifactDocumentMismatch {
            artifact_document_id: parsed.document_id.clone(),
            run_document_id: run.document_id,
        });
    }

    insert_parsed_document(&tx, run_id, parsed)?;
    let parsed_run = transition_in_tx(
        &tx,
        run_id,
        PipelineState::Parsing,
        expected_version,
        PipelineState::Parsed,
        Some(PipelineStage::Parse),
        None,
        TransitionPatch {
            warnings: Some(warnings),
            failure: None,
        },
    )?;
    tx.commit()?;
    Ok(parsed_run)
}

pub(super) fn fail_parsing(
    conn: &mut Connection,
    run_id: &str,
    expected_version: u32,
    failure: PipelineFailure,
) -> Result<PipelineRun, StoreError> {
    let reason = Some(failure.code.clone());
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let failed_run = transition_in_tx(
        &tx,
        run_id,
        PipelineState::Parsing,
        expected_version,
        PipelineState::Failed,
        Some(PipelineStage::Parse),
        reason,
        TransitionPatch {
            warnings: None,
            failure: Some(failure),
        },
    )?;
    tx.commit()?;
    Ok(failed_run)
}

pub(super) fn start_normalizing(
    conn: &mut Connection,
    run_id: &str,
    expected_version: u32,
) -> Result<(PipelineRun, ParsedDocument), StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let parsed = get_parsed_document(&tx, run_id)?
        .ok_or_else(|| StoreError::ParsedArtifactNotFound(run_id.to_string()))?;
    let normalizing_run = transition_in_tx(
        &tx,
        run_id,
        PipelineState::Parsed,
        expected_version,
        PipelineState::Normalizing,
        Some(PipelineStage::Normalize),
        None,
        TransitionPatch::default(),
    )?;
    tx.commit()?;
    Ok((normalizing_run, parsed))
}

pub(super) fn complete_normalization(
    conn: &mut Connection,
    run_id: &str,
    expected_version: u32,
    normalized: &NormalizedDocument,
    warnings: Vec<PipelineWarning>,
) -> Result<PipelineRun, StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let run = get_pipeline_run(&tx, run_id)?
        .ok_or_else(|| StoreError::RunNotFound(run_id.to_string()))?;
    if run.document_id != normalized.document_id {
        return Err(StoreError::NormalizedArtifactDocumentMismatch {
            artifact_document_id: normalized.document_id.clone(),
            run_document_id: run.document_id,
        });
    }

    insert_normalized_document(&tx, run_id, normalized)?;
    let normalized_run = transition_in_tx(
        &tx,
        run_id,
        PipelineState::Normalizing,
        expected_version,
        PipelineState::Normalized,
        Some(PipelineStage::Normalize),
        None,
        TransitionPatch {
            warnings: Some(warnings),
            failure: None,
        },
    )?;
    tx.commit()?;
    Ok(normalized_run)
}

pub(super) fn fail_normalization(
    conn: &mut Connection,
    run_id: &str,
    expected_version: u32,
    failure: PipelineFailure,
) -> Result<PipelineRun, StoreError> {
    let reason = Some(failure.code.clone());
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let failed_run = transition_in_tx(
        &tx,
        run_id,
        PipelineState::Normalizing,
        expected_version,
        PipelineState::Failed,
        Some(PipelineStage::Normalize),
        reason,
        TransitionPatch {
            warnings: None,
            failure: Some(failure),
        },
    )?;
    tx.commit()?;
    Ok(failed_run)
}

pub(super) fn start_structuring(
    conn: &mut Connection,
    run_id: &str,
    expected_version: u32,
) -> Result<(PipelineRun, NormalizedDocument), StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let normalized = get_normalized_document(&tx, run_id)?
        .ok_or_else(|| StoreError::NormalizedArtifactNotFound(run_id.to_string()))?;
    let structuring_run = transition_in_tx(
        &tx,
        run_id,
        PipelineState::Normalized,
        expected_version,
        PipelineState::Structuring,
        Some(PipelineStage::Structure),
        None,
        TransitionPatch::default(),
    )?;
    tx.commit()?;
    Ok((structuring_run, normalized))
}

pub(super) fn complete_structuring(
    conn: &mut Connection,
    run_id: &str,
    expected_version: u32,
    structured: &StructuredDocument,
    warnings: Vec<PipelineWarning>,
) -> Result<PipelineRun, StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let run = get_pipeline_run(&tx, run_id)?
        .ok_or_else(|| StoreError::RunNotFound(run_id.to_string()))?;
    if run.document_id != structured.document_id {
        return Err(StoreError::StructuredArtifactDocumentMismatch {
            artifact_document_id: structured.document_id.clone(),
            run_document_id: run.document_id,
        });
    }

    insert_structured_document(&tx, run_id, structured)?;
    let structured_run = transition_in_tx(
        &tx,
        run_id,
        PipelineState::Structuring,
        expected_version,
        PipelineState::Structured,
        Some(PipelineStage::Structure),
        None,
        TransitionPatch {
            warnings: Some(warnings),
            failure: None,
        },
    )?;
    tx.commit()?;
    Ok(structured_run)
}

pub(super) fn fail_structuring(
    conn: &mut Connection,
    run_id: &str,
    expected_version: u32,
    failure: PipelineFailure,
) -> Result<PipelineRun, StoreError> {
    let reason = Some(failure.code.clone());
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let failed_run = transition_in_tx(
        &tx,
        run_id,
        PipelineState::Structuring,
        expected_version,
        PipelineState::Failed,
        Some(PipelineStage::Structure),
        reason,
        TransitionPatch {
            warnings: None,
            failure: Some(failure),
        },
    )?;
    tx.commit()?;
    Ok(failed_run)
}

fn insert_parsed_document(
    conn: &Connection,
    run_id: &str,
    parsed: &ParsedDocument,
) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO parsed_documents (
            run_id, document_id, parser_id, parser_version, parsed_artifact, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            run_id,
            parsed.document_id,
            parsed.parser_id,
            parsed.parser_version,
            to_json(parsed)?,
            Utc::now().to_rfc3339(),
        ],
    )?;
    Ok(())
}

fn insert_normalized_document(
    conn: &Connection,
    run_id: &str,
    normalized: &NormalizedDocument,
) -> Result<(), StoreError> {
    let artifact_json = to_json(normalized)?;
    let artifact_hash = sha256_hex(artifact_json.as_bytes());
    conn.execute(
        "INSERT INTO normalized_documents (
            run_id, document_id, normalization_version, artifact_hash,
            normalized_artifact, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            run_id,
            normalized.document_id,
            normalized.normalization_version,
            artifact_hash,
            artifact_json,
            Utc::now().to_rfc3339(),
        ],
    )?;
    Ok(())
}

fn insert_structured_document(
    conn: &Connection,
    run_id: &str,
    structured: &StructuredDocument,
) -> Result<(), StoreError> {
    let artifact_json = to_json(structured)?;
    let artifact_hash = sha256_hex(artifact_json.as_bytes());
    conn.execute(
        "INSERT INTO structured_documents (
            run_id, document_id, structure_version, artifact_hash,
            structured_artifact, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            run_id,
            structured.document_id,
            structured.structure_version,
            artifact_hash,
            artifact_json,
            Utc::now().to_rfc3339(),
        ],
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn transition_in_tx(
    tx: &Transaction<'_>,
    run_id: &str,
    expected_state: PipelineState,
    expected_version: u32,
    next_state: PipelineState,
    stage: Option<PipelineStage>,
    reason: Option<String>,
    patch: TransitionPatch,
) -> Result<PipelineRun, StoreError> {
    let mut run =
        get_pipeline_run(tx, run_id)?.ok_or_else(|| StoreError::RunNotFound(run_id.to_string()))?;
    let sequence_no: u32 = tx.query_row(
        "SELECT COALESCE(MAX(sequence_no), -1) + 1 FROM pipeline_events WHERE run_id = ?1",
        [run_id],
        |row| row.get(0),
    )?;
    let event = StateMachine::transition(
        &mut run,
        expected_state.clone(),
        next_state,
        expected_version,
        sequence_no,
        stage,
        reason,
    )?;

    if let Some(warnings) = patch.warnings {
        run.warnings = warnings;
    }
    if let Some(failure) = patch.failure {
        run.failure = Some(failure);
    }

    let changed = tx.execute(
        "UPDATE pipeline_runs SET
            state = ?1,
            state_version = ?2,
            pipeline_version = ?3,
            started_at = ?4,
            updated_at = ?5,
            completed_at = ?6,
            current_stage = ?7,
            progress = ?8,
            warnings = ?9,
            failure = ?10,
            cancellation_requested = ?11,
            resumable = ?12
         WHERE run_id = ?13 AND state = ?14 AND state_version = ?15",
        params![
            to_json(&run.state)?,
            run.state_version,
            run.pipeline_version,
            run.started_at.map(|timestamp| timestamp.to_rfc3339()),
            run.updated_at.to_rfc3339(),
            run.completed_at.map(|timestamp| timestamp.to_rfc3339()),
            run.current_stage.as_ref().map(to_json).transpose()?,
            to_json(&run.progress)?,
            to_json(&run.warnings)?,
            run.failure.as_ref().map(to_json).transpose()?,
            run.cancellation_requested,
            run.resumable,
            run_id,
            to_json(&expected_state)?,
            expected_version,
        ],
    )?;
    if changed != 1 {
        return Err(StoreError::StaleWrite {
            run_id: run_id.to_string(),
        });
    }
    insert_pipeline_event(tx, &event)?;
    Ok(run)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::ingest::ingest_pdf;
    use std::fs;
    use std::path::PathBuf;

    struct TestFile(PathBuf);

    impl TestFile {
        fn new(extension: &str, contents: &[u8]) -> Self {
            let path = std::env::temp_dir().join(format!("doc-sum-{}.{extension}", Uuid::new_v4()));
            fs::write(&path, contents).expect("test fixture should be writable");
            Self(path)
        }

        fn empty(extension: &str) -> Self {
            Self::new(extension, b"")
        }
    }

    impl Drop for TestFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[test]
    fn durable_transition_boundary_rejects_invalid_stale_and_lost_event_writes() {
        let source = TestFile::new("pdf", b"%PDF-1.4\nFOUNDATION_ATOMICITY_BODY");
        let mut conn = init_db(":memory:").expect("schema should initialize");
        let (_, run) = ingest_pdf(&mut conn, source.0.to_str().expect("UTF-8 path"))
            .expect("candidate should ingest");

        let initial_events = list_pipeline_events(&conn, &run.run_id).expect("events should load");
        assert_eq!(
            initial_events
                .iter()
                .map(|event| (event.sequence_no, event.next_state.clone()))
                .collect::<Vec<_>>(),
            vec![
                (0, PipelineState::Received),
                (1, PipelineState::Ingesting),
                (2, PipelineState::Ingested),
            ]
        );
        assert!(!serde_json::to_string(&initial_events)
            .expect("events should serialize")
            .contains("FOUNDATION_ATOMICITY_BODY"));

        let invalid = transition_pipeline_run(
            &mut conn,
            &run.run_id,
            PipelineState::Ingested,
            run.state_version,
            PipelineState::Parsed,
            None,
            None,
        );
        assert!(matches!(
            invalid,
            Err(StoreError::Transition(
                TransitionError::InvalidTransition { .. }
            ))
        ));

        let stale_state = transition_pipeline_run(
            &mut conn,
            &run.run_id,
            PipelineState::Received,
            run.state_version,
            PipelineState::Ingesting,
            None,
            None,
        );
        assert!(matches!(
            stale_state,
            Err(StoreError::Transition(
                TransitionError::StaleExpectedState { .. }
            ))
        ));

        conn.execute_batch(
            "CREATE TRIGGER test_fail_parsing_event
             BEFORE INSERT ON pipeline_events
             WHEN NEW.next_state = '\"Parsing\"'
             BEGIN
                 SELECT RAISE(ABORT, 'injected event write failure');
             END;",
        )
        .expect("failure trigger should install");
        let injected_failure = transition_pipeline_run(
            &mut conn,
            &run.run_id,
            PipelineState::Ingested,
            run.state_version,
            PipelineState::Parsing,
            Some(PipelineStage::Parse),
            None,
        );
        assert!(matches!(injected_failure, Err(StoreError::Sqlite(_))));
        let after_rollback = get_pipeline_run(&conn, &run.run_id)
            .expect("run should load")
            .expect("run should exist");
        assert_eq!(after_rollback.state, PipelineState::Ingested);
        assert_eq!(after_rollback.state_version, run.state_version);
        assert_eq!(
            list_pipeline_events(&conn, &run.run_id)
                .expect("events should load")
                .len(),
            initial_events.len()
        );

        conn.execute_batch("DROP TRIGGER test_fail_parsing_event;")
            .expect("test trigger should drop");
        let caller_a = transition_pipeline_run(
            &mut conn,
            &run.run_id,
            PipelineState::Ingested,
            run.state_version,
            PipelineState::Parsing,
            Some(PipelineStage::Parse),
            None,
        )
        .expect("first caller should advance");
        assert_eq!(caller_a.state_version, run.state_version + 1);

        let caller_b = transition_pipeline_run(
            &mut conn,
            &run.run_id,
            PipelineState::Ingested,
            run.state_version,
            PipelineState::Parsing,
            Some(PipelineStage::Parse),
            None,
        );
        assert!(matches!(
            caller_b,
            Err(StoreError::Transition(
                TransitionError::StaleExpectedState { .. }
            ))
        ));
        let persisted = get_pipeline_run(&conn, &run.run_id)
            .expect("run should load")
            .expect("run should exist");
        assert_eq!(persisted.state, PipelineState::Parsing);
        assert_eq!(persisted.state_version, caller_a.state_version);
    }

    #[test]
    fn pipeline_events_are_append_only_in_sqlite() {
        let source = TestFile::new("pdf", b"%PDF-1.4\nIMMUTABLE_EVENT_BODY");
        let mut conn = init_db(":memory:").expect("schema should initialize");
        let (_, run) = ingest_pdf(&mut conn, source.0.to_str().expect("UTF-8 path"))
            .expect("candidate should ingest");
        let events = list_pipeline_events(&conn, &run.run_id).expect("events should load");

        let update = conn.execute(
            "UPDATE pipeline_events SET reason = 'rewritten' WHERE event_id = ?1",
            [&events[0].event_id],
        );
        assert!(update.is_err());
        let delete = conn.execute(
            "DELETE FROM pipeline_events WHERE event_id = ?1",
            [&events[0].event_id],
        );
        assert!(delete.is_err());

        let persisted = list_pipeline_events(&conn, &run.run_id).expect("events should reload");
        assert_eq!(persisted.len(), events.len());
        assert_eq!(persisted[0].event_id, events[0].event_id);
        assert_eq!(persisted[0].reason, events[0].reason);
    }

    #[test]
    fn two_connections_reject_a_stale_persisted_transition() {
        let source = TestFile::new("pdf", b"%PDF-1.4\nCONCURRENT_CALLERS");
        let database = TestFile::empty("db");
        let mut caller_a = init_db(&database.0).expect("schema should initialize");
        let (_, run) = ingest_pdf(&mut caller_a, source.0.to_str().expect("UTF-8 path"))
            .expect("candidate should ingest");
        let mut caller_b = init_db(&database.0).expect("second connection should open");

        let observed_a = get_pipeline_run(&caller_a, &run.run_id)
            .expect("run should load")
            .expect("run should exist");
        let observed_b = get_pipeline_run(&caller_b, &run.run_id)
            .expect("run should load")
            .expect("run should exist");
        assert_eq!(observed_a.state_version, observed_b.state_version);

        transition_pipeline_run(
            &mut caller_a,
            &run.run_id,
            PipelineState::Ingested,
            observed_a.state_version,
            PipelineState::Parsing,
            Some(PipelineStage::Parse),
            None,
        )
        .expect("first connection should advance");
        let stale = transition_pipeline_run(
            &mut caller_b,
            &run.run_id,
            PipelineState::Ingested,
            observed_b.state_version,
            PipelineState::Parsing,
            Some(PipelineStage::Parse),
            None,
        );
        assert!(matches!(
            stale,
            Err(StoreError::Transition(
                TransitionError::StaleExpectedState { .. }
            ))
        ));
    }

    #[test]
    fn legacy_schema_migrates_once_and_reopens_deterministically() {
        let database = TestFile::empty("db");
        {
            let legacy = Connection::open(&database.0).expect("legacy DB should open");
            legacy
                .execute_batch(
                    r#"
                    CREATE TABLE documents (
                        document_id TEXT PRIMARY KEY,
                        original_filename TEXT NOT NULL,
                        file_type TEXT NOT NULL,
                        byte_size INTEGER NOT NULL,
                        content_hash TEXT NOT NULL,
                        local_source_path TEXT NOT NULL,
                        created_at TEXT NOT NULL
                    );
                    CREATE TABLE parsed_documents (
                        document_id TEXT PRIMARY KEY,
                        parser_id TEXT NOT NULL,
                        parser_version TEXT NOT NULL,
                        parsed_artifact TEXT NOT NULL
                    );
                    CREATE TABLE pipeline_runs (
                        run_id TEXT PRIMARY KEY,
                        document_id TEXT NOT NULL,
                        state TEXT NOT NULL,
                        state_version INTEGER NOT NULL,
                        pipeline_version TEXT NOT NULL,
                        created_at TEXT NOT NULL,
                        started_at TEXT,
                        updated_at TEXT NOT NULL,
                        completed_at TEXT,
                        current_stage TEXT,
                        progress TEXT NOT NULL,
                        warnings TEXT NOT NULL,
                        failure TEXT,
                        cancellation_requested BOOLEAN NOT NULL,
                        resumable BOOLEAN NOT NULL
                    );
                    CREATE TABLE pipeline_events (
                        event_id TEXT PRIMARY KEY,
                        run_id TEXT NOT NULL,
                        previous_state TEXT,
                        next_state TEXT NOT NULL,
                        timestamp TEXT NOT NULL,
                        stage TEXT,
                        work_unit_id TEXT,
                        reason TEXT
                    );
                    INSERT INTO documents VALUES (
                        'legacy-document', 'legacy.pdf', 'pdf', 5, 'hash', '/legacy.pdf',
                        '2026-01-01T00:00:00+00:00'
                    );
                    INSERT INTO pipeline_runs VALUES (
                        'legacy-run', 'legacy-document', '"Ingested"', 3, '1.0',
                        '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:01+00:00',
                        '2026-01-01T00:00:02+00:00', NULL, '"Ingest"',
                        '{"total_units":0,"completed_units":0,"failed_units":0}',
                        '[]', NULL, 0, 1
                    );
                    INSERT INTO pipeline_events VALUES (
                        'legacy-event', 'legacy-run', '"Ingesting"', '"Ingested"',
                        '2026-01-01T00:00:02+00:00', '"Ingest"', NULL, NULL
                    );
                    "#,
                )
                .expect("legacy schema should be created");
        }

        {
            let migrated = init_db(&database.0).expect("legacy DB should migrate");
            assert_eq!(schema_version(&migrated).expect("version should load"), 4);
            let run = get_pipeline_run(&migrated, "legacy-run")
                .expect("run should load")
                .expect("run should exist");
            assert_eq!(run.state, PipelineState::Ingested);
            let events = list_pipeline_events(&migrated, "legacy-run").expect("events should load");
            assert_eq!(events.len(), 2);
            assert_eq!(events[0].sequence_no, 0);
            assert_eq!(events[0].next_state, PipelineState::Received);
            assert_eq!(events[1].sequence_no, 1);
        }

        let reopened = init_db(&database.0).expect("migrated DB should reopen");
        assert_eq!(schema_version(&reopened).expect("version should load"), 4);
        assert_eq!(
            list_pipeline_events(&reopened, "legacy-run")
                .expect("events should load")
                .len(),
            2
        );
    }
}
