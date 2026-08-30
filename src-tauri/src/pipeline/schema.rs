use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use thiserror::Error;

pub const CURRENT_SCHEMA_VERSION: u32 = 12;

const SCHEMA_V2: &str = r#"
CREATE TABLE documents (
    document_id TEXT PRIMARY KEY,
    original_filename TEXT NOT NULL,
    file_type TEXT NOT NULL,
    byte_size INTEGER NOT NULL CHECK (byte_size >= 0),
    content_hash TEXT NOT NULL,
    local_source_path TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE pipeline_runs (
    run_id TEXT PRIMARY KEY,
    document_id TEXT NOT NULL,
    state TEXT NOT NULL,
    state_version INTEGER NOT NULL CHECK (state_version >= 1),
    pipeline_version TEXT NOT NULL,
    created_at TEXT NOT NULL,
    started_at TEXT,
    updated_at TEXT NOT NULL,
    completed_at TEXT,
    current_stage TEXT,
    progress TEXT NOT NULL,
    warnings TEXT NOT NULL,
    failure TEXT,
    cancellation_requested INTEGER NOT NULL CHECK (cancellation_requested IN (0, 1)),
    resumable INTEGER NOT NULL CHECK (resumable IN (0, 1)),
    FOREIGN KEY(document_id) REFERENCES documents(document_id)
);

CREATE TABLE pipeline_events (
    event_id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    sequence_no INTEGER NOT NULL CHECK (sequence_no >= 0),
    previous_state TEXT,
    next_state TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    stage TEXT,
    work_unit_id TEXT,
    reason TEXT,
    UNIQUE(run_id, sequence_no),
    FOREIGN KEY(run_id) REFERENCES pipeline_runs(run_id)
);

CREATE TABLE parsed_documents (
    run_id TEXT PRIMARY KEY,
    document_id TEXT NOT NULL,
    parser_id TEXT NOT NULL,
    parser_version TEXT NOT NULL,
    parsed_artifact TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY(run_id) REFERENCES pipeline_runs(run_id),
    FOREIGN KEY(document_id) REFERENCES documents(document_id)
);

CREATE INDEX pipeline_runs_document_id_idx ON pipeline_runs(document_id);
CREATE INDEX pipeline_events_run_id_idx ON pipeline_events(run_id, sequence_no);
CREATE INDEX parsed_documents_document_id_idx ON parsed_documents(document_id);

CREATE TRIGGER pipeline_events_no_update
BEFORE UPDATE ON pipeline_events
BEGIN
    SELECT RAISE(ABORT, 'pipeline_events are immutable');
END;

CREATE TRIGGER pipeline_events_no_delete
BEFORE DELETE ON pipeline_events
BEGIN
    SELECT RAISE(ABORT, 'pipeline_events are immutable');
END;
"#;

const V2_TO_V3: &str = r#"
CREATE TABLE normalized_documents (
    run_id TEXT PRIMARY KEY,
    document_id TEXT NOT NULL,
    normalization_version TEXT NOT NULL,
    artifact_hash TEXT NOT NULL,
    normalized_artifact TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY(run_id) REFERENCES pipeline_runs(run_id),
    FOREIGN KEY(document_id) REFERENCES documents(document_id)
);

CREATE INDEX normalized_documents_document_id_idx
ON normalized_documents(document_id);
"#;

const V3_TO_V4: &str = r#"
CREATE TABLE structured_documents (
    run_id TEXT PRIMARY KEY,
    document_id TEXT NOT NULL,
    structure_version TEXT NOT NULL,
    artifact_hash TEXT NOT NULL,
    structured_artifact TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY(run_id) REFERENCES pipeline_runs(run_id),
    FOREIGN KEY(document_id) REFERENCES documents(document_id)
);

CREATE INDEX structured_documents_document_id_idx
ON structured_documents(document_id);
"#;

const V4_TO_V5: &str = r#"
CREATE TABLE chunked_documents (
    run_id TEXT PRIMARY KEY,
    document_id TEXT NOT NULL,
    chunking_version TEXT NOT NULL,
    artifact_hash TEXT NOT NULL,
    chunked_artifact TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY(run_id) REFERENCES pipeline_runs(run_id),
    FOREIGN KEY(document_id) REFERENCES documents(document_id)
);

CREATE INDEX chunked_documents_document_id_idx
ON chunked_documents(document_id);
"#;

const V5_TO_V6: &str = r#"
CREATE TABLE analyzed_documents (
    run_id TEXT PRIMARY KEY,
    document_id TEXT NOT NULL,
    analysis_version TEXT NOT NULL,
    artifact_hash TEXT NOT NULL,
    analyzed_artifact TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY(run_id) REFERENCES pipeline_runs(run_id),
    FOREIGN KEY(document_id) REFERENCES documents(document_id)
);

CREATE INDEX analyzed_documents_document_id_idx
ON analyzed_documents(document_id);
"#;

const V6_TO_V7: &str = r#"
CREATE TABLE synthesized_documents (
    run_id TEXT PRIMARY KEY,
    document_id TEXT NOT NULL,
    synthesis_version TEXT NOT NULL,
    artifact_hash TEXT NOT NULL,
    synthesized_artifact TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY(run_id) REFERENCES pipeline_runs(run_id),
    FOREIGN KEY(document_id) REFERENCES documents(document_id)
);

CREATE INDEX synthesized_documents_document_id_idx
ON synthesized_documents(document_id);
"#;

const V7_TO_V8: &str = r#"
CREATE TABLE verified_documents (
    run_id TEXT PRIMARY KEY,
    document_id TEXT NOT NULL,
    verification_version TEXT NOT NULL,
    artifact_hash TEXT NOT NULL,
    verified_artifact TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY(run_id) REFERENCES pipeline_runs(run_id),
    FOREIGN KEY(document_id) REFERENCES documents(document_id)
);

CREATE INDEX verified_documents_document_id_idx
ON verified_documents(document_id);
"#;

const V8_TO_V9: &str = r#"
CREATE TABLE summary_artifacts (
    run_id TEXT PRIMARY KEY,
    document_id TEXT NOT NULL,
    summary_version TEXT NOT NULL,
    artifact_hash TEXT NOT NULL,
    summary_artifact TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY(run_id) REFERENCES pipeline_runs(run_id),
    FOREIGN KEY(document_id) REFERENCES documents(document_id)
);

CREATE INDEX summary_artifacts_document_id_idx
ON summary_artifacts(document_id);
"#;

const V9_TO_V10: &str = r#"
CREATE TABLE connect_jobs (
    job_id TEXT PRIMARY KEY,
    request_hash TEXT NOT NULL,
    capability_id TEXT NOT NULL,
    capability_version TEXT NOT NULL,
    input_artifact_id TEXT NOT NULL,
    input_media_type TEXT NOT NULL,
    input_byte_size INTEGER NOT NULL CHECK (input_byte_size > 0),
    input_sha256 TEXT NOT NULL,
    input_display_name TEXT NOT NULL,
    source_app_id TEXT NOT NULL,
    import_path TEXT NOT NULL,
    pipeline_run_id TEXT NOT NULL UNIQUE,
    provider_instance_id TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('accepted', 'processing', 'completed', 'failed')),
    result_json TEXT,
    error_json TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY(pipeline_run_id) REFERENCES pipeline_runs(run_id),
    CHECK (
        (status IN ('accepted', 'processing') AND result_json IS NULL AND error_json IS NULL)
        OR (status = 'completed' AND result_json IS NOT NULL AND error_json IS NULL)
        OR (status = 'failed' AND result_json IS NULL AND error_json IS NOT NULL)
    )
);

