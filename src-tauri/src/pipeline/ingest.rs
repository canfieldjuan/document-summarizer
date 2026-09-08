use crate::pipeline::contracts::{
    IngestedDocument, ModelProfileSnapshot, PipelineProgress, PipelineRun, PipelineState,
    SummaryProfile,
};
use crate::pipeline::db::{self, StoreError};
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum IngestError {
    #[error("File not found or inaccessible: {0}")]
    IoError(#[from] io::Error),
    #[error("Unsupported file extension; expected .pdf")]
    UnsupportedExtension,
    #[error("File is not a PDF candidate because it lacks the %PDF- signature")]
    InvalidPdfSignature,
    #[error("Source path cannot be represented as UTF-8")]
    InvalidSourcePath,
    #[error("Source has no usable filename")]
    InvalidFilename,
    #[error("Source content changed after profile suggestion")]
    SourceChanged,
    #[error("Database persistence failed: {0}")]
    Store(#[from] StoreError),
}

impl IngestError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::IoError(_) => "SOURCE_IO_ERROR",
            Self::UnsupportedExtension => "UNSUPPORTED_FILE_TYPE",
            Self::InvalidPdfSignature => "INVALID_PDF_SIGNATURE",
            Self::InvalidSourcePath => "INVALID_SOURCE_PATH",
            Self::InvalidFilename => "INVALID_FILENAME",
            Self::SourceChanged => "SOURCE_CHANGED_AFTER_PROFILE_SUGGESTION",
            Self::Store(_) => "DATABASE_ERROR",
        }
    }
}

pub fn ingest_pdf(
    conn: &mut Connection,
    file_path: &str,
) -> Result<(IngestedDocument, PipelineRun), IngestError> {
    let (document, run) = prepare_pdf_ingestion(file_path, None)?;
    let ingested_run = db::persist_ingestion(conn, &document, &run)?;
    Ok((document, ingested_run))
}

pub(crate) fn ingest_pdf_with_profiles(
    conn: &mut Connection,
    file_path: &str,
    profile_snapshot: Option<&ModelProfileSnapshot>,
    summary_profile: SummaryProfile,
    expected_content_hash: Option<&str>,
) -> Result<(IngestedDocument, PipelineRun), IngestError> {
    let (document, run) = prepare_pdf_ingestion(file_path, None)?;
    if expected_content_hash.is_some_and(|expected| expected != document.content_hash) {
        return Err(IngestError::SourceChanged);
    }
    let ingested_run = db::persist_ingestion_with_profiles(
        conn,
        &document,
        &run,
        profile_snapshot,
        summary_profile,
    )?;
    Ok((document, ingested_run))
}

pub(crate) fn prepare_pdf_ingestion(
    file_path: &str,
    original_filename_override: Option<&str>,
) -> Result<(IngestedDocument, PipelineRun), IngestError> {
    let path = Path::new(file_path);

    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if !extension.eq_ignore_ascii_case("pdf") {
        return Err(IngestError::UnsupportedExtension);
    }

    let mut file = File::open(path)?;
    let mut signature = [0u8; 5];
    let bytes_read = file.read(&mut signature)?;
    if bytes_read < 5 || &signature != b"%PDF-" {
        return Err(IngestError::InvalidPdfSignature);
    }

    let mut hasher = Sha256::new();
    hasher.update(&signature[..bytes_read]);
    let mut byte_size = bytes_read as u64;
    let mut buffer = [0; 8192];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
        byte_size = byte_size.checked_add(count as u64).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "source byte count overflowed")
        })?;
    }
    let content_hash = format!("{:x}", hasher.finalize());
    let canonical_path = path.canonicalize()?;
    let local_source_path = canonical_path
        .to_str()
        .ok_or(IngestError::InvalidSourcePath)?
        .to_string();
    let original_filename = match original_filename_override {
        Some(name) if valid_original_filename(name) => name.to_string(),
        Some(_) => return Err(IngestError::InvalidFilename),
        None => path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(IngestError::InvalidFilename)?
            .to_string(),
    };
    let now = Utc::now();

    let document = IngestedDocument {
        document_id: Uuid::new_v4().to_string(),
        original_filename,
        file_type: "pdf".to_string(),
        byte_size,
        content_hash,
        local_source_path,
        created_at: now,
    };

    let run = prepare_received_run(document.document_id.clone(), now);
    Ok((document, run))
}

