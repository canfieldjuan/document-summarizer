#![allow(
    dead_code,
    reason = "the durable ledger lands immediately before its gateway transport consumer"
)]

use crate::pipeline::contracts::PipelineStage;
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::{Uuid, Version};

const MAX_OUTPUT_BYTES: usize = 750_000;
const MAX_DEPLOYMENT_ID_BYTES: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GatewayRequestState {
    Reserved,
    Submitted,
    Completed,
    Acknowledged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GatewayCompletion {
    pub media_type: String,
    pub content: String,
    pub content_sha256: String,
    pub deployment_id: String,
    pub task_policy_version: u32,
}

impl GatewayCompletion {
    pub(crate) fn new(
        media_type: &str,
        content: &str,
        deployment_id: &str,
        task_policy_version: u32,
    ) -> Result<Self, GatewayStoreError> {
        let completion = Self {
            media_type: media_type.to_string(),
            content: content.to_string(),
            content_sha256: sha256_hex(content.as_bytes()),
            deployment_id: deployment_id.to_string(),
            task_policy_version,
        };
        validate_completion(&completion)?;
        Ok(completion)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GatewayRequestKey {
    pub run_id: String,
    pub stage: PipelineStage,
    pub ordinal: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GatewayRequestRecord {
    pub key: GatewayRequestKey,
    pub request_id: String,
    pub request_expires_at: String,
    pub semantic_request_hash: String,
    pub gateway_request_hash: Option<String>,
    pub state: GatewayRequestState,
    pub completion: Option<GatewayCompletion>,
    pub acknowledged: bool,
}

#[derive(Debug, Error)]
pub(crate) enum GatewayStoreError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("JSON persistence error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Gateway request input is invalid: {0}")]
    InvalidInput(&'static str),
    #[error("Gateway request is missing")]
    RequestNotFound,
    #[error("Gateway request expired before its first submission")]
    RequestExpired,
    #[error("Gateway request key already belongs to different semantic input")]
    SemanticIdentityConflict,
    #[error("Gateway request was already submitted with a different canonical digest")]
    SubmissionIdentityConflict,
    #[error("Gateway request cannot transition from {from:?} to {to:?}")]
    InvalidTransition {
        from: GatewayRequestState,
        to: GatewayRequestState,
    },
    #[error("Gateway completion conflicts with the response already persisted locally")]
    CompletionConflict,
    #[error("Persisted gateway output failed its SHA-256 integrity check")]
    OutputIntegrityMismatch,
    #[error("Persisted gateway request is invalid: {0}")]
    InvalidRecord(&'static str),
}

pub(crate) fn reserve_request(
    conn: &mut Connection,
    key: &GatewayRequestKey,
    semantic_request_hash: &str,
    request_expires_at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<GatewayRequestRecord, GatewayStoreError> {
    let stage = stage_name(&key.stage)?;
    validate_digest(semantic_request_hash)?;
    if key.run_id.is_empty() {
        return Err(GatewayStoreError::InvalidInput("run identity is empty"));
    }
    if request_expires_at <= now || request_expires_at.timestamp_subsec_nanos() != 0 {
        return Err(GatewayStoreError::InvalidInput(
            "expiry is not a future whole-second timestamp",
        ));
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let timestamp = timestamp_text(now);
    tx.execute(
        "INSERT INTO model_gateway_requests (
            run_id, stage, request_ordinal, request_id, request_expires_at,
            semantic_request_hash, state, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'reserved', ?7, ?7)
         ON CONFLICT(run_id, stage, request_ordinal) DO NOTHING",
        params![
            key.run_id,
            stage,
            key.ordinal,
            Uuid::new_v4().hyphenated().to_string(),
            timestamp_text(request_expires_at),
            semantic_request_hash,
            timestamp,
        ],
    )?;
    let record = load_required(&tx, key)?;
    if record.semantic_request_hash != semantic_request_hash {
        return Err(GatewayStoreError::SemanticIdentityConflict);
    }
    tx.commit()?;
    Ok(record)
}

pub(crate) fn mark_submitted(
    conn: &mut Connection,
    key: &GatewayRequestKey,
    gateway_request_hash: &str,
    now: DateTime<Utc>,
) -> Result<GatewayRequestRecord, GatewayStoreError> {
    validate_digest(gateway_request_hash)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let record = load_required(&tx, key)?;
    if record
        .gateway_request_hash
        .as_deref()
        .is_some_and(|hash| hash != gateway_request_hash)
    {
        return Err(GatewayStoreError::SubmissionIdentityConflict);
    }
    if record.state == GatewayRequestState::Reserved {
        let expires_at = DateTime::parse_from_rfc3339(&record.request_expires_at)
            .map_err(|_| invalid_record("expiry is invalid"))?
            .with_timezone(&Utc);
        if expires_at <= now {
            return Err(GatewayStoreError::RequestExpired);
        }
        tx.execute(
            "UPDATE model_gateway_requests
             SET gateway_request_hash = ?1, state = 'submitted', updated_at = ?2
             WHERE run_id = ?3 AND stage = ?4 AND request_ordinal = ?5",
            params![
                gateway_request_hash,
                timestamp_text(now),
                key.run_id,
                stage_name(&key.stage)?,
                key.ordinal,
            ],
        )?;
    }
    let submitted = load_required(&tx, key)?;
    tx.commit()?;
    Ok(submitted)
}

pub(crate) fn persist_completion(
    conn: &mut Connection,
    key: &GatewayRequestKey,
    gateway_request_hash: &str,
    completion: &GatewayCompletion,
    now: DateTime<Utc>,
) -> Result<GatewayRequestRecord, GatewayStoreError> {
    validate_digest(gateway_request_hash)?;
    validate_completion(completion)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let record = load_required(&tx, key)?;
    if record.gateway_request_hash.as_deref() != Some(gateway_request_hash) {
        return Err(GatewayStoreError::SubmissionIdentityConflict);
    }
    match record.state {
        GatewayRequestState::Submitted => {
            let completion_json = serde_json::to_string(completion)?;
            let timestamp = timestamp_text(now);
            tx.execute(
                "UPDATE model_gateway_requests
                 SET state = 'completed', completion_json = ?1, completion_sha256 = ?2,
                     completed_at = ?3, updated_at = ?3
                 WHERE run_id = ?4 AND stage = ?5 AND request_ordinal = ?6",
                params![
                    completion_json,
                    sha256_hex(completion_json.as_bytes()),
                    timestamp,
                    key.run_id,
                    stage_name(&key.stage)?,
                    key.ordinal,
                ],
            )?;
        }
        GatewayRequestState::Completed | GatewayRequestState::Acknowledged
            if record.completion.as_ref() == Some(completion) => {}
        GatewayRequestState::Completed | GatewayRequestState::Acknowledged => {
            return Err(GatewayStoreError::CompletionConflict);
        }
        from => {
            return Err(GatewayStoreError::InvalidTransition {
                from,
                to: GatewayRequestState::Completed,
            });
        }
    }
    let completed = load_required(&tx, key)?;
    tx.commit()?;
    Ok(completed)
}

pub(crate) fn mark_acknowledged(
    conn: &mut Connection,
    key: &GatewayRequestKey,
    now: DateTime<Utc>,
) -> Result<GatewayRequestRecord, GatewayStoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let record = load_required(&tx, key)?;
    match record.state {
        GatewayRequestState::Completed => {
            let timestamp = timestamp_text(now);
            tx.execute(
                "UPDATE model_gateway_requests
                 SET state = 'acknowledged', acknowledgement_disposition = 'persisted',
                     acknowledged_at = ?1, updated_at = ?1
                 WHERE run_id = ?2 AND stage = ?3 AND request_ordinal = ?4",
                params![timestamp, key.run_id, stage_name(&key.stage)?, key.ordinal,],
            )?;
        }
        GatewayRequestState::Acknowledged => {}
        from => {
            return Err(GatewayStoreError::InvalidTransition {
                from,
                to: GatewayRequestState::Acknowledged,
            });
        }
    }
    let acknowledged = load_required(&tx, key)?;
    tx.commit()?;
    Ok(acknowledged)
}

pub(crate) fn load_request(
    conn: &Connection,
    key: &GatewayRequestKey,
) -> Result<Option<GatewayRequestRecord>, GatewayStoreError> {
    let raw = conn
        .query_row(
            "SELECT run_id, stage, request_ordinal, request_id, request_expires_at,
                    semantic_request_hash, gateway_request_hash, state, completion_json,
                    completion_sha256, acknowledgement_disposition
             FROM model_gateway_requests
             WHERE run_id = ?1 AND stage = ?2 AND request_ordinal = ?3",
            params![key.run_id, stage_name(&key.stage)?, key.ordinal],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, Option<String>>(10)?,
                ))
            },
        )
        .optional()?;
    raw.map(parse_record).transpose()
}

fn load_required(
    conn: &Connection,
    key: &GatewayRequestKey,
) -> Result<GatewayRequestRecord, GatewayStoreError> {
    load_request(conn, key)?.ok_or(GatewayStoreError::RequestNotFound)
}

type RawRecord = (
    String,
    String,
    i64,
    String,
    String,
    String,
    Option<String>,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn parse_record(raw: RawRecord) -> Result<GatewayRequestRecord, GatewayStoreError> {
    let (
        run_id,
        stage,
        ordinal,
        request_id,
        expiry,
        semantic_hash,
        gateway_hash,
        state,
        completion_json,
        completion_hash,
        acknowledgement,
    ) = raw;
    let uuid =
        Uuid::parse_str(&request_id).map_err(|_| invalid_record("request UUID is invalid"))?;
    if uuid.get_version() != Some(Version::Random) || uuid.hyphenated().to_string() != request_id {
        return Err(invalid_record("request UUID is not canonical lowercase v4"));
    }
    validate_digest(&semantic_hash).map_err(|_| invalid_record("semantic digest is invalid"))?;
    if let Some(hash) = &gateway_hash {
        validate_digest(hash).map_err(|_| invalid_record("gateway digest is invalid"))?;
    }
    DateTime::parse_from_rfc3339(&expiry).map_err(|_| invalid_record("expiry is invalid"))?;
    let completion = match (completion_json, completion_hash) {
        (None, None) => None,
        (Some(json), Some(hash)) if sha256_hex(json.as_bytes()) == hash => {
            let completion: GatewayCompletion = serde_json::from_str(&json)?;
            validate_completion(&completion)?;
            Some(completion)
        }
        (Some(_), Some(_)) => return Err(GatewayStoreError::OutputIntegrityMismatch),
        _ => return Err(invalid_record("completion fields are partially populated")),
    };
    Ok(GatewayRequestRecord {
        key: GatewayRequestKey {
            run_id,
            stage: parse_stage(&stage)?,
            ordinal: u32::try_from(ordinal).map_err(|_| invalid_record("ordinal is invalid"))?,
        },
        request_id,
        request_expires_at: expiry,
        semantic_request_hash: semantic_hash,
        gateway_request_hash: gateway_hash,
        state: parse_state(&state)?,
        completion,
        acknowledged: acknowledgement.as_deref() == Some("persisted"),
    })
}

fn validate_completion(completion: &GatewayCompletion) -> Result<(), GatewayStoreError> {
    if completion.media_type != "application/json"
        || completion.content.is_empty()
        || completion.content.len() > MAX_OUTPUT_BYTES
        || completion.deployment_id.is_empty()
        || completion.deployment_id.len() > MAX_DEPLOYMENT_ID_BYTES
        || completion.task_policy_version == 0
    {
        return Err(GatewayStoreError::InvalidInput(
            "completion violates the gateway output contract",
        ));
    }
    validate_digest(&completion.content_sha256)?;
    if sha256_hex(completion.content.as_bytes()) != completion.content_sha256 {
        return Err(GatewayStoreError::OutputIntegrityMismatch);
    }
    Ok(())
}

fn stage_name(stage: &PipelineStage) -> Result<&'static str, GatewayStoreError> {
    match stage {
        PipelineStage::Analyze => Ok("Analyze"),
        PipelineStage::Synthesize => Ok("Synthesize"),
        PipelineStage::Verify => Ok("Verify"),
        _ => Err(GatewayStoreError::InvalidInput(
            "stage does not perform model inference",
        )),
    }
}

fn parse_stage(value: &str) -> Result<PipelineStage, GatewayStoreError> {
    match value {
        "Analyze" => Ok(PipelineStage::Analyze),
        "Synthesize" => Ok(PipelineStage::Synthesize),
        "Verify" => Ok(PipelineStage::Verify),
        _ => Err(invalid_record("stage is invalid")),
    }
}

fn parse_state(value: &str) -> Result<GatewayRequestState, GatewayStoreError> {
    match value {
        "reserved" => Ok(GatewayRequestState::Reserved),
        "submitted" => Ok(GatewayRequestState::Submitted),
        "completed" => Ok(GatewayRequestState::Completed),
        "acknowledged" => Ok(GatewayRequestState::Acknowledged),
        _ => Err(invalid_record("state is invalid")),
    }
}

fn validate_digest(value: &str) -> Result<(), GatewayStoreError> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(GatewayStoreError::InvalidInput("digest is invalid"))
    }
}