CREATE INDEX connect_jobs_status_idx ON connect_jobs(status);
CREATE INDEX connect_jobs_pipeline_run_idx ON connect_jobs(pipeline_run_id);
CREATE UNIQUE INDEX connect_jobs_single_active_idx ON connect_jobs((1))
WHERE status IN ('accepted', 'processing');
"#;

const V10_TO_V11: &str = r#"
CREATE TABLE citation_artifacts (
    run_id TEXT PRIMARY KEY,
    document_id TEXT NOT NULL,
    citation_version TEXT NOT NULL,
    summary_integrity_hash TEXT NOT NULL,
    artifact_hash TEXT NOT NULL,
    citation_artifact TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY(run_id) REFERENCES pipeline_runs(run_id),
    FOREIGN KEY(document_id) REFERENCES documents(document_id)
);

CREATE INDEX citation_artifacts_document_id_idx
ON citation_artifacts(document_id);
"#;

const V11_TO_V12: &str = r#"
CREATE TABLE pipeline_run_retries (
    retry_run_id TEXT PRIMARY KEY,
    source_run_id TEXT NOT NULL,
    checkpoint TEXT NOT NULL CHECK (checkpoint = '"Ingested"'),
    created_at TEXT NOT NULL,
    CHECK (retry_run_id <> source_run_id),
    FOREIGN KEY(retry_run_id) REFERENCES pipeline_runs(run_id),
    FOREIGN KEY(source_run_id) REFERENCES pipeline_runs(run_id)
);

CREATE UNIQUE INDEX pipeline_run_retries_source_run_id_uq
ON pipeline_run_retries(source_run_id);

CREATE TRIGGER pipeline_run_retries_no_update
BEFORE UPDATE ON pipeline_run_retries
BEGIN
    SELECT RAISE(ABORT, 'pipeline_run_retries are immutable');
END;

CREATE TRIGGER pipeline_run_retries_no_delete
BEFORE DELETE ON pipeline_run_retries
BEGIN
    SELECT RAISE(ABORT, 'pipeline_run_retries are immutable');
END;
"#;

const LEGACY_TO_V2: &str = r#"
DROP TRIGGER IF EXISTS pipeline_events_no_update;
DROP TRIGGER IF EXISTS pipeline_events_no_delete;

