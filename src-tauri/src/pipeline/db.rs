use crate::pipeline::contracts::{
    AnalyzedDocument, ChunkedDocument, CitationArtifact, IngestedDocument, ModelProfileSnapshot,
    NormalizedDocument, ParsedDocument, PipelineEvent, PipelineFailure, PipelineRun, PipelineStage,
    PipelineState, PipelineWarning, RetryCheckpoint, RetryLineage, StructuredDocument,
    SummaryArtifact, SummaryProfile, SynthesizedDocument, VerifiedDocument,
};
use crate::pipeline::schema::{self, MigrationError};
use crate::pipeline::state::{StateMachine, TransitionError};
use chrono::{DateTime, Utc};
use rusqlite::{
    params, params_from_iter, Connection, OptionalExtension, Transaction, TransactionBehavior,
};
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
    #[error("Structured artifact not found for pipeline run: {0}")]
    StructuredArtifactNotFound(String),
    #[error("Chunked artifact not found for pipeline run: {0}")]
    ChunkedArtifactNotFound(String),
    #[error("Invalid new ingestion: {0}")]
    InvalidIngestion(String),
    #[error("Invalid model request owner: {0}")]
    InvalidRequestOwner(String),
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
    #[error("Chunked artifact document {artifact_document_id} does not match run document {run_document_id}")]
    ChunkedArtifactDocumentMismatch {
        artifact_document_id: String,
        run_document_id: String,
    },
    #[error("Chunked artifact metadata does not match its storage record for run {run_id}")]
    ChunkedArtifactMetadataMismatch { run_id: String },
    #[error("Chunked artifact integrity hash does not match for run {run_id}")]
    ChunkedArtifactIntegrityMismatch { run_id: String },
    #[error("{artifact_kind} artifact not found for pipeline run {run_id}")]
    DownstreamArtifactNotFound {
        artifact_kind: String,
        run_id: String,
    },
    #[error("{artifact_kind} artifact document {artifact_document_id} does not match run document {run_document_id}")]
    DownstreamArtifactDocumentMismatch {
        artifact_kind: String,
        artifact_document_id: String,
        run_document_id: String,
    },
    #[error(
        "{artifact_kind} artifact metadata does not match its storage record for run {run_id}"
    )]
    DownstreamArtifactMetadataMismatch {
        artifact_kind: String,
        run_id: String,
    },
    #[error("{artifact_kind} artifact integrity hash does not match for run {run_id}")]
    DownstreamArtifactIntegrityMismatch {
        artifact_kind: String,
        run_id: String,
    },
    #[error("Persisted transition lost its expected state/version for run {run_id}")]
    StaleWrite { run_id: String },
    #[error(
        "Verification artifact attempt ordinal {actual} does not match storage ordinal {expected}"
    )]
    AttemptOrdinalMismatch { expected: u32, actual: u32 },
    #[error("Invalid interrupted-run recovery transition from {state:?}")]
    InvalidRecoveryTransition { state: PipelineState },
    #[error("Pipeline run {run_id} cannot be retried: {reason}")]
    InvalidRetrySource { run_id: String, reason: String },
    #[error("Pipeline run {source_run_id} already has retry run {retry_run_id}")]
    RetryAlreadyExists {
        source_run_id: String,
        retry_run_id: String,
    },
    #[error("Retry lineage metadata is inconsistent for run {retry_run_id}")]
    RetryLineageMismatch { retry_run_id: String },
    #[error("Pipeline run {run_id} model profile does not match its immutable snapshot")]
    ModelProfileMismatch { run_id: String },
    #[error("Pipeline run {run_id} summary profile does not match its immutable selection")]
    SummaryProfileMismatch { run_id: String },
    #[error("Pipeline run {0} has no immutable summary profile")]
    SummaryProfileUnavailable(String),
    #[error("Pipeline run {run_id} cannot be cancelled from {state:?}")]
    CancellationNotAllowed {
        run_id: String,
        state: PipelineState,
    },
    #[error(transparent)]
    Migration(#[from] MigrationError),
    #[error(transparent)]
    Transition(#[from] TransitionError),
}

impl StoreError {
    pub(crate) fn is_stale_transition(&self) -> bool {
        matches!(
            self,
            Self::StaleWrite { .. }
                | Self::Transition(
                    TransitionError::StaleExpectedState { .. }
                        | TransitionError::ConcurrentModification { .. }
                )
        )
    }
}

#[derive(Default)]
struct TransitionPatch {
    warnings: Option<Vec<PipelineWarning>>,
    failure: Option<PipelineFailure>,
    cancellation_requested: Option<bool>,
}

#[derive(Debug, Clone)]
pub(super) enum InterruptedRunAction {
    Fail(PipelineFailure),
    CompleteCancellation,
}

#[derive(Debug, Clone)]
pub(super) struct InterruptedRunTransition {
    pub run_id: String,
    pub expected_state: PipelineState,
    pub expected_version: u32,
    pub action: InterruptedRunAction,
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

pub(crate) fn get_or_create_profile_suggestion_owner(
    conn: &mut Connection,
    source_content_hash: &str,
    task_contract_version: &str,
) -> Result<String, StoreError> {
    validate_request_owner_key(source_content_hash, task_contract_version)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let existing = tx
        .query_row(
            "SELECT owner_id FROM profile_suggestion_requests
             WHERE source_content_hash = ?1 AND task_contract_version = ?2",
            params![source_content_hash, task_contract_version],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if let Some(owner_id) = existing {
        let kind: Option<String> = tx
            .query_row(
                "SELECT owner_kind FROM model_request_owners WHERE owner_id = ?1",
                [&owner_id],
                |row| row.get(0),
            )
            .optional()?;
        if kind.as_deref() != Some("profile_suggestion") {
            return Err(StoreError::InvalidRequestOwner(
                "persisted profile suggestion owner kind is invalid".to_string(),
            ));
        }
        tx.commit()?;
        return Ok(owner_id);
    }

    let owner_id = Uuid::new_v4().hyphenated().to_string();
    let created_at = Utc::now().to_rfc3339();
    tx.execute(
        "INSERT INTO model_request_owners (owner_id, owner_kind, pipeline_run_id, created_at)
         VALUES (?1, 'profile_suggestion', NULL, ?2)",
        params![owner_id, created_at],
    )?;
    tx.execute(
        "INSERT INTO profile_suggestion_requests (
            source_content_hash, task_contract_version, owner_id, created_at
         ) VALUES (?1, ?2, ?3, ?4)",
        params![
            source_content_hash,
            task_contract_version,
            owner_id,
            created_at
        ],
    )?;
    tx.commit()?;
    Ok(owner_id)
}

fn validate_request_owner_key(
    source_content_hash: &str,
    task_contract_version: &str,
) -> Result<(), StoreError> {
    let valid_hash = source_content_hash.len() == 64
        && source_content_hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if !valid_hash {
        return Err(StoreError::InvalidRequestOwner(
            "source content hash must be a lowercase SHA-256 digest".to_string(),
        ));
    }
    let valid_version = !task_contract_version.is_empty()
        && task_contract_version.len() <= 128
        && task_contract_version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'@'));
    if !valid_version {
        return Err(StoreError::InvalidRequestOwner(
            "task contract version is invalid".to_string(),
        ));
    }
    Ok(())
}

pub(crate) fn ensure_run_model_profile(
    conn: &Connection,
    run_id: &str,
    snapshot: Option<&ModelProfileSnapshot>,
) -> Result<(), StoreError> {
    let persisted = conn
        .query_row(
            "SELECT profile_snapshot FROM pipeline_run_model_profiles WHERE run_id = ?1",
            [run_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    match (persisted, snapshot) {
        (Some(persisted), Some(snapshot)) => {
            let persisted: ModelProfileSnapshot = from_json(&persisted)?;
            if &persisted != snapshot {
                return Err(StoreError::ModelProfileMismatch {
                    run_id: run_id.to_string(),
                });
            }
            Ok(())
        }
        (Some(_), None) => Err(StoreError::ModelProfileMismatch {
            run_id: run_id.to_string(),
        }),
        (None, Some(snapshot)) => {
            let run_exists: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM pipeline_runs WHERE run_id = ?1)",
                [run_id],
                |row| row.get(0),
            )?;
            if !run_exists {
                return Err(StoreError::RunNotFound(run_id.to_string()));
            }
            conn.execute(
                "INSERT INTO pipeline_run_model_profiles (run_id, profile_snapshot, created_at)
                 VALUES (?1, ?2, ?3)",
                params![run_id, to_json(snapshot)?, Utc::now().to_rfc3339()],
            )?;
            Ok(())
        }
        (None, None) => Ok(()),
    }
}

pub(crate) fn get_run_model_profile(
    conn: &Connection,
    run_id: &str,
) -> Result<Option<ModelProfileSnapshot>, StoreError> {
    conn.query_row(
        "SELECT profile_snapshot FROM pipeline_run_model_profiles WHERE run_id = ?1",
        [run_id],
        |row| row.get::<_, String>(0),
    )
    .optional()?
    .map(|value| from_json(&value))
    .transpose()
}