fn timestamp_text(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn sha256_hex(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}

fn invalid_record(message: &'static str) -> GatewayStoreError {
    GatewayStoreError::InvalidRecord(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::db;
    use chrono::TimeZone;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{Arc, Barrier};
    use std::thread;

    struct TestDatabase(PathBuf);

    impl TestDatabase {
        fn new() -> (Self, Connection) {
            let database = Self(
                std::env::temp_dir().join(format!("doc-sum-gateway-store-{}.db", Uuid::new_v4())),
            );
            let conn = db::init_db(&database.0).unwrap();
            conn.execute_batch(
                r#"
                INSERT INTO documents VALUES (
                    'document-1', 'document.pdf', 'pdf', 1, 'hash', '/document.pdf',
                    '2026-09-11T18:00:00Z'
                );
                INSERT INTO pipeline_runs VALUES (
                    'run-1', 'document-1', '"Chunked"', 1, '1.0',
                    '2026-09-11T18:00:00Z', NULL, '2026-09-11T18:00:00Z', NULL, '"Chunk"',
                    '{"total_units":0,"completed_units":0,"failed_units":0}',
                    '[]', NULL, 0, 1
                );
                INSERT INTO pipeline_run_summary_profiles VALUES (
                    'run-1', '"general"', '2026-09-11T18:00:00Z'
                );
                "#,
            )
            .unwrap();
            (database, conn)
        }
    }

    impl Drop for TestDatabase {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    fn key() -> GatewayRequestKey {
        GatewayRequestKey {
            run_id: "run-1".into(),
            stage: PipelineStage::Analyze,
            ordinal: 0,
        }
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 11, 18, 0, 0)
            .single()
            .unwrap()
    }

    fn digest(character: char) -> String {
        character.to_string().repeat(64)
    }

    #[test]
    fn reservations_converge_survive_reopen_and_reject_semantic_collisions() {
        let (database, conn) = TestDatabase::new();
        drop(conn);
        let barrier = Arc::new(Barrier::new(2));
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let path = database.0.clone();
                let barrier = barrier.clone();
                thread::spawn(move || {
                    let mut conn = db::init_db(path).unwrap();
                    barrier.wait();
                    reserve_request(
                        &mut conn,
                        &key(),
                        &digest('a'),
                        now() + chrono::Duration::minutes(5),
                        now(),
                    )
                    .unwrap()
                    .request_id
                })
            })
            .collect();
        let ids: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert_eq!(ids[0], ids[1]);

        let mut reopened = db::init_db(&database.0).unwrap();
        let replay = reserve_request(
            &mut reopened,
            &key(),
            &digest('a'),
            now() + chrono::Duration::minutes(5),
            now(),
        )
        .unwrap();
        assert_eq!(replay.request_id, ids[0]);
        assert!(matches!(
            reserve_request(
                &mut reopened,
                &key(),
                &digest('b'),
                now() + chrono::Duration::minutes(5),
                now()
            ),
            Err(GatewayStoreError::SemanticIdentityConflict)
        ));
    }

    #[test]
    fn lifecycle_is_monotonic_idempotent_and_integrity_checked() {
        let (_database, mut conn) = TestDatabase::new();
        reserve_request(
            &mut conn,
            &key(),
            &digest('a'),
            now() + chrono::Duration::minutes(5),
            now(),
        )
        .unwrap();
        assert!(matches!(
            mark_acknowledged(&mut conn, &key(), now()),
            Err(GatewayStoreError::InvalidTransition { .. })
        ));
        let submitted = mark_submitted(&mut conn, &key(), &digest('b'), now()).unwrap();
        assert_eq!(submitted.state, GatewayRequestState::Submitted);
        assert_eq!(
            mark_submitted(&mut conn, &key(), &digest('b'), now()).unwrap(),
            submitted
        );
        assert!(matches!(
            mark_submitted(&mut conn, &key(), &digest('c'), now()),
            Err(GatewayStoreError::SubmissionIdentityConflict)
        ));

        let completion =
            GatewayCompletion::new("application/json", r#"{"summary":"private"}"#, "host-1", 1)
                .unwrap();
        let completed =
            persist_completion(&mut conn, &key(), &digest('b'), &completion, now()).unwrap();
        assert_eq!(completed.completion.as_ref(), Some(&completion));
        assert_eq!(
            persist_completion(&mut conn, &key(), &digest('b'), &completion, now()).unwrap(),
            completed
        );
        let acknowledged = mark_acknowledged(&mut conn, &key(), now()).unwrap();
        assert!(acknowledged.acknowledged);
        assert_eq!(acknowledged.completion.as_ref(), Some(&completion));
        assert_eq!(
            mark_acknowledged(&mut conn, &key(), now()).unwrap(),
            acknowledged
        );
        assert!(conn
            .execute(
                "UPDATE model_gateway_requests SET state = 'submitted' WHERE run_id = 'run-1'",
                [],
            )
            .is_err());

        conn.execute(
            "UPDATE model_gateway_requests SET completion_json = '{\"tampered\":true}'
             WHERE run_id = 'run-1'",
            [],
        )
        .unwrap();
        assert!(matches!(
            load_request(&conn, &key()),
            Err(GatewayStoreError::OutputIntegrityMismatch)
        ));
    }

    #[test]
    fn input_boundaries_fail_closed() {
        let (_database, mut conn) = TestDatabase::new();
        let mut deterministic_key = key();
        deterministic_key.stage = PipelineStage::Chunk;
        assert!(matches!(
            reserve_request(
                &mut conn,
                &deterministic_key,
                &digest('a'),
                now() + chrono::Duration::minutes(5),
                now()
            ),
            Err(GatewayStoreError::InvalidInput(_))
        ));
        assert!(matches!(
            reserve_request(&mut conn, &key(), &digest('a'), now(), now()),
            Err(GatewayStoreError::InvalidInput(_))
        ));
        assert!(matches!(
            reserve_request(
                &mut conn,
                &key(),
                &digest('a'),
                now() + chrono::Duration::nanoseconds(1),
                now()
            ),
            Err(GatewayStoreError::InvalidInput(_))
        ));
        assert!(matches!(
            reserve_request(
                &mut conn,
                &key(),
                &"A".repeat(64),
                now() + chrono::Duration::minutes(5),
                now()
            ),
            Err(GatewayStoreError::InvalidInput(_))
        ));

        let max_output = "x".repeat(MAX_OUTPUT_BYTES);
        assert!(GatewayCompletion::new("application/json", &max_output, "host-1", 1).is_ok());
        assert!(
            GatewayCompletion::new("application/json", &format!("{max_output}x"), "host-1", 1)
                .is_err()
        );
        assert!(GatewayCompletion::new("text/plain", "{}", "host-1", 1).is_err());
        assert!(GatewayCompletion::new("application/json", "{}", "", 1).is_err());
        assert!(GatewayCompletion::new("application/json", "{}", &"h".repeat(129), 1).is_err());
        assert!(GatewayCompletion::new("application/json", "{}", "host-1", 0).is_err());
    }

    #[test]
    fn first_submission_obeys_expiry_while_reconciliation_remains_reusable() {
        let (_database, mut conn) = TestDatabase::new();
        let expires_at = now() + chrono::Duration::minutes(5);
        reserve_request(&mut conn, &key(), &digest('a'), expires_at, now()).unwrap();
        assert!(matches!(
            mark_submitted(&mut conn, &key(), &digest('b'), expires_at),
            Err(GatewayStoreError::RequestExpired)
        ));
        assert_eq!(
            load_request(&conn, &key()).unwrap().unwrap().state,
            GatewayRequestState::Reserved
        );

        let reusable_key = GatewayRequestKey {
            ordinal: 1,
            ..key()
        };
        reserve_request(&mut conn, &reusable_key, &digest('a'), expires_at, now()).unwrap();
        let submitted = mark_submitted(
            &mut conn,
            &reusable_key,
            &digest('b'),
            expires_at - chrono::Duration::seconds(1),
        )
        .unwrap();
        assert_eq!(submitted.state, GatewayRequestState::Submitted);
        assert_eq!(
            mark_submitted(
                &mut conn,
                &reusable_key,
                &digest('b'),
                expires_at + chrono::Duration::minutes(1),
            )
            .unwrap(),
            submitted
        );
    }
}