CREATE TABLE IF NOT EXISTS documents (
    document_id TEXT PRIMARY KEY,
    original_filename TEXT NOT NULL,
    file_type TEXT NOT NULL,
    byte_size INTEGER NOT NULL,
    content_hash TEXT NOT NULL,
    local_source_path TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS parsed_documents (
    document_id TEXT PRIMARY KEY,
    parser_id TEXT NOT NULL,
    parser_version TEXT NOT NULL,
    parsed_artifact TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS pipeline_events (
    event_id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    previous_state TEXT,
    next_state TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    stage TEXT,
    work_unit_id TEXT,
    reason TEXT
);

CREATE TABLE pipeline_runs_v2 (
    run_id TEXT PRIMARY KEY,
    document_id TEXT NOT NULL,
    state TEXT NOT NULL,
    state_version INTEGER NOT NULL CHECK (state_version >= 1),
    pipeline_version TEXT NOT NULL,
    created_at TEXT NOT NULL,
    started_at TEXT,
    updated_at TEXT NOT NULL,
    completed_at TEXT,
    current_stage TEXT,
    progress TEXT NOT NULL,
    warnings TEXT NOT NULL,
    failure TEXT,
    cancellation_requested INTEGER NOT NULL CHECK (cancellation_requested IN (0, 1)),
    resumable INTEGER NOT NULL CHECK (resumable IN (0, 1)),
    FOREIGN KEY(document_id) REFERENCES documents(document_id)
);

INSERT INTO pipeline_runs_v2 SELECT * FROM pipeline_runs;

CREATE TABLE pipeline_events_v2 (
    event_id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    sequence_no INTEGER NOT NULL CHECK (sequence_no >= 0),
    previous_state TEXT,
    next_state TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    stage TEXT,
    work_unit_id TEXT,
    reason TEXT,
    UNIQUE(run_id, sequence_no),
    FOREIGN KEY(run_id) REFERENCES pipeline_runs_v2(run_id)
);

INSERT INTO pipeline_events_v2 (
    event_id, run_id, sequence_no, previous_state, next_state, timestamp, stage, work_unit_id, reason
)
SELECT
    'migration-received-' || run_id,
    run_id,
    0,
    NULL,
    '"Received"',
    created_at,
    NULL,
    NULL,
    'run_created_from_legacy_schema'
FROM pipeline_runs_v2;

INSERT INTO pipeline_events_v2 (
    event_id, run_id, sequence_no, previous_state, next_state, timestamp, stage, work_unit_id, reason
)
SELECT
    event_id,
    run_id,
    ROW_NUMBER() OVER (PARTITION BY run_id ORDER BY timestamp, event_id),
    previous_state,
    next_state,
    timestamp,
    stage,
    work_unit_id,
    reason
FROM pipeline_events;

CREATE TABLE parsed_documents_v2 (
    run_id TEXT PRIMARY KEY,
    document_id TEXT NOT NULL,
    parser_id TEXT NOT NULL,
    parser_version TEXT NOT NULL,
    parsed_artifact TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY(run_id) REFERENCES pipeline_runs_v2(run_id),
    FOREIGN KEY(document_id) REFERENCES documents(document_id)
);

INSERT INTO parsed_documents_v2 (
    run_id, document_id, parser_id, parser_version, parsed_artifact, created_at
)
SELECT
    pipeline_runs_v2.run_id,
    parsed_documents.document_id,
    parsed_documents.parser_id,
    parsed_documents.parser_version,
    parsed_documents.parsed_artifact,
    pipeline_runs_v2.updated_at
FROM parsed_documents
JOIN pipeline_runs_v2 USING (document_id);

DROP TABLE pipeline_events;
DROP TABLE parsed_documents;
DROP TABLE pipeline_runs;

ALTER TABLE pipeline_runs_v2 RENAME TO pipeline_runs;
ALTER TABLE pipeline_events_v2 RENAME TO pipeline_events;
ALTER TABLE parsed_documents_v2 RENAME TO parsed_documents;

CREATE INDEX pipeline_runs_document_id_idx ON pipeline_runs(document_id);
CREATE INDEX pipeline_events_run_id_idx ON pipeline_events(run_id, sequence_no);
CREATE INDEX parsed_documents_document_id_idx ON parsed_documents(document_id);

CREATE TRIGGER pipeline_events_no_update
BEFORE UPDATE ON pipeline_events
BEGIN
    SELECT RAISE(ABORT, 'pipeline_events are immutable');
END;

CREATE TRIGGER pipeline_events_no_delete
BEFORE DELETE ON pipeline_events
BEGIN
    SELECT RAISE(ABORT, 'pipeline_events are immutable');
END;
"#;

#[derive(Debug, Error)]
pub enum MigrationError {
    #[error("SQLite migration error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("Database schema version {found} is newer than supported version {supported}")]
    UnsupportedVersion { found: u32, supported: u32 },
    #[error("Database schema invariant failed: {0}")]
    Invariant(String),
}

pub fn migrate(conn: &mut Connection) -> Result<(), MigrationError> {
    let mut current_version = version(conn)?;
    if current_version > CURRENT_SCHEMA_VERSION {
        return Err(MigrationError::UnsupportedVersion {
            found: current_version,
            supported: CURRENT_SCHEMA_VERSION,
        });
    }
    if current_version == CURRENT_SCHEMA_VERSION {
        return validate(conn);
    }

    let has_legacy_schema = table_exists(conn, "pipeline_runs")?;
    if current_version == 0 && !has_legacy_schema {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(SCHEMA_V2)?;
        tx.execute_batch(V2_TO_V3)?;
        tx.execute_batch(V3_TO_V4)?;
        tx.execute_batch(V4_TO_V5)?;
        tx.execute_batch(V5_TO_V6)?;
        tx.execute_batch(V6_TO_V7)?;
        tx.execute_batch(V7_TO_V8)?;
        tx.execute_batch(V8_TO_V9)?;
        tx.execute_batch(V9_TO_V10)?;
        tx.execute_batch(V10_TO_V11)?;
        tx.execute_batch(V11_TO_V12)?;
        tx.pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION)?;
        tx.commit()?;
        return validate(conn);
    }

    if current_version < 2 {
        migrate_legacy_to_v2(conn)?;
        current_version = 2;
    }
    if current_version == 2 {
        migrate_v2_to_v3(conn)?;
        current_version = 3;
    }
    if current_version == 3 {
        migrate_v3_to_v4(conn)?;
        current_version = 4;
    }
    if current_version == 4 {
        migrate_v4_to_v5(conn)?;
        current_version = 5;
    }
    if current_version == 5 {
        migrate_v5_to_v6(conn)?;
        current_version = 6;
    }
    if current_version == 6 {
        migrate_v6_to_v7(conn)?;
        current_version = 7;
    }
    if current_version == 7 {
        migrate_v7_to_v8(conn)?;
        current_version = 8;
    }
    if current_version == 8 {
        migrate_v8_to_v9(conn)?;
        current_version = 9;
    }
    if current_version == 9 {
        migrate_v9_to_v10(conn)?;
        current_version = 10;
    }
    if current_version == 10 {
        migrate_v10_to_v11(conn)?;
        current_version = 11;
    }
    if current_version == 11 {
        migrate_v11_to_v12(conn)?;
    }
    validate(conn)
}

pub fn version(conn: &Connection) -> Result<u32, MigrationError> {
    Ok(conn.pragma_query_value(None, "user_version", |row| row.get(0))?)
}

fn migrate_legacy_to_v2(conn: &mut Connection) -> Result<(), MigrationError> {
    conn.pragma_update(None, "foreign_keys", "OFF")?;
    let migration_result = (|| -> Result<(), MigrationError> {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(LEGACY_TO_V2)?;
        tx.pragma_update(None, "user_version", 2)?;
        tx.commit()?;
        Ok(())
    })();
    let enable_result = conn
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(MigrationError::from);
    migration_result?;
    enable_result?;
    Ok(())
}

fn migrate_v2_to_v3(conn: &mut Connection) -> Result<(), MigrationError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute_batch(V2_TO_V3)?;
    tx.pragma_update(None, "user_version", 3)?;
    tx.commit()?;
    Ok(())
}