pub(crate) fn ensure_run_summary_profile(
    conn: &Connection,
    run_id: &str,
    profile: SummaryProfile,
) -> Result<(), StoreError> {
    let persisted = conn
        .query_row(
            "SELECT summary_profile FROM pipeline_run_summary_profiles WHERE run_id = ?1",
            [run_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    match persisted {
        Some(persisted) => {
            let persisted: SummaryProfile = from_json(&persisted)?;
            if persisted != profile {
                return Err(StoreError::SummaryProfileMismatch {
                    run_id: run_id.to_string(),
                });
            }
            Ok(())
        }
        None => {
            let run_exists: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM pipeline_runs WHERE run_id = ?1)",
                [run_id],
                |row| row.get(0),
            )?;
            if !run_exists {
                return Err(StoreError::RunNotFound(run_id.to_string()));
            }
            conn.execute(
                "INSERT INTO pipeline_run_summary_profiles (run_id, summary_profile, created_at)
                 VALUES (?1, ?2, ?3)",
                params![run_id, to_json(&profile)?, Utc::now().to_rfc3339()],
            )?;
            Ok(())
        }
    }
}

pub(crate) fn get_run_summary_profile(
    conn: &Connection,
    run_id: &str,
) -> Result<Option<SummaryProfile>, StoreError> {
    conn.query_row(
        "SELECT summary_profile FROM pipeline_run_summary_profiles WHERE run_id = ?1",
        [run_id],
        |row| row.get::<_, String>(0),
    )
    .optional()?
    .map(|value| from_json(&value))
    .transpose()
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

pub fn get_retry_lineage_for_retry(
    conn: &Connection,
    retry_run_id: &str,
) -> Result<Option<RetryLineage>, StoreError> {
    load_retry_lineage(conn, "WHERE retry_lineage.retry_run_id = ?1", retry_run_id)
}

pub fn get_retry_lineage_for_source(
    conn: &Connection,
    source_run_id: &str,
) -> Result<Option<RetryLineage>, StoreError> {
    load_retry_lineage(
        conn,
        "WHERE retry_lineage.source_run_id = ?1",
        source_run_id,
    )
}

fn load_retry_lineage(
    conn: &Connection,
    predicate: &str,
    run_id: &str,
) -> Result<Option<RetryLineage>, StoreError> {
    let query = format!(
        "SELECT retry_lineage.retry_run_id, retry_lineage.source_run_id,
                retry_lineage.checkpoint, retry_lineage.created_at,
                retry_run.document_id, source_run.document_id, retry_run.created_at
         FROM pipeline_run_retries AS retry_lineage
         JOIN pipeline_runs AS retry_run ON retry_run.run_id = retry_lineage.retry_run_id
         JOIN pipeline_runs AS source_run ON source_run.run_id = retry_lineage.source_run_id
         {predicate}"
    );
    let row = conn
        .query_row(&query, [run_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
            ))
        })
        .optional()?;

    row.map(
        |(
            retry_run_id,
            source_run_id,
            checkpoint,
            created_at,
            retry_document_id,
            source_document_id,
            retry_created_at,
        )| {
            let lineage = RetryLineage {
                retry_run_id: retry_run_id.clone(),
                source_run_id,
                checkpoint: from_json(&checkpoint)?,
                created_at: parse_timestamp(created_at)?,
            };
            if lineage.checkpoint != RetryCheckpoint::Ingested
                || retry_document_id != source_document_id
                || lineage.created_at != parse_timestamp(retry_created_at)?
            {
                return Err(StoreError::RetryLineageMismatch { retry_run_id });
            }
            Ok(lineage)
        },
    )
    .transpose()
}

