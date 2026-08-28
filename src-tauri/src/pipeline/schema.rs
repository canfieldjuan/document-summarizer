use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use thiserror::Error;

pub const CURRENT_SCHEMA_VERSION: u32 = 2;

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
    let version = version(conn)?;
    if version > CURRENT_SCHEMA_VERSION {
        return Err(MigrationError::UnsupportedVersion {
            found: version,
            supported: CURRENT_SCHEMA_VERSION,
        });
    }
    if version == CURRENT_SCHEMA_VERSION {
        return validate(conn);
    }

    let has_legacy_schema = table_exists(conn, "pipeline_runs")?;
    if version == 0 && !has_legacy_schema {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(SCHEMA_V2)?;
        tx.pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION)?;
        tx.commit()?;
    } else {
        migrate_legacy(conn)?;
    }
    validate(conn)
}

pub fn version(conn: &Connection) -> Result<u32, MigrationError> {
    Ok(conn.pragma_query_value(None, "user_version", |row| row.get(0))?)
}

fn migrate_legacy(conn: &mut Connection) -> Result<(), MigrationError> {
    conn.pragma_update(None, "foreign_keys", "OFF")?;
    let migration_result = (|| -> Result<(), MigrationError> {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(LEGACY_TO_V2)?;
        tx.pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION)?;
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