fn migrate_v3_to_v4(conn: &mut Connection) -> Result<(), MigrationError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute_batch(V3_TO_V4)?;
    tx.pragma_update(None, "user_version", 4)?;
    tx.commit()?;
    Ok(())
}

fn migrate_v4_to_v5(conn: &mut Connection) -> Result<(), MigrationError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute_batch(V4_TO_V5)?;
    tx.pragma_update(None, "user_version", 5)?;
    tx.commit()?;
    Ok(())
}

fn migrate_v5_to_v6(conn: &mut Connection) -> Result<(), MigrationError> {
    migrate_additive(conn, V5_TO_V6, 6)
}

fn migrate_v6_to_v7(conn: &mut Connection) -> Result<(), MigrationError> {
    migrate_additive(conn, V6_TO_V7, 7)
}

fn migrate_v7_to_v8(conn: &mut Connection) -> Result<(), MigrationError> {
    migrate_additive(conn, V7_TO_V8, 8)
}

fn migrate_v8_to_v9(conn: &mut Connection) -> Result<(), MigrationError> {
    migrate_additive(conn, V8_TO_V9, 9)
}

fn migrate_v9_to_v10(conn: &mut Connection) -> Result<(), MigrationError> {
    migrate_additive(conn, V9_TO_V10, 10)
}

fn migrate_v10_to_v11(conn: &mut Connection) -> Result<(), MigrationError> {
    migrate_additive(conn, V10_TO_V11, 11)
}

fn migrate_v11_to_v12(conn: &mut Connection) -> Result<(), MigrationError> {
    migrate_additive(conn, V11_TO_V12, 12)
}

fn migrate_additive(
    conn: &mut Connection,
    statements: &str,
    version: u32,
) -> Result<(), MigrationError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute_batch(statements)?;
    tx.pragma_update(None, "user_version", version)?;
    tx.commit()?;
    Ok(())
}