pub fn list_recent_pipeline_runs(
    conn: &Connection,
    limit: u32,
) -> Result<Vec<PipelineRun>, StoreError> {
    if limit == 0 {
        return Ok(Vec::new());
    }

    let run_ids = {
        let mut statement = conn.prepare(
            "SELECT run_id
             FROM pipeline_runs
             ORDER BY updated_at DESC, run_id DESC
             LIMIT ?1",
        )?;
        let rows = statement.query_map([i64::from(limit)], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    run_ids
        .into_iter()
        .map(|run_id| {
            get_pipeline_run(conn, &run_id)?
                .ok_or_else(|| StoreError::RunNotFound(run_id.to_string()))
        })
        .collect()
}

pub(super) fn list_pipeline_runs_for_recovery(
    conn: &Connection,
) -> Result<Vec<PipelineRun>, StoreError> {
    let mut states = PipelineState::IMPLEMENTED_ACTIVE_STATES
        .iter()
        .map(to_json)
        .collect::<Result<Vec<_>, _>>()?;
    states.push(to_json(&PipelineState::Cancelling)?);
    let placeholders = (1..=states.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let query = format!(
        "SELECT run_id FROM pipeline_runs
         WHERE state IN ({placeholders})
         ORDER BY run_id"
    );
    let run_ids = {
        let mut statement = conn.prepare(&query)?;
        let rows = statement.query_map(params_from_iter(&states), |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    run_ids
        .into_iter()
        .map(|run_id| {
            get_pipeline_run(conn, &run_id)?
                .ok_or_else(|| StoreError::RunNotFound(run_id.to_string()))
        })
        .collect()
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

pub fn get_chunked_document(
    conn: &Connection,
    run_id: &str,
) -> Result<Option<ChunkedDocument>, StoreError> {
    let row = conn
        .query_row(
            "SELECT chunked_documents.document_id, chunking_version, artifact_hash,
                    chunked_artifact, pipeline_runs.document_id
             FROM chunked_documents
             JOIN pipeline_runs USING (run_id)
             WHERE chunked_documents.run_id = ?1",
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
        |(document_id, chunking_version, artifact_hash, artifact_json, run_document_id)| {
            if sha256_hex(artifact_json.as_bytes()) != artifact_hash {
                return Err(StoreError::ChunkedArtifactIntegrityMismatch {
                    run_id: run_id.to_string(),
                });
            }
            let artifact: ChunkedDocument = from_json(&artifact_json)?;
            if artifact.document_id != document_id
                || artifact.chunking_version != chunking_version
                || document_id != run_document_id
            {
                return Err(StoreError::ChunkedArtifactMetadataMismatch {
                    run_id: run_id.to_string(),
                });
            }
            Ok(artifact)
        },
    )
    .transpose()
}

trait DownstreamArtifactMetadata {
    fn document_id(&self) -> &str;
    fn version(&self) -> &str;
}

#[derive(Clone, Copy)]
struct DownstreamArtifactTable {
    table: &'static str,
    version_column: &'static str,
    artifact_column: &'static str,
}

const ANALYZED_ARTIFACT_TABLE: DownstreamArtifactTable = DownstreamArtifactTable {
    table: "analyzed_documents",
    version_column: "analysis_version",
    artifact_column: "analyzed_artifact",
};
const SYNTHESIZED_ARTIFACT_TABLE: DownstreamArtifactTable = DownstreamArtifactTable {
    table: "synthesized_documents",
    version_column: "synthesis_version",
    artifact_column: "synthesized_artifact",
};
const VERIFIED_ARTIFACT_TABLE: DownstreamArtifactTable = DownstreamArtifactTable {
    table: "verified_documents",
    version_column: "verification_version",
    artifact_column: "verified_artifact",
};
const SUMMARY_ARTIFACT_TABLE: DownstreamArtifactTable = DownstreamArtifactTable {
    table: "summary_artifacts",
    version_column: "summary_version",
    artifact_column: "summary_artifact",
};

impl DownstreamArtifactMetadata for AnalyzedDocument {
    fn document_id(&self) -> &str {
        &self.document_id
    }

    fn version(&self) -> &str {
        &self.analysis_version
    }
}

impl DownstreamArtifactMetadata for SynthesizedDocument {
    fn document_id(&self) -> &str {
        &self.document_id
    }

    fn version(&self) -> &str {
        &self.synthesis_version
    }
}

impl DownstreamArtifactMetadata for VerifiedDocument {
    fn document_id(&self) -> &str {
        &self.document_id
    }

    fn version(&self) -> &str {
        &self.verification_version
    }
}

impl DownstreamArtifactMetadata for SummaryArtifact {
    fn document_id(&self) -> &str {
        &self.document_id
    }

    fn version(&self) -> &str {
        &self.summary_version
    }
}

fn get_downstream_artifact<T: DeserializeOwned + DownstreamArtifactMetadata>(
    conn: &Connection,
    run_id: &str,
    table: DownstreamArtifactTable,
    artifact_kind: &str,
) -> Result<Option<T>, StoreError> {
    let DownstreamArtifactTable {
        table,
        version_column,
        artifact_column,
    } = table;
    let sql = format!(
        "SELECT artifact.document_id, artifact.{version_column}, artifact.artifact_hash,
                artifact.{artifact_column}, pipeline_runs.document_id
         FROM {table} AS artifact
         JOIN pipeline_runs USING (run_id)
         WHERE artifact.run_id = ?1"
    );
    let row = conn
        .query_row(&sql, [run_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .optional()?;

    row.map(
        |(document_id, version, artifact_hash, artifact_json, run_document_id)| {
            if sha256_hex(artifact_json.as_bytes()) != artifact_hash {
                return Err(StoreError::DownstreamArtifactIntegrityMismatch {
                    artifact_kind: artifact_kind.to_string(),
                    run_id: run_id.to_string(),
                });
            }
            let artifact: T = from_json(&artifact_json)?;
            if artifact.document_id() != document_id
                || artifact.version() != version
                || document_id != run_document_id
            {
                return Err(StoreError::DownstreamArtifactMetadataMismatch {
                    artifact_kind: artifact_kind.to_string(),
                    run_id: run_id.to_string(),
                });
            }
            Ok(artifact)
        },
    )
    .transpose()
}

pub fn get_analyzed_document(
    conn: &Connection,
    run_id: &str,
) -> Result<Option<AnalyzedDocument>, StoreError> {
    get_downstream_artifact(conn, run_id, ANALYZED_ARTIFACT_TABLE, "analyzed")
}

pub fn get_synthesized_document(
    conn: &Connection,
    run_id: &str,
) -> Result<Option<SynthesizedDocument>, StoreError> {
    get_downstream_artifact(conn, run_id, SYNTHESIZED_ARTIFACT_TABLE, "synthesized")
}

pub fn get_synthesis_attempt(
    conn: &Connection,
    run_id: &str,
    attempt_ordinal: u32,
) -> Result<Option<SynthesizedDocument>, StoreError> {
    get_summary_attempt_artifact(
        conn,
        run_id,
        attempt_ordinal,
        "summary_synthesis_attempts",
        "synthesis_version",
        "synthesized_artifact",
        "synthesis attempt",
    )
}

pub fn get_verified_document(
    conn: &Connection,
    run_id: &str,
) -> Result<Option<VerifiedDocument>, StoreError> {
    get_downstream_artifact(conn, run_id, VERIFIED_ARTIFACT_TABLE, "verified")
}

pub fn get_verification_attempt(
    conn: &Connection,
    run_id: &str,
    attempt_ordinal: u32,
) -> Result<Option<VerifiedDocument>, StoreError> {
    let artifact: Option<VerifiedDocument> = get_summary_attempt_artifact(
        conn,
        run_id,
        attempt_ordinal,
        "summary_verification_attempts",
        "verification_version",
        "verified_artifact",
        "verification attempt",
    )?;
    if let Some(verified) = &artifact {
        ensure_attempt_ordinal(attempt_ordinal, verified.synthesis_attempt_ordinal)?;
    }
    Ok(artifact)
}

#[allow(clippy::too_many_arguments)]
fn get_summary_attempt_artifact<T: DeserializeOwned + DownstreamArtifactMetadata>(
    conn: &Connection,
    run_id: &str,
    attempt_ordinal: u32,
    table: &str,
    version_column: &str,
    artifact_column: &str,
    artifact_kind: &str,
) -> Result<Option<T>, StoreError> {
    let sql = format!(
        "SELECT attempt.document_id, attempt.{version_column}, attempt.artifact_hash,
                attempt.{artifact_column}, pipeline_runs.document_id
         FROM {table} AS attempt
         JOIN pipeline_runs USING (run_id)
         WHERE attempt.run_id = ?1 AND attempt.attempt_ordinal = ?2"
    );
    let row = conn
        .query_row(&sql, params![run_id, attempt_ordinal], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .optional()?;

    row.map(
        |(document_id, version, artifact_hash, artifact_json, run_document_id)| {
            if sha256_hex(artifact_json.as_bytes()) != artifact_hash {
                return Err(StoreError::DownstreamArtifactIntegrityMismatch {
                    artifact_kind: artifact_kind.to_string(),
                    run_id: run_id.to_string(),
                });
            }
            let artifact: T = from_json(&artifact_json)?;
            if artifact.document_id() != document_id
                || artifact.version() != version
                || document_id != run_document_id
            {
                return Err(StoreError::DownstreamArtifactMetadataMismatch {
                    artifact_kind: artifact_kind.to_string(),
                    run_id: run_id.to_string(),
                });
            }
            Ok(artifact)
        },
    )
    .transpose()
}

pub fn get_summary_artifact(
    conn: &Connection,
    run_id: &str,
) -> Result<Option<SummaryArtifact>, StoreError> {
    let artifact: Option<SummaryArtifact> =
        get_downstream_artifact(conn, run_id, SUMMARY_ARTIFACT_TABLE, "summary")?;
    if let Some(summary) = &artifact {
        if summary.calculate_integrity_hash()? != summary.integrity_hash {
            return Err(StoreError::DownstreamArtifactIntegrityMismatch {
                artifact_kind: "summary".to_string(),
                run_id: run_id.to_string(),
            });
        }
    }
    Ok(artifact)
}

pub fn get_citation_artifact(
    conn: &Connection,
    run_id: &str,
) -> Result<Option<CitationArtifact>, StoreError> {
    let row = conn
        .query_row(
            "SELECT citation.document_id, citation.citation_version,
                    citation.summary_integrity_hash, citation.artifact_hash,
                    citation.citation_artifact, citation.created_at,
                    pipeline_runs.document_id
             FROM citation_artifacts AS citation
             JOIN pipeline_runs USING (run_id)
             WHERE citation.run_id = ?1",
            [run_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
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
            citation_version,
            summary_integrity_hash,
            artifact_hash,
            artifact_json,
            created_at,
            run_document_id,
        )| {
            if sha256_hex(artifact_json.as_bytes()) != artifact_hash {
                return Err(StoreError::DownstreamArtifactIntegrityMismatch {
                    artifact_kind: "citation".to_string(),
                    run_id: run_id.to_string(),
                });
            }
            let artifact: CitationArtifact = from_json(&artifact_json)?;
            if artifact.document_id != document_id
                || artifact.citation_version != citation_version
                || artifact.summary_integrity_hash != summary_integrity_hash
                || artifact.created_at.to_rfc3339() != created_at
                || document_id != run_document_id
            {
                return Err(StoreError::DownstreamArtifactMetadataMismatch {
                    artifact_kind: "citation".to_string(),
                    run_id: run_id.to_string(),
                });
            }
            if artifact.calculate_integrity_hash()? != artifact.integrity_hash {
                return Err(StoreError::DownstreamArtifactIntegrityMismatch {
                    artifact_kind: "citation".to_string(),
                    run_id: run_id.to_string(),
                });
            }
            Ok(artifact)
        },
    )
    .transpose()
}

pub fn summary_artifact_exists(conn: &Connection, run_id: &str) -> Result<bool, StoreError> {
    let count: u32 = conn.query_row(
        "SELECT COUNT(*) FROM summary_artifacts WHERE run_id = ?1",
        [run_id],
        |row| row.get(0),
    )?;
    Ok(count == 1)
}

pub(super) fn persist_ingestion(
    conn: &mut Connection,
    document: &IngestedDocument,
    run: &PipelineRun,
) -> Result<PipelineRun, StoreError> {
    persist_ingestion_with_profiles(conn, document, run, None, SummaryProfile::General)
}

pub(crate) fn persist_ingestion_with_profiles(
    conn: &mut Connection,
    document: &IngestedDocument,
    run: &PipelineRun,
    profile_snapshot: Option<&ModelProfileSnapshot>,
    summary_profile: SummaryProfile,
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
    let ingested = persist_ingestion_in_transaction(&tx, document, run, summary_profile)?;
    ensure_run_model_profile(&tx, &ingested.run_id, profile_snapshot)?;
    tx.commit()?;
    Ok(ingested)
}

pub(crate) fn persist_ingestion_in_transaction(
    tx: &Transaction<'_>,
    document: &IngestedDocument,
    run: &PipelineRun,
    summary_profile: SummaryProfile,
) -> Result<PipelineRun, StoreError> {
    if run.document_id != document.document_id
        || run.state != PipelineState::Received
        || run.state_version != 1
    {
        return Err(StoreError::InvalidIngestion(
            "new run must reference the document at RECEIVED version 1".to_string(),
        ));
    }

    insert_document(tx, document)?;
    let ingested = persist_received_run_to_ingested(tx, run, "run_created", None)?;
    ensure_run_summary_profile(tx, &ingested.run_id, summary_profile)?;
    Ok(ingested)
}

fn persist_received_run_to_ingested(
    tx: &Transaction<'_>,
    run: &PipelineRun,
    creation_reason: &str,
    transition_reason: Option<&str>,
) -> Result<PipelineRun, StoreError> {
    insert_pipeline_run(tx, run)?;
    insert_pipeline_event(
        tx,
        &PipelineEvent {
            event_id: Uuid::new_v4().to_string(),
            run_id: run.run_id.clone(),
            sequence_no: 0,
            previous_state: None,
            next_state: PipelineState::Received,
            timestamp: run.created_at,
            stage: None,
            work_unit_id: None,
            reason: Some(creation_reason.to_string()),
        },
    )?;

    let ingesting = transition_in_tx(
        tx,
        &run.run_id,
        PipelineState::Received,
        1,
        PipelineState::Ingesting,
        Some(PipelineStage::Ingest),
        transition_reason.map(str::to_string),
        TransitionPatch::default(),
    )?;
    let ingested = transition_in_tx(
        tx,
        &run.run_id,
        PipelineState::Ingesting,
        ingesting.state_version,
        PipelineState::Ingested,
        Some(PipelineStage::Ingest),
        transition_reason.map(str::to_string),
        TransitionPatch::default(),
    )?;
    Ok(ingested)
}

pub(crate) fn validate_retry_source(
    conn: &Connection,
    source_run_id: &str,
    expected_source_version: u32,
) -> Result<(PipelineRun, RetryCheckpoint), StoreError> {
    let source_run = get_pipeline_run(conn, source_run_id)?
        .ok_or_else(|| StoreError::RunNotFound(source_run_id.to_string()))?;
    if source_run.state != PipelineState::Failed {
        return Err(StoreError::InvalidRetrySource {
            run_id: source_run_id.to_string(),
            reason: "only a failed run can create a retry".to_string(),
        });
    }
    if source_run.state_version != expected_source_version {
        return Err(StoreError::Transition(
            TransitionError::ConcurrentModification {
                expected: expected_source_version,
                found: source_run.state_version,
            },
        ));
    }
    let checkpoint =
        source_run
            .retry_checkpoint()
            .ok_or_else(|| StoreError::InvalidRetrySource {
                run_id: source_run_id.to_string(),
                reason: "the failure has no reusable checkpoint".to_string(),
            })?;
    if get_run_model_profile(conn, source_run_id)?.is_none() {
        return Err(StoreError::InvalidRetrySource {
            run_id: source_run_id.to_string(),
            reason: "the failed run has no immutable model profile to inherit".to_string(),
        });
    }
    if get_run_summary_profile(conn, source_run_id)?.is_none() {
        return Err(StoreError::InvalidRetrySource {
            run_id: source_run_id.to_string(),
            reason: "the failed run has no immutable summary profile to inherit".to_string(),
        });
    }
    if let Some(existing) = get_retry_lineage_for_source(conn, source_run_id)? {
        return Err(StoreError::RetryAlreadyExists {
            source_run_id: source_run_id.to_string(),
            retry_run_id: existing.retry_run_id,
        });
    }
    Ok((source_run, checkpoint))
}

pub(super) fn create_retry_run(
    conn: &mut Connection,
    source_run_id: &str,
    expected_source_version: u32,
    retry_run: &PipelineRun,
) -> Result<(PipelineRun, IngestedDocument, RetryLineage), StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (source_run, checkpoint) =
        validate_retry_source(&tx, source_run_id, expected_source_version)?;
    if retry_run.document_id != source_run.document_id
        || retry_run.state != PipelineState::Received
        || retry_run.state_version != 1
        || retry_run.failure.is_some()
        || retry_run.completed_at.is_some()
    {
        return Err(StoreError::InvalidRetrySource {
            run_id: source_run_id.to_string(),
            reason: "new retry must reference the source document at RECEIVED version 1"
                .to_string(),
        });
    }
    let document = get_document(&tx, &source_run.document_id)?
        .ok_or_else(|| StoreError::DocumentNotFound(source_run.document_id.clone()))?;
    let ingested = persist_received_run_to_ingested(
        &tx,
        retry_run,
        "retry_run_created",
        Some("retry_checkpoint_reused"),
    )?;
    let parsing = transition_in_tx(
        &tx,
        &retry_run.run_id,
        PipelineState::Ingested,
        ingested.state_version,
        PipelineState::Parsing,
        Some(PipelineStage::Parse),
        Some("retry_processing_started".to_string()),
        TransitionPatch::default(),
    )?;
    let lineage = RetryLineage {
        retry_run_id: retry_run.run_id.clone(),
        source_run_id: source_run_id.to_string(),
        checkpoint,
        created_at: retry_run.created_at,
    };
    tx.execute(
        "INSERT INTO pipeline_run_retries (
            retry_run_id, source_run_id, checkpoint, created_at
         ) VALUES (?1, ?2, ?3, ?4)",
        params![
            lineage.retry_run_id,
            lineage.source_run_id,
            to_json(&lineage.checkpoint)?,
            lineage.created_at.to_rfc3339(),
        ],
    )?;
    tx.execute(
        "INSERT INTO pipeline_run_model_profiles (run_id, profile_snapshot, created_at)
         SELECT ?1, profile_snapshot, ?2
         FROM pipeline_run_model_profiles WHERE run_id = ?3",
        params![
            retry_run.run_id,
            retry_run.created_at.to_rfc3339(),
            source_run_id
        ],
    )?;
    let copied_summary_profile = tx.execute(
        "INSERT INTO pipeline_run_summary_profiles (run_id, summary_profile, created_at)
         SELECT ?1, summary_profile, ?2
         FROM pipeline_run_summary_profiles WHERE run_id = ?3",
        params![
            retry_run.run_id,
            retry_run.created_at.to_rfc3339(),
            source_run_id
        ],
    )?;
    if copied_summary_profile != 1 {
        return Err(StoreError::SummaryProfileUnavailable(
            source_run_id.to_string(),
        ));
    }
    tx.commit()?;
    Ok((parsing, document, lineage))
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
            cancellation_requested: None,
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
            cancellation_requested: None,
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
            cancellation_requested: None,
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
            cancellation_requested: None,
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
            cancellation_requested: None,
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
            cancellation_requested: None,
        },
    )?;
    tx.commit()?;
    Ok(failed_run)
}

pub(super) fn start_chunking(
    conn: &mut Connection,
    run_id: &str,
    expected_version: u32,
) -> Result<(PipelineRun, NormalizedDocument, StructuredDocument), StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let normalized = get_normalized_document(&tx, run_id)?
        .ok_or_else(|| StoreError::NormalizedArtifactNotFound(run_id.to_string()))?;
    let structured = get_structured_document(&tx, run_id)?
        .ok_or_else(|| StoreError::StructuredArtifactNotFound(run_id.to_string()))?;
    let chunking_run = transition_in_tx(
        &tx,
        run_id,
        PipelineState::Structured,
        expected_version,
        PipelineState::Chunking,
        Some(PipelineStage::Chunk),
        None,
        TransitionPatch::default(),
    )?;
    tx.commit()?;
    Ok((chunking_run, normalized, structured))
}

pub(super) fn complete_chunking(
    conn: &mut Connection,
    run_id: &str,
    expected_version: u32,
    chunked: &ChunkedDocument,
    warnings: Vec<PipelineWarning>,
) -> Result<PipelineRun, StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let run = get_pipeline_run(&tx, run_id)?
        .ok_or_else(|| StoreError::RunNotFound(run_id.to_string()))?;
    if run.document_id != chunked.document_id {
        return Err(StoreError::ChunkedArtifactDocumentMismatch {
            artifact_document_id: chunked.document_id.clone(),
            run_document_id: run.document_id,
        });
    }

    insert_chunked_document(&tx, run_id, chunked)?;
    let chunked_run = transition_in_tx(
        &tx,
        run_id,
        PipelineState::Chunking,
        expected_version,
        PipelineState::Chunked,
        Some(PipelineStage::Chunk),
        None,
        TransitionPatch {
            warnings: Some(warnings),
            failure: None,
            cancellation_requested: None,
        },
    )?;
    tx.commit()?;
    Ok(chunked_run)
}

pub(super) fn fail_chunking(
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
        PipelineState::Chunking,
        expected_version,
        PipelineState::Failed,
        Some(PipelineStage::Chunk),
        reason,
        TransitionPatch {
            warnings: None,
            failure: Some(failure),
            cancellation_requested: None,
        },
    )?;
    tx.commit()?;
    Ok(failed_run)
}

pub(super) fn start_analysis(
    conn: &mut Connection,
    run_id: &str,
    expected_version: u32,
) -> Result<(PipelineRun, ChunkedDocument), StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let chunked = get_chunked_document(&tx, run_id)?
        .ok_or_else(|| StoreError::ChunkedArtifactNotFound(run_id.to_string()))?;
    let analyzing_run = transition_in_tx(
        &tx,
        run_id,
        PipelineState::Chunked,
        expected_version,
        PipelineState::Analyzing,
        Some(PipelineStage::Analyze),
        None,
        TransitionPatch::default(),
    )?;
    tx.commit()?;
    Ok((analyzing_run, chunked))
}

pub(super) fn complete_analysis(
    conn: &mut Connection,
    run_id: &str,
    expected_version: u32,
    analyzed: &AnalyzedDocument,
    warnings: Vec<PipelineWarning>,
) -> Result<PipelineRun, StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    ensure_run_document_matches(&tx, run_id, "analyzed", &analyzed.document_id)?;
    insert_analyzed_document(&tx, run_id, analyzed)?;
    let analyzed_run = transition_in_tx(
        &tx,
        run_id,
        PipelineState::Analyzing,
        expected_version,
        PipelineState::Analyzed,
        Some(PipelineStage::Analyze),
        None,
        TransitionPatch {
            warnings: Some(warnings),
            failure: None,
            cancellation_requested: None,
        },
    )?;
    tx.commit()?;
    Ok(analyzed_run)
}

