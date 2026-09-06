use crate::connect::contracts::{
    ArtifactProvenance, CapabilityRef, InputArtifact, JobError, JobRequest, JobResult, JobState,
    JobStatus, ProviderRef, CAPABILITY_ID, CAPABILITY_VERSION, PROTOCOL_VERSION,
};
use crate::connect::v2;
use crate::pipeline::contracts::{IngestedDocument, ModelProfileSnapshot, PipelineRun};
use crate::pipeline::db::{self, StoreError};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::num::TryFromIntError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConnectStoreError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    PipelineStore(#[from] StoreError),
    #[error("Connect artifact byte size exceeds SQLite integer range")]
    SizeRange(#[from] TryFromIntError),
    #[error("Invalid persisted Connect timestamp: {0}")]
    InvalidTimestamp(#[from] chrono::ParseError),
    #[error("Connect job state changed concurrently: {0}")]
    StaleJob(String),
    #[error("Connect job has inconsistent persisted state: {0}")]
    InvalidJob(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredConnectJob {
    pub job_id: String,
    pub request_hash: String,
    pub protocol_version: u32,
    pub input: InputArtifact,
    pub import_path: String,
    pub pipeline_run_id: String,
    pub provider_instance_id: String,
    pub state: JobState,
    pub result: Option<JobResult>,
    pub error: Option<JobError>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl StoredConnectJob {
    pub fn status(&self) -> JobStatus {
        JobStatus {
            protocol_version: PROTOCOL_VERSION,
            job_id: self.job_id.clone(),
            capability: CapabilityRef {
                id: CAPABILITY_ID.to_string(),
                version: CAPABILITY_VERSION.to_string(),
            },
            provider: ProviderRef {
                app_id: crate::connect::contracts::APP_ID.to_string(),
                instance_id: self.provider_instance_id.clone(),
            },
            status: self.state.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
            input_artifacts: vec![ArtifactProvenance::from_input(&self.input)],
            result: self.result.clone(),
            error: self.error.clone(),
        }
    }
}

pub fn accept_job_with_ingestion(
    conn: &mut Connection,
    request: &JobRequest,
    request_hash: &str,
    import_path: &str,
    provider_instance_id: &str,
    document: &IngestedDocument,
    run: &PipelineRun,
) -> Result<(PipelineRun, StoredConnectJob), ConnectStoreError> {
    accept_job_with_ingestion_guarded(
        conn,
        request,
        request_hash,
        import_path,
        provider_instance_id,
        document,
        run,
        None,
        || true,
    )?
    .ok_or_else(|| {
        ConnectStoreError::InvalidJob(
            "unconditional Connect job admission was rejected".to_string(),
        )
    })
}

#[allow(clippy::too_many_arguments)]
pub fn accept_job_with_ingestion_guarded<F>(
    conn: &mut Connection,
    request: &JobRequest,
    request_hash: &str,
    import_path: &str,
    provider_instance_id: &str,
    document: &IngestedDocument,
    run: &PipelineRun,
    profile_snapshot: Option<&ModelProfileSnapshot>,
    mut admission_check: F,
) -> Result<Option<(PipelineRun, StoredConnectJob)>, ConnectStoreError>
where
    F: FnMut() -> bool,
{
    let input = &request.inputs[0];
    let byte_size = i64::try_from(input.byte_size)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if !admission_check() {
        return Ok(None);
    }
    let now = Utc::now();
    let ingested_run = db::persist_ingestion_in_transaction(&tx, document, run)?;
    db::ensure_run_model_profile(&tx, &ingested_run.run_id, profile_snapshot)?;
    tx.execute(
        "INSERT INTO connect_jobs (
            job_id, request_hash, protocol_version, capability_id, capability_version,
            input_artifact_id, input_media_type, input_byte_size, input_sha256,
            input_display_name, source_app_id, import_path, pipeline_run_id,
            provider_instance_id, status, result_json, error_json, created_at, updated_at
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
            'accepted', NULL, NULL, ?15, ?15
         )",
        params![
            request.job_id,
            request_hash,
            request.protocol_version,
            request.capability.id,
            request.capability.version,
            input.artifact_id,
            input.media_type,
            byte_size,
            input.sha256,
            input.display_name,
            input.source_app_id,
            import_path,
            ingested_run.run_id,
            provider_instance_id,
            now.to_rfc3339(),
        ],
    )?;
    if !admission_check() {
        return Ok(None);
    }
    tx.commit()?;
    let stored = get_job(conn, &request.job_id)?.ok_or_else(|| {
        ConnectStoreError::InvalidJob("accepted job was not readable after commit".to_string())
    })?;
    Ok(Some((ingested_run, stored)))
}

pub fn get_job(
    conn: &Connection,
    job_id: &str,
) -> Result<Option<StoredConnectJob>, ConnectStoreError> {
    let row = conn
        .query_row(
            "SELECT job_id, request_hash, protocol_version, input_artifact_id, input_media_type,
                    input_byte_size, input_sha256, input_display_name, source_app_id,
                    import_path, pipeline_run_id, provider_instance_id, status,
                    result_json, error_json, created_at, updated_at
             FROM connect_jobs WHERE job_id = ?1",
            [job_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u32>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, String>(11)?,
                    row.get::<_, String>(12)?,
                    row.get::<_, Option<String>>(13)?,
                    row.get::<_, Option<String>>(14)?,
                    row.get::<_, String>(15)?,
                    row.get::<_, String>(16)?,
                ))
            },
        )
        .optional()?;

    row.map(
        |(
            job_id,
            request_hash,
            protocol_version,
            artifact_id,
            media_type,
            byte_size,
            sha256,
            display_name,
            source_app_id,
            import_path,
            pipeline_run_id,
            provider_instance_id,
            state,
            result_json,
            error_json,
            created_at,
            updated_at,
        )| {
            let state = parse_state(&state)?;
            let result = result_json
                .as_deref()
                .map(serde_json::from_str)
                .transpose()?;
            let error = error_json
                .as_deref()
                .map(serde_json::from_str)
                .transpose()?;
            validate_state_payload(&job_id, &state, &result, &error)?;
            let byte_size = u64::try_from(byte_size).map_err(|_| {
                ConnectStoreError::InvalidJob(format!(
                    "job {job_id} has a negative input byte size"
                ))
            })?;
            Ok(StoredConnectJob {
                job_id,
                request_hash,
                protocol_version,
                input: InputArtifact {
                    artifact_id,
                    media_type,
                    byte_size,
                    sha256,
                    display_name,
                    source_app_id,
                },
                import_path,
                pipeline_run_id,
                provider_instance_id,
                state,
                result,
                error,
                created_at: DateTime::parse_from_rfc3339(&created_at)?.with_timezone(&Utc),
                updated_at: DateTime::parse_from_rfc3339(&updated_at)?.with_timezone(&Utc),
            })
        },
    )
    .transpose()
}

pub fn has_active_job(conn: &Connection) -> Result<bool, ConnectStoreError> {
    let active: bool = conn.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM connect_jobs WHERE status IN ('accepted', 'processing')
         )",
        [],
        |row| row.get(0),
    )?;
    Ok(active)
}