fn validate(conn: &Connection) -> Result<(), MigrationError> {
    let quick_check: String = conn.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    if quick_check != "ok" {
        return Err(MigrationError::Invariant(format!(
            "quick_check returned {quick_check}"
        )));
    }

    let event_sequence_columns: u32 = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('pipeline_events') WHERE name = 'sequence_no'",
        [],
        |row| row.get(0),
    )?;
    if event_sequence_columns != 1 {
        return Err(MigrationError::Invariant(
            "pipeline_events.sequence_no is missing".to_string(),
        ));
    }

    let normalized_columns: u32 = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('normalized_documents')
         WHERE name IN ('normalization_version', 'artifact_hash', 'normalized_artifact')",
        [],
        |row| row.get(0),
    )?;
    if normalized_columns != 3 {
        return Err(MigrationError::Invariant(
            "normalized_documents artifact columns are missing".to_string(),
        ));
    }

    let structured_columns: u32 = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('structured_documents')
         WHERE name IN ('structure_version', 'artifact_hash', 'structured_artifact')",
        [],
        |row| row.get(0),
    )?;
    if structured_columns != 3 {
        return Err(MigrationError::Invariant(
            "structured_documents artifact columns are missing".to_string(),
        ));
    }

    let chunked_columns: u32 = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('chunked_documents')
         WHERE name IN ('chunking_version', 'artifact_hash', 'chunked_artifact')",
        [],
        |row| row.get(0),
    )?;
    if chunked_columns != 3 {
        return Err(MigrationError::Invariant(
            "chunked_documents artifact columns are missing".to_string(),
        ));
    }

    for (table, columns) in [
        (
            "analyzed_documents",
            ["analysis_version", "artifact_hash", "analyzed_artifact"],
        ),
        (
            "synthesized_documents",
            ["synthesis_version", "artifact_hash", "synthesized_artifact"],
        ),
        (
            "verified_documents",
            ["verification_version", "artifact_hash", "verified_artifact"],
        ),
        (
            "summary_artifacts",
            ["summary_version", "artifact_hash", "summary_artifact"],
        ),
    ] {
        for column in columns {
            let present: u32 = conn.query_row(
                "SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name = ?2",
                [table, column],
                |row| row.get(0),
            )?;
            if present != 1 {
                return Err(MigrationError::Invariant(format!(
                    "{table}.{column} is missing"
                )));
            }
        }
    }

    for column in [
        "citation_version",
        "summary_integrity_hash",
        "artifact_hash",
        "citation_artifact",
    ] {
        let present: u32 = conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('citation_artifacts') WHERE name = ?1",
            [column],
            |row| row.get(0),
        )?;
        if present != 1 {
            return Err(MigrationError::Invariant(format!(
                "citation_artifacts.{column} is missing"
            )));
        }
    }

    for column in [
        "request_hash",
        "input_artifact_id",
        "input_sha256",
        "pipeline_run_id",
        "provider_instance_id",
        "status",
        "result_json",
        "error_json",
    ] {
        let present: u32 = conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('connect_jobs') WHERE name = ?1",
            [column],
            |row| row.get(0),
        )?;
        if present != 1 {
            return Err(MigrationError::Invariant(format!(
                "connect_jobs.{column} is missing"
            )));
        }
    }
    let single_active_index: u32 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master
         WHERE type = 'index' AND name = 'connect_jobs_single_active_idx'",
        [],
        |row| row.get(0),
    )?;
    if single_active_index != 1 {
        return Err(MigrationError::Invariant(
            "connect_jobs single-active-job index is missing".to_string(),
        ));
    }

    for column in ["retry_run_id", "source_run_id", "checkpoint", "created_at"] {
        let present: u32 = conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('pipeline_run_retries') WHERE name = ?1",
            [column],
            |row| row.get(0),
        )?;
        if present != 1 {
            return Err(MigrationError::Invariant(format!(
                "pipeline_run_retries.{column} is missing"
            )));
        }
    }
    let retry_source_index: u32 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master
         WHERE type = 'index' AND name = 'pipeline_run_retries_source_run_id_uq'",
        [],
        |row| row.get(0),
    )?;
    if retry_source_index != 1 {
        return Err(MigrationError::Invariant(
            "pipeline_run_retries source-run uniqueness index is missing".to_string(),
        ));
    }
    for trigger in [
        "pipeline_run_retries_no_update",
        "pipeline_run_retries_no_delete",
    ] {
        let present: u32 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger' AND name = ?1",
            [trigger],
            |row| row.get(0),
        )?;
        if present != 1 {
            return Err(MigrationError::Invariant(format!("{trigger} is missing")));
        }
    }

    let foreign_key_violation: Option<String> = conn
        .query_row("PRAGMA foreign_key_check", [], |row| row.get(0))
        .optional()?;
    if let Some(table) = foreign_key_violation {
        return Err(MigrationError::Invariant(format!(
            "foreign key violation in {table}"
        )));
    }
    Ok(())
}