pub(super) fn fail_analysis(
    conn: &mut Connection,
    run_id: &str,
    expected_version: u32,
    failure: PipelineFailure,
) -> Result<PipelineRun, StoreError> {
    fail_downstream_stage(
        conn,
        run_id,
        PipelineState::Analyzing,
        expected_version,
        PipelineStage::Analyze,
        failure,
    )
}

pub(super) fn start_synthesis(
    conn: &mut Connection,
    run_id: &str,
    expected_version: u32,
) -> Result<(PipelineRun, AnalyzedDocument), StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let analyzed = get_analyzed_document(&tx, run_id)?.ok_or_else(|| {
        StoreError::DownstreamArtifactNotFound {
            artifact_kind: "analyzed".to_string(),
            run_id: run_id.to_string(),
        }
    })?;
    let synthesizing_run = transition_in_tx(
        &tx,
        run_id,
        PipelineState::Analyzed,
        expected_version,
        PipelineState::Synthesizing,
        Some(PipelineStage::Synthesize),
        None,
        TransitionPatch::default(),
    )?;
    tx.commit()?;
    Ok((synthesizing_run, analyzed))
}

pub(super) fn complete_synthesis(
    conn: &mut Connection,
    run_id: &str,
    expected_version: u32,
    synthesized: &SynthesizedDocument,
    warnings: Vec<PipelineWarning>,
) -> Result<PipelineRun, StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    ensure_run_document_matches(&tx, run_id, "synthesized", &synthesized.document_id)?;
    insert_synthesized_document(&tx, run_id, synthesized)?;
    insert_synthesis_attempt(&tx, run_id, 0, synthesized)?;
    let synthesized_run = transition_in_tx(
        &tx,
        run_id,
        PipelineState::Synthesizing,
        expected_version,
        PipelineState::Synthesized,
        Some(PipelineStage::Synthesize),
        None,
        TransitionPatch {
            warnings: Some(warnings),
            failure: None,
            cancellation_requested: None,
        },
    )?;
    tx.commit()?;
    Ok(synthesized_run)
}