pub fn active_v2_provider_instance_id(
    conn: &Connection,
) -> Result<Option<String>, ConnectStoreError> {
    let mut statement = conn.prepare(
        "SELECT provider_instance_id
         FROM connect_jobs
         WHERE protocol_version = ?1 AND status IN ('accepted', 'processing')
         GROUP BY provider_instance_id
         ORDER BY provider_instance_id
         LIMIT 2",
    )?;
    let mut rows = statement.query([v2::PROTOCOL_VERSION])?;
    let first = match rows.next()? {
        Some(row) => Some(row.get(0)?),
        None => None,
    };
    if rows.next()?.is_some() {
        return Err(ConnectStoreError::InvalidJob(
            "active Connect v2 jobs have inconsistent provider identities".to_string(),
        ));
    }
    Ok(first)
}

pub fn mark_processing(
    conn: &Connection,
    job_id: &str,
) -> Result<StoredConnectJob, ConnectStoreError> {
    update_state(conn, job_id, "accepted", "processing", None, None)
}

pub fn mark_completed(
    conn: &Connection,
    job_id: &str,
    result: &JobResult,
) -> Result<StoredConnectJob, ConnectStoreError> {
    update_state(
        conn,
        job_id,
        "processing",
        "completed",
        Some(serde_json::to_string(result)?),
        None,
    )
}

pub fn mark_failed(
    conn: &Connection,
    job_id: &str,
    error: &JobError,
) -> Result<StoredConnectJob, ConnectStoreError> {
    let error_json = serde_json::to_string(error)?;
    let now = Utc::now().to_rfc3339();
    let changed = conn.execute(
        "UPDATE connect_jobs
         SET status = 'failed', result_json = NULL, error_json = ?1, updated_at = ?2
         WHERE job_id = ?3 AND status IN ('accepted', 'processing')",
        params![error_json, now, job_id],
    )?;
    if changed != 1 {
        return Err(ConnectStoreError::StaleJob(job_id.to_string()));
    }
    get_job(conn, job_id)?.ok_or_else(|| ConnectStoreError::InvalidJob(job_id.to_string()))
}