pub(crate) fn prepare_received_run(document_id: String, created_at: DateTime<Utc>) -> PipelineRun {
    PipelineRun {
        run_id: Uuid::new_v4().to_string(),
        document_id,
        state: PipelineState::Received,
        state_version: 1,
        pipeline_version: "1.0".to_string(),
        created_at,
        started_at: None,
        updated_at: created_at,
        completed_at: None,
        current_stage: None,
        progress: PipelineProgress {
            total_units: 0,
            completed_units: 0,
            failed_units: 0,
        },
        warnings: vec![],
        failure: None,
        cancellation_requested: false,
        resumable: true,
    }
}

fn valid_original_filename(name: &str) -> bool {
    let trimmed = name.trim();
    !trimmed.is_empty()
        && trimmed.len() <= 255
        && trimmed != "."
        && trimmed != ".."
        && !trimmed.contains(['/', '\\', '\0'])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::db::init_db;
    use std::fs::{self, File};
    use std::io::Write;

    struct TestDirectory(std::path::PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("doc-sum-ingest-{}", Uuid::new_v4()));
            fs::create_dir(&path).expect("test directory should be unique and writable");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn test_directory_cleanup_is_isolated_on_return_and_unwind() {
        let neighbor = TestDirectory::new();
        let neighbor_file = neighbor.0.join("keep.pdf");
        fs::write(&neighbor_file, b"neighbor").unwrap();
        let normal = TestDirectory::new();
        let normal_path = normal.0.clone();
        assert_ne!(normal_path, neighbor.0);
        fs::write(normal_path.join("source.pdf"), b"fixture").unwrap();
        fs::write(normal_path.join("test.db-journal"), b"sidecar").unwrap();
        drop(normal);
        assert!(!normal_path.exists());

        let unwinding = TestDirectory::new();
        let unwind_path = unwinding.0.clone();
        let result = std::panic::catch_unwind(move || {
            let _directory = unwinding;
            fs::write(_directory.0.join("test.db"), b"database").unwrap();
            panic!("injected test failure");
        });
        assert!(result.is_err());
        assert!(!unwind_path.exists());
        assert_eq!(fs::read(neighbor_file).unwrap(), b"neighbor");
    }

    #[test]
    fn test_ingest_valid_pdf() {
        let directory = TestDirectory::new();
        let source_path = directory.0.join("test_doc.pdf");
        let test_path = source_path.to_str().unwrap();
        let mut conn = init_db(":memory:").unwrap();
        let source_bytes = b"%PDF-1.4 mock content";
        let mut file = File::create(test_path).unwrap();
        file.write_all(source_bytes).unwrap();
        drop(file);

        let (doc, run) = ingest_pdf(&mut conn, test_path).expect("Should ingest PDF");

        assert_eq!(doc.original_filename, "test_doc.pdf");
        assert_eq!(doc.file_type, "pdf");
        assert_eq!(doc.byte_size, source_bytes.len() as u64);
        let expected_hash = format!("{:x}", Sha256::digest(source_bytes));
        assert_eq!(doc.content_hash, expected_hash);
        assert_eq!(fs::read(test_path).unwrap(), source_bytes);
        assert_eq!(
            doc.local_source_path,
            fs::canonicalize(test_path).unwrap().to_str().unwrap()
        );

        assert_eq!(run.state, PipelineState::Ingested);
        assert_eq!(run.state_version, 3);
        assert_eq!(run.document_id, doc.document_id);

        let events = db::list_pipeline_events(&conn, &run.run_id).unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].previous_state, None);
        assert_eq!(events[0].next_state, PipelineState::Received);
    }

    #[test]
    fn expected_hash_rejects_changed_source_before_persistence_and_accepts_match() {
        let directory = TestDirectory::new();
        let source_path = directory.0.join("suggested.pdf");
        let test_path = source_path.to_str().unwrap();
        let source_bytes = b"%PDF-1.4 suggested content";
        fs::write(test_path, source_bytes).unwrap();
        let expected_hash = format!("{:x}", Sha256::digest(source_bytes));
        let mut conn = init_db(":memory:").unwrap();

        let error = ingest_pdf_with_profiles(
            &mut conn,
            test_path,
            None,
            SummaryProfile::General,
            Some("different-content-hash"),
        )
        .unwrap_err();
        assert!(matches!(error, IngestError::SourceChanged));
        for table in ["documents", "pipeline_runs", "pipeline_events"] {
            let count: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 0, "{table} must remain empty after mismatch");
        }

        let (document, run) = ingest_pdf_with_profiles(
            &mut conn,
            test_path,
            None,
            SummaryProfile::General,
            Some(&expected_hash),
        )
        .unwrap();
        assert_eq!(document.content_hash, expected_hash);
        assert_eq!(run.state, PipelineState::Ingested);
    }

    #[test]
    fn test_duplicate_bytes_create_independent_document_and_run_identities() {
        let directory = TestDirectory::new();
        let source_path = directory.0.join("test_duplicate.pdf");
        let test_path = source_path.to_str().unwrap();
        let mut conn = init_db(":memory:").unwrap();
        let source_bytes = b"%PDF-1.4 duplicate content";
        fs::write(test_path, source_bytes).unwrap();

        let (first_document, first_run) = ingest_pdf(&mut conn, test_path).unwrap();
        let (second_document, second_run) = ingest_pdf(&mut conn, test_path).unwrap();

        assert_ne!(first_document.document_id, second_document.document_id);
        assert_ne!(first_run.run_id, second_run.run_id);
        assert_eq!(first_document.content_hash, second_document.content_hash);
        assert_eq!(first_document.byte_size, second_document.byte_size);
        assert_eq!(
            first_document.local_source_path,
            second_document.local_source_path
        );
        assert_eq!(first_run.document_id, first_document.document_id);
        assert_eq!(second_run.document_id, second_document.document_id);
        assert_eq!(
            db::list_pipeline_events(&conn, &first_run.run_id)
                .unwrap()
                .len(),
            3
        );
        assert_eq!(
            db::list_pipeline_events(&conn, &second_run.run_id)
                .unwrap()
                .len(),
            3
        );
        assert_eq!(fs::read(test_path).unwrap(), source_bytes);
    }

    #[test]
    fn test_ingestion_rolls_back_document_run_and_events_together() {
        let directory = TestDirectory::new();
        let source_path = directory.0.join("test_ingestion_rollback.pdf");
        let test_path = source_path.to_str().unwrap();
        let mut conn = init_db(":memory:").unwrap();
        conn.execute_batch(
            "CREATE TRIGGER test_fail_final_ingestion_event
             BEFORE INSERT ON pipeline_events
             WHEN NEW.next_state = '\"Ingested\"'
             BEGIN
                 SELECT RAISE(ABORT, 'injected final event failure');
             END;",
        )
        .unwrap();
        fs::write(test_path, b"%PDF-1.4 rollback content").unwrap();

        let result = ingest_pdf(&mut conn, test_path);
        assert!(matches!(result, Err(IngestError::Store(_))));
        for table in ["documents", "pipeline_runs", "pipeline_events"] {
            let count: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 0, "{table} must roll back");
        }
    }

    #[test]
    fn test_ingest_missing_file() {
        let directory = TestDirectory::new();
        let source_path = directory.0.join("missing.pdf");
        let mut conn = init_db(":memory:").unwrap();
        let result = ingest_pdf(&mut conn, source_path.to_str().unwrap());
        assert!(matches!(result.unwrap_err(), IngestError::IoError(_)));
    }

    #[test]
    fn test_ingest_unsupported_file() {
        let directory = TestDirectory::new();
        let source_path = directory.0.join("test_doc.txt");
        let test_path = source_path.to_str().unwrap();
        let mut conn = init_db(":memory:").unwrap();
        let mut file = File::create(test_path).unwrap();
        file.write_all(b"%PDF-1.4 mock content").unwrap(); // Has signature but wrong extension

        let result = ingest_pdf(&mut conn, test_path);
        assert!(matches!(
            result.unwrap_err(),
            IngestError::UnsupportedExtension
        ));
    }

    #[test]
    fn test_ingest_bad_signature() {
        let directory = TestDirectory::new();
        let source_path = directory.0.join("test_bad_sig.pdf");
        let test_path = source_path.to_str().unwrap();
        let mut conn = init_db(":memory:").unwrap();
        let mut file = File::create(test_path).unwrap();
        file.write_all(b"NOT A PDF content").unwrap(); // Right extension but bad signature

        let result = ingest_pdf(&mut conn, test_path);
        assert!(matches!(
            result.unwrap_err(),
            IngestError::InvalidPdfSignature
        ));

        // Verify no run was stranded
        let mut stmt = conn.prepare("SELECT count(*) FROM pipeline_runs").unwrap();
        let count: i64 = stmt.query_row([], |r| r.get(0)).unwrap();
        assert_eq!(
            count, 0,
            "No run should be created or stranded on validation failure"
        );
    }

    #[test]
    fn test_ingest_persistence_across_database_connection_reopen() {
        let directory = TestDirectory::new();
        let database_path = directory.0.join("test_persistence.db");
        let db_path = database_path.to_str().unwrap();
        let source_path = directory.0.join("test_persist_doc.pdf");
        let test_path = source_path.to_str().unwrap();
        let source_bytes = b"%PDF-1.4 mock content persistence";

        let mut file = File::create(test_path).unwrap();
        file.write_all(source_bytes).unwrap();
        drop(file);

        let expected_document;
        let expected_run;
        let expected_events;

        {
            let mut conn = init_db(db_path).unwrap();
            let (doc, run) = ingest_pdf(&mut conn, test_path).expect("Should ingest PDF");
            expected_events = db::list_pipeline_events(&conn, &run.run_id).unwrap();
            expected_document = doc;
            expected_run = run;
        }

        {
            let conn = init_db(db_path).unwrap();
            let document = db::get_document(&conn, &expected_document.document_id)
                .unwrap()
                .unwrap();
            let run = db::get_pipeline_run(&conn, &expected_run.run_id)
                .unwrap()
                .unwrap();
            let events = db::list_pipeline_events(&conn, &expected_run.run_id).unwrap();

            assert_eq!(document.document_id, expected_document.document_id);
            assert_eq!(
                document.original_filename,
                expected_document.original_filename
            );
            assert_eq!(document.byte_size, source_bytes.len() as u64);
            assert_eq!(document.byte_size, expected_document.byte_size);
            assert_eq!(document.content_hash, expected_document.content_hash);
            assert_eq!(
                document.local_source_path,
                expected_document.local_source_path
            );
            assert_eq!(document.created_at, expected_document.created_at);
            assert_eq!(run.state, PipelineState::Ingested);
            assert_eq!(run.state_version, expected_run.state_version);
            assert_eq!(run.document_id, document.document_id);
            assert_eq!(events.len(), expected_events.len());
            for (persisted, expected) in events.iter().zip(expected_events.iter()) {
                assert_eq!(persisted.event_id, expected.event_id);
                assert_eq!(persisted.sequence_no, expected.sequence_no);
                assert_eq!(persisted.previous_state, expected.previous_state);
                assert_eq!(persisted.next_state, expected.next_state);
                assert_eq!(persisted.timestamp, expected.timestamp);
            }
        }
    }
}