fn table_exists(conn: &Connection, table: &str) -> Result<bool, MigrationError> {
    let count: u32 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [table],
        |row| row.get(0),
    )?;
    Ok(count == 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use uuid::Uuid;

    struct TestDatabase(PathBuf);

    impl TestDatabase {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("doc-sum-schema-{}.db", Uuid::new_v4())))
        }
    }

    impl Drop for TestDatabase {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[test]
    fn schema_v2_upgrades_to_current_without_rewriting_slice2_artifacts() {
        let database = TestDatabase::new();
        {
            let conn = Connection::open(&database.0).expect("v2 database should open");
            conn.pragma_update(None, "foreign_keys", "ON")
                .expect("foreign keys should enable");
            conn.execute_batch(SCHEMA_V2)
                .expect("v2 schema should initialize");
            conn.pragma_update(None, "user_version", 2)
                .expect("v2 version should persist");
            conn.execute_batch(
                r#"
                INSERT INTO documents VALUES (
                    'slice2-document', 'slice2.pdf', 'pdf', 12, 'slice2-hash',
                    '/slice2.pdf', '2026-08-28T00:00:00+00:00'
                );
                INSERT INTO pipeline_runs VALUES (
                    'slice2-run', 'slice2-document', '"Parsed"', 5, '1.0',
                    '2026-08-28T00:00:00+00:00', '2026-08-28T00:00:01+00:00',
                    '2026-08-28T00:00:02+00:00', NULL, '"Parse"',
                    '{"total_units":0,"completed_units":0,"failed_units":0}',
                    '[]', NULL, 0, 1
                );
                INSERT INTO pipeline_events VALUES (
                    'slice2-event', 'slice2-run', 0, NULL, '"Parsed"',
                    '2026-08-28T00:00:02+00:00', '"Parse"', NULL, NULL
                );
                INSERT INTO parsed_documents VALUES (
                    'slice2-run', 'slice2-document', 'fixture-parser', 'test',
                    '{"slice":2}', '2026-08-28T00:00:02+00:00'
                );
                "#,
            )
            .expect("Slice 2 rows should persist");
        }

        {
            let mut conn = Connection::open(&database.0).expect("database should reopen");
            conn.pragma_update(None, "foreign_keys", "ON")
                .expect("foreign keys should enable");
            migrate(&mut conn).expect("v2 schema should migrate");
            assert_eq!(
                version(&conn).expect("version should load"),
                CURRENT_SCHEMA_VERSION
            );
            assert_eq!(
                conn.query_row(
                    "SELECT parsed_artifact FROM parsed_documents WHERE run_id = 'slice2-run'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .expect("parsed artifact should survive"),
                r#"{"slice":2}"#
            );
            assert_eq!(
                conn.query_row("SELECT COUNT(*) FROM normalized_documents", [], |row| {
                    row.get::<_, u32>(0)
                })
                .expect("normalized table should exist"),
                0
            );
            assert_eq!(
                conn.query_row("SELECT COUNT(*) FROM structured_documents", [], |row| {
                    row.get::<_, u32>(0)
                })
                .expect("structured table should exist"),
                0
            );
        }

        let mut reopened = Connection::open(&database.0).expect("migrated database should reopen");
        reopened
            .pragma_update(None, "foreign_keys", "ON")
            .expect("foreign keys should enable");
        migrate(&mut reopened).expect("repeated initialization should be deterministic");
        assert_eq!(
            version(&reopened).expect("version should load"),
            CURRENT_SCHEMA_VERSION
        );
    }

    #[test]
    fn schema_v3_upgrades_to_v4_without_rewriting_slice3_artifacts() {
        let database = TestDatabase::new();
        {
            let conn = Connection::open(&database.0).expect("v3 database should open");
            conn.pragma_update(None, "foreign_keys", "ON")
                .expect("foreign keys should enable");
            conn.execute_batch(SCHEMA_V2)
                .expect("v2 schema should initialize");
            conn.execute_batch(V2_TO_V3)
                .expect("v3 schema should initialize");
            conn.pragma_update(None, "user_version", 3)
                .expect("v3 version should persist");
            conn.execute_batch(
                r#"
                INSERT INTO documents VALUES (
                    'slice3-document', 'slice3.pdf', 'pdf', 12, 'slice3-hash',
                    '/slice3.pdf', '2026-08-28T00:00:00+00:00'
                );
                INSERT INTO pipeline_runs VALUES (
                    'slice3-run', 'slice3-document', '"Normalized"', 7, '1.0',
                    '2026-08-28T00:00:00+00:00', '2026-08-28T00:00:01+00:00',
                    '2026-08-28T00:00:02+00:00', NULL, '"Normalize"',
                    '{"total_units":0,"completed_units":0,"failed_units":0}',
                    '[]', NULL, 0, 1
                );
                INSERT INTO pipeline_events VALUES (
                    'slice3-event', 'slice3-run', 0, NULL, '"Normalized"',
                    '2026-08-28T00:00:02+00:00', '"Normalize"', NULL, NULL
                );
                INSERT INTO normalized_documents VALUES (
                    'slice3-run', 'slice3-document', '1.0.0', 'slice3-artifact-hash',
                    '{"slice":3}', '2026-08-28T00:00:02+00:00'
                );
                "#,
            )
            .expect("Slice 3 rows should persist");
        }

        {
            let mut conn = Connection::open(&database.0).expect("database should reopen");
            conn.pragma_update(None, "foreign_keys", "ON")
                .expect("foreign keys should enable");
            migrate(&mut conn).expect("v3 schema should migrate");
            assert_eq!(
                version(&conn).expect("version should load"),
                CURRENT_SCHEMA_VERSION
            );
            assert_eq!(
                conn.query_row(
                    "SELECT normalized_artifact FROM normalized_documents
                     WHERE run_id = 'slice3-run'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .expect("normalized artifact should survive"),
                r#"{"slice":3}"#
            );
            assert_eq!(
                conn.query_row("SELECT COUNT(*) FROM structured_documents", [], |row| {
                    row.get::<_, u32>(0)
                })
                .expect("structured table should exist"),
                0
            );
        }

        let mut reopened = Connection::open(&database.0).expect("migrated database should reopen");
        reopened
            .pragma_update(None, "foreign_keys", "ON")
            .expect("foreign keys should enable");
        migrate(&mut reopened).expect("repeated initialization should be deterministic");
        assert_eq!(
            version(&reopened).expect("version should load"),
            CURRENT_SCHEMA_VERSION
        );
    }

    #[test]
    fn schema_v5_upgrades_to_current_without_rewriting_chunked_artifacts() {
        let database = TestDatabase::new();
        {
            let conn = Connection::open(&database.0).expect("v5 database should open");
            conn.pragma_update(None, "foreign_keys", "ON")
                .expect("foreign keys should enable");
            conn.execute_batch(SCHEMA_V2)
                .expect("v2 schema should initialize");
            conn.execute_batch(V2_TO_V3)
                .expect("v3 schema should initialize");
            conn.execute_batch(V3_TO_V4)
                .expect("v4 schema should initialize");
            conn.execute_batch(V4_TO_V5)
                .expect("v5 schema should initialize");
            conn.pragma_update(None, "user_version", 5)
                .expect("v5 version should persist");
            conn.execute_batch(
                r#"
                INSERT INTO documents VALUES (
                    'slice5-document', 'slice5.pdf', 'pdf', 12, 'slice5-hash',
                    '/slice5.pdf', '2026-08-29T00:00:00+00:00'
                );
                INSERT INTO pipeline_runs VALUES (
                    'slice5-run', 'slice5-document', '"Chunked"', 11, '1.0',
                    '2026-08-29T00:00:00+00:00', '2026-08-29T00:00:01+00:00',
                    '2026-08-29T00:00:02+00:00', NULL, '"Chunk"',
                    '{"total_units":0,"completed_units":0,"failed_units":0}',
                    '[]', NULL, 0, 1
                );
                INSERT INTO pipeline_events VALUES (
                    'slice5-event', 'slice5-run', 0, NULL, '"Chunked"',
                    '2026-08-29T00:00:02+00:00', '"Chunk"', NULL, NULL
                );
                INSERT INTO chunked_documents VALUES (
                    'slice5-run', 'slice5-document', '1.0.0', 'slice5-artifact-hash',
                    '{"slice":5}', '2026-08-29T00:00:02+00:00'
                );
                "#,
            )
            .expect("Slice 5 rows should persist");
        }

        {
            let mut conn = Connection::open(&database.0).expect("database should reopen");
            conn.pragma_update(None, "foreign_keys", "ON")
                .expect("foreign keys should enable");
            migrate(&mut conn).expect("v5 schema should migrate");
            assert_eq!(
                version(&conn).expect("version should load"),
                CURRENT_SCHEMA_VERSION
            );
            assert_eq!(
                conn.query_row(
                    "SELECT chunked_artifact FROM chunked_documents WHERE run_id = 'slice5-run'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .expect("chunked artifact should survive"),
                r#"{"slice":5}"#
            );
            for table in [
                "analyzed_documents",
                "synthesized_documents",
                "verified_documents",
                "summary_artifacts",
                "citation_artifacts",
                "connect_jobs",
            ] {
                let present: u32 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                        [table],
                        |row| row.get(0),
                    )
                    .expect("artifact table should be queryable");
                assert_eq!(present, 1);
            }
        }

        let mut reopened = Connection::open(&database.0).expect("migrated database should reopen");
        reopened
            .pragma_update(None, "foreign_keys", "ON")
            .expect("foreign keys should enable");
        migrate(&mut reopened).expect("repeated initialization should be deterministic");
        assert_eq!(
            version(&reopened).expect("version should load"),
            CURRENT_SCHEMA_VERSION
        );
    }

    #[test]
    fn schema_v10_adds_citations_without_rewriting_existing_summary_artifacts() {
        let database = TestDatabase::new();
        {
            let conn = Connection::open(&database.0).expect("v10 database should open");
            conn.pragma_update(None, "foreign_keys", "ON")
                .expect("foreign keys should enable");
            for migration in [
                SCHEMA_V2, V2_TO_V3, V3_TO_V4, V4_TO_V5, V5_TO_V6, V6_TO_V7, V7_TO_V8, V8_TO_V9,
                V9_TO_V10,
            ] {
                conn.execute_batch(migration)
                    .expect("schema through v10 should initialize");
            }
            conn.pragma_update(None, "user_version", 10)
                .expect("v10 version should persist");
            conn.execute_batch(
                r#"
                INSERT INTO documents VALUES (
                    'legacy-document', 'legacy.pdf', 'pdf', 12, 'legacy-hash',
                    '/legacy.pdf', '2026-08-29T00:00:00+00:00'
                );
                INSERT INTO pipeline_runs VALUES (
                    'legacy-run', 'legacy-document', '"CompleteWithWarnings"', 18, '1.0',
                    '2026-08-29T00:00:00+00:00', '2026-08-29T00:00:01+00:00',
                    '2026-08-29T00:00:02+00:00', '2026-08-29T00:00:03+00:00', '"Verify"',
                    '{"total_units":0,"completed_units":0,"failed_units":0}',
                    '[]', NULL, 0, 1
                );
                INSERT INTO summary_artifacts VALUES (
                    'legacy-run', 'legacy-document', '1.0.0', 'legacy-row-hash',
                    '{"slice":6}', '2026-08-29T00:00:03+00:00'
                );
                "#,
            )
            .expect("legacy v10 summary row should persist");
        }

        {
            let mut conn = Connection::open(&database.0).expect("database should reopen");
            conn.pragma_update(None, "foreign_keys", "ON")
                .expect("foreign keys should enable");
            migrate(&mut conn).expect("v10 schema should migrate");
            assert_eq!(
                conn.query_row(
                    "SELECT summary_artifact FROM summary_artifacts WHERE run_id = 'legacy-run'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .expect("legacy summary should survive"),
                r#"{"slice":6}"#
            );
            assert_eq!(
                conn.query_row("SELECT COUNT(*) FROM citation_artifacts", [], |row| {
                    row.get::<_, u32>(0)
                })
                .expect("citation table should exist"),
                0
            );
            assert_eq!(
                version(&conn).expect("version should load"),
                CURRENT_SCHEMA_VERSION
            );
        }

        let mut reopened = Connection::open(&database.0).expect("migrated database should reopen");
        reopened
            .pragma_update(None, "foreign_keys", "ON")
            .expect("foreign keys should enable");
        migrate(&mut reopened).expect("repeated initialization should be deterministic");
        assert_eq!(
            version(&reopened).expect("version should load"),
            CURRENT_SCHEMA_VERSION
        );
    }

    #[test]
    fn schema_v11_adds_immutable_retry_lineage_without_rewriting_runs() {
        let database = TestDatabase::new();
        {
            let conn = Connection::open(&database.0).expect("v11 database should open");
            conn.pragma_update(None, "foreign_keys", "ON")
                .expect("foreign keys should enable");
            for migration in [
                SCHEMA_V2, V2_TO_V3, V3_TO_V4, V4_TO_V5, V5_TO_V6, V6_TO_V7, V7_TO_V8, V8_TO_V9,
                V9_TO_V10, V10_TO_V11,
            ] {
                conn.execute_batch(migration)
                    .expect("schema through v11 should initialize");
            }
            conn.pragma_update(None, "user_version", 11)
                .expect("v11 version should persist");
            conn.execute_batch(
                r#"
                INSERT INTO documents VALUES (
                    'retry-document', 'retry.pdf', 'pdf', 12, 'retry-hash',
                    '/retry.pdf', '2026-08-29T00:00:00+00:00'
                );
                INSERT INTO pipeline_runs VALUES (
                    'failed-run', 'retry-document', '"Failed"', 5, '1.0',
                    '2026-08-29T00:00:00+00:00', '2026-08-29T00:00:01+00:00',
                    '2026-08-29T00:00:02+00:00', '2026-08-29T00:00:02+00:00', '"Parse"',
                    '{"total_units":0,"completed_units":0,"failed_units":0}',
                    '[]', '{"code":"PROCESS_INTERRUPTED","message":"stopped","stage":"Parse","recoverable":true}',
                    0, 1
                );
                INSERT INTO pipeline_runs VALUES (
                    'retry-run', 'retry-document', '"Ingested"', 3, '1.0',
                    '2026-08-29T00:00:03+00:00', '2026-08-29T00:00:03+00:00',
                    '2026-08-29T00:00:03+00:00', NULL, '"Ingest"',
                    '{"total_units":0,"completed_units":0,"failed_units":0}',
                    '[]', NULL, 0, 1
                );
                "#,
            )
            .expect("v11 runs should persist");
        }

        {
            let mut conn = Connection::open(&database.0).expect("database should reopen");
            conn.pragma_update(None, "foreign_keys", "ON")
                .expect("foreign keys should enable");
            migrate(&mut conn).expect("v11 schema should migrate");
            assert_eq!(
                version(&conn).expect("version should load"),
                CURRENT_SCHEMA_VERSION
            );
            assert_eq!(
                conn.query_row("SELECT COUNT(*) FROM pipeline_runs", [], |row| {
                    row.get::<_, u32>(0)
                })
                .expect("existing runs should survive"),
                2
            );
            conn.execute(
                "INSERT INTO pipeline_run_retries (
                    retry_run_id, source_run_id, checkpoint, created_at
                 ) VALUES (?1, ?2, ?3, ?4)",
                [
                    "retry-run",
                    "failed-run",
                    "\"Ingested\"",
                    "2026-08-29T00:00:03+00:00",
                ],
            )
            .expect("retry lineage should persist");
            assert!(conn
                .execute(
                    "UPDATE pipeline_run_retries SET checkpoint = checkpoint
                     WHERE retry_run_id = 'retry-run'",
                    [],
                )
                .is_err());
            assert!(conn
                .execute(
                    "DELETE FROM pipeline_run_retries WHERE retry_run_id = 'retry-run'",
                    [],
                )
                .is_err());
            assert_eq!(
                conn.query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
                    .expect("quick check should run"),
                "ok"
            );
        }

        let mut reopened = Connection::open(&database.0).expect("migrated database should reopen");
        reopened
            .pragma_update(None, "foreign_keys", "ON")
            .expect("foreign keys should enable");
        migrate(&mut reopened).expect("repeated initialization should be deterministic");
        assert_eq!(
            reopened
                .query_row("SELECT COUNT(*) FROM pipeline_run_retries", [], |row| {
                    row.get::<_, u32>(0)
                })
                .expect("retry lineage should survive reopen"),
            1
        );
    }
}