pub fn mark_interrupted_jobs_failed(
    conn: &Connection,
    error: &JobError,
) -> Result<usize, ConnectStoreError> {
    let changed = conn.execute(
        "UPDATE connect_jobs
         SET status = 'failed', result_json = NULL, error_json = ?1, updated_at = ?2
         WHERE status IN ('accepted', 'processing')",
        params![serde_json::to_string(error)?, Utc::now().to_rfc3339()],
    )?;
    Ok(changed)
}

fn update_state(
    conn: &Connection,
    job_id: &str,
    expected_state: &str,
    next_state: &str,
    result_json: Option<String>,
    error_json: Option<String>,
) -> Result<StoredConnectJob, ConnectStoreError> {
    let changed = conn.execute(
        "UPDATE connect_jobs
         SET status = ?1, result_json = ?2, error_json = ?3, updated_at = ?4
         WHERE job_id = ?5 AND status = ?6",
        params![
            next_state,
            result_json,
            error_json,
            Utc::now().to_rfc3339(),
            job_id,
            expected_state,
        ],
    )?;
    if changed != 1 {
        return Err(ConnectStoreError::StaleJob(job_id.to_string()));
    }
    get_job(conn, job_id)?.ok_or_else(|| ConnectStoreError::InvalidJob(job_id.to_string()))
}

fn parse_state(value: &str) -> Result<JobState, ConnectStoreError> {
    match value {
        "accepted" => Ok(JobState::Accepted),
        "processing" => Ok(JobState::Processing),
        "completed" => Ok(JobState::Completed),
        "failed" => Ok(JobState::Failed),
        _ => Err(ConnectStoreError::InvalidJob(format!(
            "unknown job state {value}"
        ))),
    }
}