pub(super) fn fail_synthesis(
    conn: &mut Connection,
    run_id: &str,
    expected_version: u32,
    failure: PipelineFailure,
) -> Result<PipelineRun, StoreError> {
    fail_downstream_stage(
        conn,
        run_id,
        PipelineState::Synthesizing,
        expected_version,
        PipelineStage::Synthesize,
        failure,
    )
}

pub(super) fn start_verification(
    conn: &mut Connection,
    run_id: &str,
    expected_version: u32,
) -> Result<(PipelineRun, SynthesizedDocument), StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let synthesized = get_synthesized_document(&tx, run_id)?.ok_or_else(|| {
        StoreError::DownstreamArtifactNotFound {
            artifact_kind: "synthesized".to_string(),
            run_id: run_id.to_string(),
        }
    })?;
    let verifying_run = transition_in_tx(
        &tx,
        run_id,
        PipelineState::Synthesized,
        expected_version,
        PipelineState::Verifying,
        Some(PipelineStage::Verify),
        None,
        TransitionPatch::default(),
    )?;
    tx.commit()?;
    Ok((verifying_run, synthesized))
}

pub(super) fn complete_verification(
    conn: &mut Connection,
    run_id: &str,
    expected_version: u32,
    attempt_ordinal: u32,
    verified: &VerifiedDocument,
    warnings: Vec<PipelineWarning>,
) -> Result<PipelineRun, StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    ensure_run_document_matches(&tx, run_id, "verified", &verified.document_id)?;
    ensure_attempt_ordinal(attempt_ordinal, verified.synthesis_attempt_ordinal)?;
    insert_verification_attempt(&tx, run_id, attempt_ordinal, verified)?;
    insert_verified_document(&tx, run_id, verified)?;
    let verified_run = transition_in_tx(
        &tx,
        run_id,
        PipelineState::Verifying,
        expected_version,
        PipelineState::Verified,
        Some(PipelineStage::Verify),
        None,
        TransitionPatch {
            warnings: Some(warnings),
            failure: None,
            cancellation_requested: None,
        },
    )?;
    tx.commit()?;
    Ok(verified_run)
}

pub(super) fn record_verification_attempt(
    conn: &mut Connection,
    run_id: &str,
    expected_version: u32,
    attempt_ordinal: u32,
    verified: &VerifiedDocument,
) -> Result<(), StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    ensure_run_state_version(&tx, run_id, PipelineState::Verifying, expected_version)?;
    ensure_run_document_matches(&tx, run_id, "verification attempt", &verified.document_id)?;
    ensure_attempt_ordinal(attempt_ordinal, verified.synthesis_attempt_ordinal)?;
    insert_verification_attempt(&tx, run_id, attempt_ordinal, verified)?;
    tx.commit()?;
    Ok(())
}

pub(super) fn fail_verification(
    conn: &mut Connection,
    run_id: &str,
    expected_version: u32,
    failure: PipelineFailure,
) -> Result<PipelineRun, StoreError> {
    fail_downstream_stage(
        conn,
        run_id,
        PipelineState::Verifying,
        expected_version,
        PipelineStage::Verify,
        failure,
    )
}

pub(super) fn complete_summary(
    conn: &mut Connection,
    run_id: &str,
    expected_version: u32,
    summary: &SummaryArtifact,
    citations: &CitationArtifact,
) -> Result<PipelineRun, StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    ensure_run_document_matches(&tx, run_id, "summary", &summary.document_id)?;
    ensure_run_document_matches(&tx, run_id, "citation", &citations.document_id)?;
    if citations.summary_integrity_hash != summary.integrity_hash
        || citations.rendered_text != summary.text
    {
        return Err(StoreError::DownstreamArtifactMetadataMismatch {
            artifact_kind: "citation".to_string(),
            run_id: run_id.to_string(),
        });
    }
    insert_summary_artifact(&tx, run_id, summary)?;
    insert_citation_artifact(&tx, run_id, citations)?;
    let (next_state, reason) = if summary.warnings.is_empty() {
        (PipelineState::Complete, None)
    } else {
        let reason = if summary
            .warnings
            .iter()
            .any(|warning| warning.code == "SEMANTIC_VERIFICATION_DEFERRED")
        {
            "semantic_verification_deferred"
        } else if summary
            .warnings
            .iter()
            .any(|warning| warning.code == "SEMANTIC_CLAIMS_WITHHELD")
        {
            "semantic_claims_withheld"
        } else {
            "completed_with_warnings"
        };
        (
            PipelineState::CompleteWithWarnings,
            Some(reason.to_string()),
        )
    };
    let completed_run = transition_in_tx(
        &tx,
        run_id,
        PipelineState::Verified,
        expected_version,
        next_state,
        Some(PipelineStage::Verify),
        reason,
        TransitionPatch {
            warnings: Some(summary.warnings.clone()),
            failure: None,
            cancellation_requested: None,
        },
    )?;
    tx.commit()?;
    Ok(completed_run)
}

pub(super) fn fail_summary(
    conn: &mut Connection,
    run_id: &str,
    expected_version: u32,
    failure: PipelineFailure,
) -> Result<PipelineRun, StoreError> {
    fail_downstream_stage(
        conn,
        run_id,
        PipelineState::Verified,
        expected_version,
        PipelineStage::Verify,
        failure,
    )
}

pub(crate) fn request_cancellation(
    conn: &mut Connection,
    run_id: &str,
    expected_version: u32,
) -> Result<PipelineRun, StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let run = get_pipeline_run(&tx, run_id)?
        .ok_or_else(|| StoreError::RunNotFound(run_id.to_string()))?;
    if !run.state.can_request_cancellation() {
        return Err(StoreError::CancellationNotAllowed {
            run_id: run_id.to_string(),
            state: run.state,
        });
    }
    let stage = run.state.active_stage();
    let cancelling = transition_in_tx(
        &tx,
        run_id,
        run.state,
        expected_version,
        PipelineState::Cancelling,
        stage,
        Some("cancellation_requested".to_string()),
        TransitionPatch {
            warnings: None,
            failure: None,
            cancellation_requested: Some(true),
        },
    )?;
    tx.commit()?;
    Ok(cancelling)
}

pub(crate) fn complete_cancellation(
    conn: &mut Connection,
    run_id: &str,
    expected_version: u32,
) -> Result<PipelineRun, StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let run = get_pipeline_run(&tx, run_id)?
        .ok_or_else(|| StoreError::RunNotFound(run_id.to_string()))?;
    if run.state != PipelineState::Cancelling || !run.cancellation_requested {
        return Err(StoreError::CancellationNotAllowed {
            run_id: run_id.to_string(),
            state: run.state,
        });
    }
    let cancelled = transition_in_tx(
        &tx,
        run_id,
        PipelineState::Cancelling,
        expected_version,
        PipelineState::Cancelled,
        run.current_stage,
        Some("cancellation_completed".to_string()),
        TransitionPatch {
            warnings: None,
            failure: None,
            cancellation_requested: Some(true),
        },
    )?;
    tx.commit()?;
    Ok(cancelled)
}

pub(crate) fn fail_background_execution(
    conn: &mut Connection,
    run_id: &str,
    expected_state: PipelineState,
    expected_version: u32,
    failure: PipelineFailure,
) -> Result<PipelineRun, StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let persisted = get_pipeline_run(&tx, run_id)?
        .ok_or_else(|| StoreError::RunNotFound(run_id.to_string()))?;
    if persisted.state == PipelineState::Cancelling {
        if !persisted.cancellation_requested {
            return Err(StoreError::CancellationNotAllowed {
                run_id: run_id.to_string(),
                state: persisted.state,
            });
        }
        let cancelled = transition_in_tx(
            &tx,
            run_id,
            PipelineState::Cancelling,
            persisted.state_version,
            PipelineState::Cancelled,
            persisted.current_stage,
            Some("cancellation_completed".to_string()),
            TransitionPatch {
                warnings: None,
                failure: None,
                cancellation_requested: Some(true),
            },
        )?;
        tx.commit()?;
        return Ok(cancelled);
    }
    if !expected_state.can_request_cancellation() {
        return Err(StoreError::CancellationNotAllowed {
            run_id: run_id.to_string(),
            state: expected_state,
        });
    }
    let reason = Some(failure.code.clone());
    let stage = failure.stage.clone();
    let failed = transition_in_tx(
        &tx,
        run_id,
        expected_state,
        expected_version,
        PipelineState::Failed,
        stage,
        reason,
        TransitionPatch {
            warnings: None,
            failure: Some(failure),
            cancellation_requested: None,
        },
    )?;
    tx.commit()?;
    Ok(failed)
}

fn fail_downstream_stage(
    conn: &mut Connection,
    run_id: &str,
    expected_state: PipelineState,
    expected_version: u32,
    stage: PipelineStage,
    failure: PipelineFailure,
) -> Result<PipelineRun, StoreError> {
    let reason = Some(failure.code.clone());
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let failed_run = transition_in_tx(
        &tx,
        run_id,
        expected_state,
        expected_version,
        PipelineState::Failed,
        Some(stage),
        reason,
        TransitionPatch {
            warnings: None,
            failure: Some(failure),
            cancellation_requested: None,
        },
    )?;
    tx.commit()?;
    Ok(failed_run)
}

fn ensure_run_document_matches(
    conn: &Connection,
    run_id: &str,
    artifact_kind: &str,
    artifact_document_id: &str,
) -> Result<(), StoreError> {
    let run = get_pipeline_run(conn, run_id)?
        .ok_or_else(|| StoreError::RunNotFound(run_id.to_string()))?;
    if run.document_id != artifact_document_id {
        return Err(StoreError::DownstreamArtifactDocumentMismatch {
            artifact_kind: artifact_kind.to_string(),
            artifact_document_id: artifact_document_id.to_string(),
            run_document_id: run.document_id,
        });
    }
    Ok(())
}

fn ensure_run_state_version(
    conn: &Connection,
    run_id: &str,
    expected_state: PipelineState,
    expected_version: u32,
) -> Result<(), StoreError> {
    let run = get_pipeline_run(conn, run_id)?
        .ok_or_else(|| StoreError::RunNotFound(run_id.to_string()))?;
    if run.state != expected_state {
        return Err(TransitionError::StaleExpectedState {
            expected: expected_state,
            found: run.state,
        }
        .into());
    }
    if run.state_version != expected_version {
        return Err(TransitionError::ConcurrentModification {
            expected: expected_version,
            found: run.state_version,
        }
        .into());
    }
    Ok(())
}

fn ensure_attempt_ordinal(expected: u32, actual: u32) -> Result<(), StoreError> {
    if expected != actual {
        return Err(StoreError::AttemptOrdinalMismatch { expected, actual });
    }
    Ok(())
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

fn insert_chunked_document(
    conn: &Connection,
    run_id: &str,
    chunked: &ChunkedDocument,
) -> Result<(), StoreError> {
    let artifact_json = to_json(chunked)?;
    let artifact_hash = sha256_hex(artifact_json.as_bytes());
    conn.execute(
        "INSERT INTO chunked_documents (
            run_id, document_id, chunking_version, artifact_hash,
            chunked_artifact, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            run_id,
            chunked.document_id,
            chunked.chunking_version,
            artifact_hash,
            artifact_json,
            Utc::now().to_rfc3339(),
        ],
    )?;
    Ok(())
}

fn insert_downstream_artifact<T: Serialize>(
    conn: &Connection,
    run_id: &str,
    document_id: &str,
    version: &str,
    artifact: &T,
    table: DownstreamArtifactTable,
) -> Result<(), StoreError> {
    let DownstreamArtifactTable {
        table,
        version_column,
        artifact_column,
    } = table;
    let artifact_json = to_json(artifact)?;
    let artifact_hash = sha256_hex(artifact_json.as_bytes());
    let sql = format!(
        "INSERT INTO {table} (
            run_id, document_id, {version_column}, artifact_hash, {artifact_column}, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)"
    );
    conn.execute(
        &sql,
        params![
            run_id,
            document_id,
            version,
            artifact_hash,
            artifact_json,
            Utc::now().to_rfc3339(),
        ],
    )?;
    Ok(())
}

fn insert_analyzed_document(
    conn: &Connection,
    run_id: &str,
    analyzed: &AnalyzedDocument,
) -> Result<(), StoreError> {
    insert_downstream_artifact(
        conn,
        run_id,
        &analyzed.document_id,
        &analyzed.analysis_version,
        analyzed,
        ANALYZED_ARTIFACT_TABLE,
    )
}

fn insert_synthesized_document(
    conn: &Connection,
    run_id: &str,
    synthesized: &SynthesizedDocument,
) -> Result<(), StoreError> {
    insert_downstream_artifact(
        conn,
        run_id,
        &synthesized.document_id,
        &synthesized.synthesis_version,
        synthesized,
        SYNTHESIZED_ARTIFACT_TABLE,
    )
}

fn insert_synthesis_attempt(
    conn: &Connection,
    run_id: &str,
    attempt_ordinal: u32,
    synthesized: &SynthesizedDocument,
) -> Result<(), StoreError> {
    let artifact_json = to_json(synthesized)?;
    let artifact_hash = sha256_hex(artifact_json.as_bytes());
    conn.execute(
        "INSERT INTO summary_synthesis_attempts (
            run_id, attempt_ordinal, document_id, synthesis_version, artifact_hash,
            synthesized_artifact, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            run_id,
            attempt_ordinal,
            synthesized.document_id,
            synthesized.synthesis_version,
            artifact_hash,
            artifact_json,
            Utc::now().to_rfc3339(),
        ],
    )?;
    Ok(())
}

fn insert_verified_document(
    conn: &Connection,
    run_id: &str,
    verified: &VerifiedDocument,
) -> Result<(), StoreError> {
    insert_downstream_artifact(
        conn,
        run_id,
        &verified.document_id,
        &verified.verification_version,
        verified,
        VERIFIED_ARTIFACT_TABLE,
    )
}

fn insert_verification_attempt(
    conn: &Connection,
    run_id: &str,
    attempt_ordinal: u32,
    verified: &VerifiedDocument,
) -> Result<(), StoreError> {
    let artifact_json = to_json(verified)?;
    let artifact_hash = sha256_hex(artifact_json.as_bytes());
    conn.execute(
        "INSERT INTO summary_verification_attempts (
            run_id, attempt_ordinal, document_id, verification_version, artifact_hash,
            verified_artifact, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            run_id,
            attempt_ordinal,
            verified.document_id,
            verified.verification_version,
            artifact_hash,
            artifact_json,
            Utc::now().to_rfc3339(),
        ],
    )?;
    Ok(())
}

fn insert_summary_artifact(
    conn: &Connection,
    run_id: &str,
    summary: &SummaryArtifact,
) -> Result<(), StoreError> {
    if summary.calculate_integrity_hash()? != summary.integrity_hash {
        return Err(StoreError::DownstreamArtifactIntegrityMismatch {
            artifact_kind: "summary".to_string(),
            run_id: run_id.to_string(),
        });
    }
    insert_downstream_artifact(
        conn,
        run_id,
        &summary.document_id,
        &summary.summary_version,
        summary,
        SUMMARY_ARTIFACT_TABLE,
    )
}