fn validate_state_payload(
    job_id: &str,
    state: &JobState,
    result: &Option<JobResult>,
    error: &Option<JobError>,
) -> Result<(), ConnectStoreError> {
    let valid = match state {
        JobState::Accepted | JobState::Processing => result.is_none() && error.is_none(),
        JobState::Completed => result.is_some() && error.is_none(),
        JobState::Failed => result.is_none() && error.is_some(),
    };
    if valid {
        Ok(())
    } else {
        Err(ConnectStoreError::InvalidJob(format!(
            "job {job_id} state payload does not match status"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connect::contracts::{job_error, CAPABILITY_ID, CAPABILITY_VERSION};
    use crate::pipeline::contracts::ModelStageProfileSnapshot;
    use crate::pipeline::ingest::prepare_pdf_ingestion;
    use sha2::{Digest, Sha256};
    use std::fs;
    use std::path::PathBuf;
    use uuid::Uuid;

    struct TestFile(PathBuf);

    impl TestFile {
        fn pdf() -> Self {
            let path = std::env::temp_dir().join(format!("connect-store-{}.pdf", Uuid::new_v4()));
            fs::write(&path, b"%PDF-1.4\nconnect store fixture")
                .expect("fixture should be writable");
            Self(path)
        }
    }

    impl Drop for TestFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    fn request(bytes: &[u8]) -> JobRequest {
        JobRequest {
            protocol_version: PROTOCOL_VERSION,
            job_id: Uuid::new_v4().to_string(),
            capability: CapabilityRef {
                id: CAPABILITY_ID.to_string(),
                version: CAPABILITY_VERSION.to_string(),
            },
            inputs: vec![InputArtifact {
                artifact_id: Uuid::new_v4().to_string(),
                media_type: "application/pdf".to_string(),
                byte_size: bytes.len() as u64,
                sha256: format!("{:x}", Sha256::digest(bytes)),
                display_name: "received.pdf".to_string(),
                source_app_id: "fixture-app".to_string(),
            }],
        }
    }

    fn profile_snapshot() -> ModelProfileSnapshot {
        let stage = ModelStageProfileSnapshot {
            profile_id: "connect-store-profile".to_string(),
            model_name: "connect-store-model".to_string(),
            model_digest: "connect-store-digest".to_string(),
            context_tokens: 8_192,
            tokenizer_version: "connect-store-tokenizer".to_string(),
        };
        ModelProfileSnapshot {
            version: 1,
            preset_id: "connect-store-preset".to_string(),
            analysis: stage.clone(),
            verification: stage,
        }
    }

    #[test]
    fn accepted_job_and_ingestion_commit_atomically_and_enforce_one_active_job() {
        let file = TestFile::pdf();
        let bytes = fs::read(&file.0).expect("fixture should read");
        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let first = request(&bytes);
        let (document, run) = prepare_pdf_ingestion(
            file.0.to_str().expect("path should be UTF-8"),
            Some(&first.inputs[0].display_name),
        )
        .expect("ingestion should prepare");
        let snapshot = profile_snapshot();
        let (_, stored) = accept_job_with_ingestion_guarded(
            &mut conn,
            &first,
            &first.canonical_hash().unwrap(),
            file.0.to_str().unwrap(),
            &Uuid::new_v4().to_string(),
            &document,
            &run,
            Some(&snapshot),
            || true,
        )
        .expect("first guarded admission should succeed")
        .expect("active entitlement should admit the job");
        assert_eq!(stored.protocol_version, PROTOCOL_VERSION);
        assert_eq!(
            db::get_run_model_profile(&conn, &run.run_id).unwrap(),
            Some(snapshot.clone())
        );

        let second = request(&bytes);
        let (second_document, second_run) = prepare_pdf_ingestion(
            file.0.to_str().unwrap(),
            Some(&second.inputs[0].display_name),
        )
        .unwrap();
        assert!(accept_job_with_ingestion_guarded(
            &mut conn,
            &second,
            &second.canonical_hash().unwrap(),
            file.0.to_str().unwrap(),
            &Uuid::new_v4().to_string(),
            &second_document,
            &second_run,
            Some(&snapshot),
            || true,
        )
        .is_err());
        assert!(db::get_pipeline_run(&conn, &second_run.run_id)
            .expect("run query should work")
            .is_none());
        assert!(db::get_run_model_profile(&conn, &second_run.run_id)
            .expect("profile query should work")
            .is_none());
    }

    #[test]
    fn connect_job_state_updates_are_compare_and_set() {
        let file = TestFile::pdf();
        let bytes = fs::read(&file.0).unwrap();
        let mut conn = db::init_db(":memory:").unwrap();
        let request = request(&bytes);
        let (document, run) = prepare_pdf_ingestion(
            file.0.to_str().unwrap(),
            Some(&request.inputs[0].display_name),
        )
        .unwrap();
        accept_job_with_ingestion(
            &mut conn,
            &request,
            &request.canonical_hash().unwrap(),
            file.0.to_str().unwrap(),
            &Uuid::new_v4().to_string(),
            &document,
            &run,
        )
        .unwrap();

        assert_eq!(
            mark_processing(&conn, &request.job_id).unwrap().state,
            JobState::Processing
        );
        assert!(mark_processing(&conn, &request.job_id).is_err());
        assert_eq!(
            mark_failed(
                &conn,
                &request.job_id,
                &job_error("TEST_FAILURE", "Injected failure.", false)
            )
            .unwrap()
            .state,
            JobState::Failed
        );
        assert!(mark_failed(
            &conn,
            &request.job_id,
            &job_error("SECOND_FAILURE", "Must not rewrite history.", false)
        )
        .is_err());
    }

    #[test]
    fn interrupted_active_job_becomes_a_durable_retryable_failure() {
        let file = TestFile::pdf();
        let bytes = fs::read(&file.0).unwrap();
        let mut conn = db::init_db(":memory:").unwrap();
        let mut request = request(&bytes);
        request.protocol_version = v2::PROTOCOL_VERSION;
        let provider_instance_id = Uuid::new_v4().to_string();
        let (document, run) = prepare_pdf_ingestion(
            file.0.to_str().unwrap(),
            Some(&request.inputs[0].display_name),
        )
        .unwrap();
        accept_job_with_ingestion(
            &mut conn,
            &request,
            &request.canonical_hash().unwrap(),
            file.0.to_str().unwrap(),
            &provider_instance_id,
            &document,
            &run,
        )
        .unwrap();
        assert_eq!(
            active_v2_provider_instance_id(&conn).unwrap(),
            Some(provider_instance_id)
        );
        assert_eq!(
            mark_interrupted_jobs_failed(
                &conn,
                &job_error("PROVIDER_RESTARTED", "Provider restarted.", true)
            )
            .unwrap(),
            1
        );
        let stored = get_job(&conn, &request.job_id).unwrap().unwrap();
        assert_eq!(stored.state, JobState::Failed);
        assert_eq!(stored.error.unwrap().code, "PROVIDER_RESTARTED");
        assert_eq!(active_v2_provider_instance_id(&conn).unwrap(), None);
    }
}