fn insert_citation_artifact(
    conn: &Connection,
    run_id: &str,
    citations: &CitationArtifact,
) -> Result<(), StoreError> {
    if citations.calculate_integrity_hash()? != citations.integrity_hash {
        return Err(StoreError::DownstreamArtifactIntegrityMismatch {
            artifact_kind: "citation".to_string(),
            run_id: run_id.to_string(),
        });
    }
    let artifact_json = to_json(citations)?;
    let artifact_hash = sha256_hex(artifact_json.as_bytes());
    conn.execute(
        "INSERT INTO citation_artifacts (
            run_id, document_id, citation_version, summary_integrity_hash,
            artifact_hash, citation_artifact, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            run_id,
            citations.document_id,
            citations.citation_version,
            citations.summary_integrity_hash,
            artifact_hash,
            artifact_json,
            citations.created_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

pub(super) fn reconcile_interrupted_runs_in_transaction(
    conn: &mut Connection,
    interrupted: &[InterruptedRunTransition],
) -> Result<Vec<PipelineRun>, StoreError> {
    if interrupted.is_empty() {
        return Ok(Vec::new());
    }

    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut recovered = Vec::with_capacity(interrupted.len());
    for candidate in interrupted {
        let recovered_run = match &candidate.action {
            InterruptedRunAction::Fail(failure) => {
                let Some(stage) = candidate.expected_state.active_stage() else {
                    return Err(StoreError::InvalidRecoveryTransition {
                        state: candidate.expected_state.clone(),
                    });
                };
                if failure.code != "PROCESS_INTERRUPTED"
                    || failure.stage.as_ref() != Some(&stage)
                    || !failure.recoverable
                {
                    return Err(StoreError::InvalidRecoveryTransition {
                        state: candidate.expected_state.clone(),
                    });
                }
                transition_in_tx(
                    &tx,
                    &candidate.run_id,
                    candidate.expected_state.clone(),
                    candidate.expected_version,
                    PipelineState::Failed,
                    Some(stage),
                    Some(failure.code.clone()),
                    TransitionPatch {
                        warnings: None,
                        failure: Some(failure.clone()),
                        cancellation_requested: None,
                    },
                )?
            }
            InterruptedRunAction::CompleteCancellation => {
                if candidate.expected_state != PipelineState::Cancelling {
                    return Err(StoreError::InvalidRecoveryTransition {
                        state: candidate.expected_state.clone(),
                    });
                }
                let persisted = get_pipeline_run(&tx, &candidate.run_id)?
                    .ok_or_else(|| StoreError::RunNotFound(candidate.run_id.clone()))?;
                if persisted.state == candidate.expected_state
                    && persisted.state_version == candidate.expected_version
                    && !persisted.cancellation_requested
                {
                    return Err(StoreError::InvalidRecoveryTransition {
                        state: candidate.expected_state.clone(),
                    });
                }
                transition_in_tx(
                    &tx,
                    &candidate.run_id,
                    PipelineState::Cancelling,
                    candidate.expected_version,
                    PipelineState::Cancelled,
                    None,
                    Some("cancellation_completed_after_restart".to_string()),
                    TransitionPatch {
                        warnings: None,
                        failure: None,
                        cancellation_requested: Some(true),
                    },
                )?
            }
        };
        recovered.push(recovered_run);
    }
    tx.commit()?;
    Ok(recovered)
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
    if let Some(cancellation_requested) = patch.cancellation_requested {
        run.cancellation_requested = cancellation_requested;
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
    use crate::pipeline::ingest::{ingest_pdf, ingest_pdf_with_profiles};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{Arc, Barrier};
    use std::thread;

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

    fn model_profile_snapshot() -> ModelProfileSnapshot {
        ModelProfileSnapshot {
            version: 1,
            preset_id: "full-test-v1".to_string(),
            analysis: crate::pipeline::contracts::ModelStageProfileSnapshot {
                runtime_kind: Default::default(),
                profile_id: "analysis-v1".to_string(),
                model_name: "analysis:latest".to_string(),
                model_digest: "analysis-digest".to_string(),
                context_tokens: 8_192,
                tokenizer_version: "qwen-test-v1".to_string(),
            },
            verification: crate::pipeline::contracts::ModelStageProfileSnapshot {
                runtime_kind: Default::default(),
                profile_id: "verification-v1".to_string(),
                model_name: "verification:latest".to_string(),
                model_digest: "verification-digest".to_string(),
                context_tokens: 16_384,
                tokenizer_version: "qwen-test-v1".to_string(),
            },
        }
    }

    #[test]
    fn profile_suggestion_request_owners_converge_across_reopen_and_concurrency() {
        let database = TestFile::empty("db");
        let conn = init_db(&database.0).expect("schema should initialize");
        drop(conn);
        let barrier = Arc::new(Barrier::new(2));
        let handles = (0..2)
            .map(|_| {
                let path = database.0.clone();
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    let mut conn = init_db(path).expect("database should open");
                    barrier.wait();
                    get_or_create_profile_suggestion_owner(
                        &mut conn,
                        &"a".repeat(64),
                        "document.summary.profile-suggestion@1",
                    )
                    .expect("owner should persist")
                })
            })
            .collect::<Vec<_>>();
        let owners = handles
            .into_iter()
            .map(|handle| handle.join().expect("worker should finish"))
            .collect::<Vec<_>>();
        assert_eq!(owners[0], owners[1]);

        let mut reopened = init_db(&database.0).expect("database should reopen");
        assert_eq!(
            get_or_create_profile_suggestion_owner(
                &mut reopened,
                &"a".repeat(64),
                "document.summary.profile-suggestion@1",
            )
            .unwrap(),
            owners[0]
        );
        let changed_content = get_or_create_profile_suggestion_owner(
            &mut reopened,
            &"b".repeat(64),
            "document.summary.profile-suggestion@1",
        )
        .unwrap();
        let changed_contract = get_or_create_profile_suggestion_owner(
            &mut reopened,
            &"a".repeat(64),
            "document.summary.profile-suggestion@2",
        )
        .unwrap();
        assert_ne!(changed_content, owners[0]);
        assert_ne!(changed_contract, owners[0]);
        assert_ne!(changed_content, changed_contract);
        assert_eq!(
            reopened
                .query_row(
                    "SELECT COUNT(*) FROM model_request_owners
                     WHERE owner_kind = 'profile_suggestion'",
                    [],
                    |row| row.get::<_, u32>(0),
                )
                .unwrap(),
            3
        );
    }

    #[test]
    fn profile_suggestion_request_owner_boundary_fails_closed() {
        let mut conn = init_db(":memory:").expect("schema should initialize");
        for (hash, version) in [
            ("a".repeat(63), "contract@1".to_string()),
            ("a".repeat(65), "contract@1".to_string()),
            ("A".repeat(64), "contract@1".to_string()),
            ("a".repeat(64), String::new()),
            ("a".repeat(64), "contract/1".to_string()),
            ("a".repeat(64), "x".repeat(129)),
        ] {
            assert!(matches!(
                get_or_create_profile_suggestion_owner(&mut conn, &hash, &version),
                Err(StoreError::InvalidRequestOwner(_))
            ));
        }
        get_or_create_profile_suggestion_owner(&mut conn, &"e".repeat(64), &"x".repeat(128))
            .expect("maximum task contract length should persist");

        let source = TestFile::new("pdf", b"%PDF-1.4\nREQUEST_OWNER_KIND");
        let (_, run) = ingest_pdf(&mut conn, source.0.to_str().expect("UTF-8 path"))
            .expect("candidate should ingest");
        assert_eq!(
            conn.query_row(
                "SELECT owner_kind FROM model_request_owners WHERE owner_id = ?1",
                [&run.run_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "pipeline_run"
        );
        assert!(conn
            .execute(
                "INSERT INTO profile_suggestion_requests (
                    source_content_hash, task_contract_version, owner_id, created_at
                 ) VALUES (?1, 'contract@1', ?2, '2026-09-11T18:00:00Z')",
                params!["c".repeat(64), run.run_id],
            )
            .is_err());
        assert!(conn
            .execute(
                "INSERT INTO profile_suggestion_requests (
                    source_content_hash, task_contract_version, owner_id, created_at
                 ) VALUES (
                    ?1, 'contract@1', '12345678-1234-4234-8234-123456789abc',
                    '2026-09-11T18:00:00Z'
                 )",
                ["d".repeat(64)],
            )
            .is_err());
    }

    #[test]
    fn run_model_profile_is_insert_once_and_must_match_on_continuation() {
        let source = TestFile::new("pdf", b"%PDF-1.4\nMODEL_PROFILE_SNAPSHOT");
        let mut conn = init_db(":memory:").expect("schema should initialize");
        let (_, run) = ingest_pdf(&mut conn, source.0.to_str().expect("UTF-8 path"))
            .expect("candidate should ingest");
        let snapshot = model_profile_snapshot();
        ensure_run_model_profile(&conn, &run.run_id, Some(&snapshot))
            .expect("first profile should persist");
        assert_eq!(
            get_run_model_profile(&conn, &run.run_id).expect("profile should load"),
            Some(snapshot.clone())
        );
        ensure_run_model_profile(&conn, &run.run_id, Some(&snapshot))
            .expect("the identical continuation profile should pass");

        let mut changed = snapshot;
        changed.analysis.context_tokens += 1;
        assert!(matches!(
            ensure_run_model_profile(&conn, &run.run_id, Some(&changed)),
            Err(StoreError::ModelProfileMismatch { .. })
        ));
        assert!(conn
            .execute(
                "UPDATE pipeline_run_model_profiles SET profile_snapshot = '{}' WHERE run_id = ?1",
                [&run.run_id],
            )
            .is_err());
    }

    #[test]
    fn run_summary_profile_is_explicit_immutable_and_rejects_unknown_values() {
        let source = TestFile::new("pdf", b"%PDF-1.4\nSUMMARY_PROFILE");
        let mut conn = init_db(":memory:").expect("schema should initialize");
        let (_, run) = ingest_pdf(&mut conn, source.0.to_str().expect("UTF-8 path"))
            .expect("candidate should ingest");

        assert_eq!(
            get_run_summary_profile(&conn, &run.run_id).expect("summary profile should load"),
            Some(SummaryProfile::General)
        );
        ensure_run_summary_profile(&conn, &run.run_id, SummaryProfile::General)
            .expect("the identical summary profile should pass");
        assert!(matches!(
            ensure_run_summary_profile(&conn, &run.run_id, SummaryProfile::Story),
            Err(StoreError::SummaryProfileMismatch { .. })
        ));
        assert!(conn
            .execute(
                "UPDATE pipeline_run_summary_profiles SET summary_profile = '\"story\"'
                 WHERE run_id = ?1",
                [&run.run_id],
            )
            .is_err());
        assert!(serde_json::from_str::<SummaryProfile>("\"automatic\"").is_err());
        assert_eq!(
            serde_json::from_str::<SummaryProfile>("\"general\"")
                .expect("General should be a valid explicit profile"),
            SummaryProfile::General
        );
        assert_eq!(
            serde_json::from_str::<SummaryProfile>("\"story\"")
                .expect("Story should be a valid explicit profile"),
            SummaryProfile::Story
        );
        assert_eq!(
            serde_json::from_str::<SummaryProfile>("\"contract\"")
                .expect("Contract should be a valid explicit profile"),
            SummaryProfile::Contract
        );

        let story_source = TestFile::new("pdf", b"%PDF-1.4\nSTORY_SUMMARY_PROFILE");
        let (_, story_run) = ingest_pdf_with_profiles(
            &mut conn,
            story_source.0.to_str().expect("UTF-8 path"),
            None,
            SummaryProfile::Story,
            None,
        )
        .expect("Story candidate should ingest");
        assert_eq!(
            get_run_summary_profile(&conn, &story_run.run_id)
                .expect("Story summary profile should load"),
            Some(SummaryProfile::Story)
        );
        ensure_run_summary_profile(&conn, &story_run.run_id, SummaryProfile::Story)
            .expect("the identical Story profile should pass");
        assert!(matches!(
            ensure_run_summary_profile(&conn, &story_run.run_id, SummaryProfile::General),
            Err(StoreError::SummaryProfileMismatch { .. })
        ));

        let contract_source = TestFile::new("pdf", b"%PDF-1.4\nCONTRACT_SUMMARY_PROFILE");
        let (_, contract_run) = ingest_pdf_with_profiles(
            &mut conn,
            contract_source.0.to_str().expect("UTF-8 path"),
            None,
            SummaryProfile::Contract,
            None,
        )
        .expect("Contract candidate should ingest");
        assert_eq!(
            get_run_summary_profile(&conn, &contract_run.run_id)
                .expect("Contract summary profile should load"),
            Some(SummaryProfile::Contract)
        );
        ensure_run_summary_profile(&conn, &contract_run.run_id, SummaryProfile::Contract)
            .expect("the identical Contract profile should pass");
        assert!(matches!(
            ensure_run_summary_profile(&conn, &contract_run.run_id, SummaryProfile::Story),
            Err(StoreError::SummaryProfileMismatch { .. })
        ));
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
    fn cancellation_state_flag_version_and_events_commit_atomically() {
        let source = TestFile::new("pdf", b"%PDF-1.4\nCANCELLATION_ATOMICITY_BODY");
        let mut conn = init_db(":memory:").expect("schema should initialize");
        let (_, ingested) = ingest_pdf(&mut conn, source.0.to_str().expect("UTF-8 path"))
            .expect("candidate should ingest");
        let (parsing, _) = start_parsing(&mut conn, &ingested.run_id, ingested.state_version)
            .expect("parsing should start");
        let events_before =
            list_pipeline_events(&conn, &parsing.run_id).expect("events should load");

        let stale = request_cancellation(
            &mut conn,
            &parsing.run_id,
            parsing.state_version.saturating_sub(1),
        );
        assert!(matches!(
            stale,
            Err(StoreError::Transition(
                TransitionError::ConcurrentModification { .. }
            ))
        ));
        assert_eq!(
            get_pipeline_run(&conn, &parsing.run_id)
                .expect("run should reload")
                .expect("run should exist"),
            parsing
        );

        conn.execute_batch(
            "CREATE TRIGGER test_fail_cancellation_event
             BEFORE INSERT ON pipeline_events
             WHEN NEW.next_state = '\"Cancelling\"'
             BEGIN
                 SELECT RAISE(ABORT, 'injected cancellation event failure');
             END;",
        )
        .expect("failure trigger should install");
        let injected = request_cancellation(&mut conn, &parsing.run_id, parsing.state_version);
        assert!(matches!(injected, Err(StoreError::Sqlite(_))));
        let after_rollback = get_pipeline_run(&conn, &parsing.run_id)
            .expect("run should reload")
            .expect("run should exist");
        assert_eq!(after_rollback, parsing);
        assert!(!after_rollback.cancellation_requested);
        assert_eq!(
            list_pipeline_events(&conn, &parsing.run_id).expect("events should reload"),
            events_before
        );

        conn.execute_batch("DROP TRIGGER test_fail_cancellation_event;")
            .expect("failure trigger should drop");
        let cancelling = request_cancellation(&mut conn, &parsing.run_id, parsing.state_version)
            .expect("cancellation should commit");
        assert_eq!(cancelling.state, PipelineState::Cancelling);
        assert_eq!(cancelling.state_version, parsing.state_version + 1);
        assert!(cancelling.cancellation_requested);
        assert!(cancelling.completed_at.is_none());

        let cancelled =
            complete_cancellation(&mut conn, &cancelling.run_id, cancelling.state_version)
                .expect("cancellation should complete");
        assert_eq!(cancelled.state, PipelineState::Cancelled);
        assert_eq!(cancelled.state_version, parsing.state_version + 2);
        assert!(cancelled.cancellation_requested);
        assert!(cancelled.completed_at.is_some());
        assert!(matches!(
            request_cancellation(&mut conn, &cancelled.run_id, cancelled.state_version),
            Err(StoreError::CancellationNotAllowed {
                state: PipelineState::Cancelled,
                ..
            })
        ));

        let events = list_pipeline_events(&conn, &cancelled.run_id)
            .expect("cancellation events should load");
        assert_eq!(events.len(), events_before.len() + 2);
        assert_eq!(
            events[events.len() - 2].next_state,
            PipelineState::Cancelling
        );
        assert_eq!(
            events[events.len() - 1].next_state,
            PipelineState::Cancelled
        );
        assert!(!serde_json::to_string(&events)
            .expect("events should serialize")
            .contains("CANCELLATION_ATOMICITY_BODY"));
    }

    #[test]
    fn cancellation_wins_over_stale_background_failure_atomically() {
        let source = TestFile::new("pdf", b"%PDF-1.4\nCANCEL_FAILURE_RACE_BODY");
        let mut conn = init_db(":memory:").expect("schema should initialize");
        let (_, ingested) = ingest_pdf(&mut conn, source.0.to_str().expect("UTF-8 path"))
            .expect("candidate should ingest");
        let (parsing, _) = start_parsing(&mut conn, &ingested.run_id, ingested.state_version)
            .expect("parsing should start");
        let cancelling = request_cancellation(&mut conn, &parsing.run_id, parsing.state_version)
            .expect("cancellation should win first");

        let settled = fail_background_execution(
            &mut conn,
            &parsing.run_id,
            parsing.state,
            parsing.state_version,
            PipelineFailure {
                code: "BACKGROUND_WORKER_PANIC".to_string(),
                message: "stale worker failure".to_string(),
                stage: Some(PipelineStage::Parse),
                recoverable: true,
            },
        )
        .expect("failure finalization should acknowledge the winning cancellation");

        assert_eq!(settled.state, PipelineState::Cancelled);
        assert_eq!(settled.state_version, cancelling.state_version + 1);
        assert!(settled.cancellation_requested);
        assert!(settled.failure.is_none());
        let events =
            list_pipeline_events(&conn, &settled.run_id).expect("cancellation history should load");
        assert_eq!(
            events[events.len() - 2].next_state,
            PipelineState::Cancelling
        );
        assert_eq!(
            events[events.len() - 1].next_state,
            PipelineState::Cancelled
        );
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
            assert_eq!(
                schema_version(&migrated).expect("version should load"),
                schema::CURRENT_SCHEMA_VERSION
            );
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
        assert_eq!(
            schema_version(&reopened).expect("version should load"),
            schema::CURRENT_SCHEMA_VERSION
        );
        assert_eq!(
            list_pipeline_events(&reopened, "legacy-run")
                .expect("events should load")
                .len(),
            2
        );
    }
}
