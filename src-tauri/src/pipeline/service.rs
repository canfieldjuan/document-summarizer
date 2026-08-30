use crate::pipeline::chunk::{chunk_document, ChunkPipelineError};
use crate::pipeline::contracts::{
    CompletedSummary, DocumentChunker, DocumentNormalizer, DocumentParser, ModelRuntime,
    StructureInterpreter,
};
use crate::pipeline::ingest::prepare_received_run;
use crate::pipeline::ingest::{ingest_pdf, IngestError};
use crate::pipeline::normalize::{normalize_document, NormalizePipelineError};
use crate::pipeline::parser::{parse_document, parse_started_document, ParsePipelineError};
use crate::pipeline::structure::{structure_document, StructurePipelineError};
use crate::pipeline::summary::{summarize_chunked_document, SummaryPipelineError};
use crate::pipeline::{
    contracts::PipelineState,
    db::{self, StoreError},
};
use chrono::Utc;
use rusqlite::Connection;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DocumentServiceError {
    #[error(transparent)]
    Ingest(#[from] IngestError),
    #[error(transparent)]
    Parse(#[from] ParsePipelineError),
    #[error(transparent)]
    Normalize(#[from] NormalizePipelineError),
    #[error(transparent)]
    Structure(#[from] StructurePipelineError),
    #[error(transparent)]
    Chunk(#[from] ChunkPipelineError),
    #[error(transparent)]
    Summary(#[from] SummaryPipelineError),
    #[error(transparent)]
    Retry(#[from] RetryPipelineError),
}

impl DocumentServiceError {
    pub fn code(&self) -> &str {
        match self {
            Self::Ingest(error) => error.code(),
            Self::Parse(error) => error.code(),
            Self::Normalize(error) => error.code(),
            Self::Structure(error) => error.code(),
            Self::Chunk(error) => error.code(),
            Self::Summary(error) => error.code(),
            Self::Retry(error) => error.code(),
        }
    }
}

#[derive(Debug, Error)]
pub enum RetryPipelineError {
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl RetryPipelineError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Store(StoreError::RunNotFound(_)) => "RETRY_SOURCE_NOT_FOUND",
            Self::Store(StoreError::InvalidRetrySource { .. }) => "RETRY_NOT_ALLOWED",
            Self::Store(StoreError::RetryAlreadyExists { .. }) => "RETRY_ALREADY_EXISTS",
            Self::Store(StoreError::Transition(_)) | Self::Store(StoreError::StaleWrite { .. }) => {
                "RETRY_STALE_STATE"
            }
            Self::Store(_) => "PIPELINE_STORE_ERROR",
        }
    }
}

pub struct SummaryComponents<'a> {
    pub parser: &'a dyn DocumentParser,
    pub normalizer: &'a dyn DocumentNormalizer,
    pub interpreter: &'a dyn StructureInterpreter,
    pub chunker: &'a dyn DocumentChunker,
    pub runtime: &'a dyn ModelRuntime,
}

pub fn process_pdf_to_summary(
    conn: &mut Connection,
    file_path: &str,
    components: SummaryComponents<'_>,
) -> Result<CompletedSummary, DocumentServiceError> {
    let (document, run) = ingest_pdf(conn, file_path)?;
    let summary = process_ingested_to_summary(conn, &run.run_id, components)?;
    Ok(CompletedSummary {
        run_id: run.run_id,
        document,
        summary: summary.summary,
        citations: summary.citations,
    })
}

pub fn retry_failed_run_to_summary(
    conn: &mut Connection,
    source_run_id: &str,
    expected_source_version: u32,
    components: SummaryComponents<'_>,
) -> Result<CompletedSummary, DocumentServiceError> {
    let source_run = db::get_pipeline_run(conn, source_run_id)
        .map_err(RetryPipelineError::from)?
        .ok_or_else(|| StoreError::RunNotFound(source_run_id.to_string()))
        .map_err(RetryPipelineError::from)?;
    if source_run.state != PipelineState::Failed {
        return Err(RetryPipelineError::Store(StoreError::InvalidRetrySource {
            run_id: source_run_id.to_string(),
            reason: "only a failed run can create a retry".to_string(),
        })
        .into());
    }
    let retry_run = prepare_received_run(source_run.document_id.clone(), Utc::now());
    let (retry_run, document, _) =
        db::create_retry_run(conn, source_run_id, expected_source_version, &retry_run)
            .map_err(RetryPipelineError::from)?;
    parse_started_document(
        conn,
        components.parser,
        &retry_run.run_id,
        retry_run.state_version,
        &document,
    )?;
    let summary = process_parsed_to_summary(conn, &retry_run.run_id, components)?;
    Ok(CompletedSummary {
        run_id: retry_run.run_id,
        document,
        summary: summary.summary,
        citations: summary.citations,
    })
}

pub fn process_ingested_to_summary(
    conn: &mut Connection,
    run_id: &str,
    components: SummaryComponents<'_>,
) -> Result<crate::pipeline::contracts::SummaryArtifacts, DocumentServiceError> {
    parse_document(conn, components.parser, run_id)?;
    process_parsed_to_summary(conn, run_id, components)
}

fn process_parsed_to_summary(
    conn: &mut Connection,
    run_id: &str,
    components: SummaryComponents<'_>,
) -> Result<crate::pipeline::contracts::SummaryArtifacts, DocumentServiceError> {
    normalize_document(conn, components.normalizer, run_id)?;
    structure_document(conn, components.interpreter, run_id)?;
    chunk_document(conn, components.chunker, run_id)?;
    Ok(summarize_chunked_document(
        conn,
        components.runtime,
        run_id,
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::chunk::DeterministicDocumentChunker;
    use crate::pipeline::contracts::{
        ModelRequest, ModelResponse, ModelRuntimeFailure, PipelineStage, PipelineState,
        RetryCheckpoint,
    };
    use crate::pipeline::db::{
        get_citation_artifact, get_normalized_document, get_pipeline_run,
        get_retry_lineage_for_retry, get_retry_lineage_for_source, get_summary_artifact,
        get_synthesized_document, init_db, list_pipeline_events,
    };
    use crate::pipeline::model::OllamaRuntime;
    use crate::pipeline::normalize::CanonicalNormalizer;
    use crate::pipeline::parser::PdfExtractParser;
    use crate::pipeline::structure::DeterministicStructureInterpreter;
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;
    use uuid::Uuid;

    struct TestDatabase(PathBuf);

    impl TestDatabase {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("doc-sum-live-citation-{}.db", Uuid::new_v4())))
        }
    }

    struct TestSource(PathBuf);

    impl TestSource {
        fn from_fixture() -> Self {
            let path =
                std::env::temp_dir().join(format!("doc-sum-retry-source-{}.pdf", Uuid::new_v4()));
            fs::copy(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/structured_report.pdf"),
                &path,
            )
            .expect("fixture copy should succeed");
            Self(path)
        }
    }

    impl Drop for TestSource {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    impl Drop for TestDatabase {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    struct FixtureRuntime;

    impl ModelRuntime for FixtureRuntime {
        fn generate(&self, request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
            Ok(ModelResponse {
                text: crate::pipeline::summary::fixture_model_output(request),
                runtime_id: self.runtime_id().to_string(),
                model_id: self.model_id().to_string(),
            })
        }

        fn health(&self) -> Result<(), ModelRuntimeFailure> {
            Ok(())
        }

        fn runtime_id(&self) -> &str {
            "fixture-runtime"
        }

        fn model_id(&self) -> &str {
            "fixture-model"
        }
    }

    struct RecoverableFailureRuntime;

    impl ModelRuntime for RecoverableFailureRuntime {
        fn generate(&self, _request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
            Err(ModelRuntimeFailure {
                code: "FIXTURE_RUNTIME_INTERRUPTED".to_string(),
                message: "Fixture runtime stopped before producing output.".to_string(),
                recoverable: true,
            })
        }

        fn health(&self) -> Result<(), ModelRuntimeFailure> {
            Ok(())
        }

        fn runtime_id(&self) -> &str {
            "recoverable-failure-runtime"
        }

        fn model_id(&self) -> &str {
            "fixture-model"
        }
    }

    #[derive(Default)]
    struct TestPipeline {
        parser: PdfExtractParser,
        normalizer: CanonicalNormalizer,
        interpreter: DeterministicStructureInterpreter,
        chunker: DeterministicDocumentChunker,
    }

    impl TestPipeline {
        fn components<'a>(&'a self, runtime: &'a dyn ModelRuntime) -> SummaryComponents<'a> {
            SummaryComponents {
                parser: &self.parser,
                normalizer: &self.normalizer,
                interpreter: &self.interpreter,
                chunker: &self.chunker,
                runtime,
            }
        }
    }

    fn create_recoverable_failed_run(
        conn: &mut Connection,
        source: &TestSource,
        pipeline: &TestPipeline,
    ) -> crate::pipeline::contracts::PipelineRun {
        let (_, ingested) = ingest_pdf(
            conn,
            source.0.to_str().expect("fixture path should be UTF-8"),
        )
        .expect("fixture should ingest");
        let failure = process_ingested_to_summary(
            conn,
            &ingested.run_id,
            pipeline.components(&RecoverableFailureRuntime),
        )
        .expect_err("fixture runtime should fail analysis");
        assert_eq!(failure.code(), "FIXTURE_RUNTIME_INTERRUPTED");
        let failed = get_pipeline_run(conn, &ingested.run_id)
            .expect("failed run should load")
            .expect("failed run should exist");
        assert_eq!(failed.state, PipelineState::Failed);
        assert_eq!(
            failed.failure.as_ref().and_then(|item| item.stage.as_ref()),
            Some(&PipelineStage::Analyze)
        );
        failed
    }

    #[test]
    fn application_service_runs_the_real_pdf_pipeline_to_a_durable_summary() {
        let source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/structured_report.pdf");
        let mut conn = init_db(":memory:").expect("schema should initialize");
        let parser = PdfExtractParser::new();
        let normalizer = CanonicalNormalizer::new();
        let interpreter = DeterministicStructureInterpreter::new();
        let chunker = DeterministicDocumentChunker::new();
        let result = process_pdf_to_summary(
            &mut conn,
            source.to_str().expect("fixture path should be UTF-8"),
            SummaryComponents {
                parser: &parser,
                normalizer: &normalizer,
                interpreter: &interpreter,
                chunker: &chunker,
                runtime: &FixtureRuntime,
            },
        )
        .expect("real PDF path should complete");

        assert_eq!(
            get_pipeline_run(&conn, &result.run_id)
                .expect("run should load")
                .expect("run should exist")
                .state,
            PipelineState::CompleteWithWarnings
        );
        assert_eq!(
            get_summary_artifact(&conn, &result.run_id)
                .expect("summary should load")
                .expect("summary should exist"),
            result.summary
        );
        assert_eq!(
            get_citation_artifact(&conn, &result.run_id)
                .expect("citations should load")
                .expect("citations should exist"),
            result.citations
        );
    }

    #[test]
    fn retry_creates_a_new_traceable_run_and_preserves_the_failed_parent_across_reopen() {
        let database = TestDatabase::new();
        let source = TestSource::from_fixture();
        let pipeline = TestPipeline::default();
        let (parent, parent_events, completed, completed_events) = {
            let mut conn = init_db(&database.0).expect("schema should initialize");
            let parent = create_recoverable_failed_run(&mut conn, &source, &pipeline);
            let parent_events =
                list_pipeline_events(&conn, &parent.run_id).expect("parent events should load");
            let before = crate::pipeline::workspace::list_recent_runs(&conn)
                .expect("history should load before retry");
            let parent_before = before
                .iter()
                .find(|item| item.run_id == parent.run_id)
                .expect("parent history should exist");
            assert!(parent_before.can_retry);
            assert!(parent_before.retry_run_id.is_none());
            let serialized =
                serde_json::to_value(parent_before).expect("history contract should serialize");
            assert_eq!(serialized["canRetry"], true);
            assert!(serialized.get("retryOfRunId").is_some());
            assert!(serialized.get("retryRunId").is_some());

            let completed = retry_failed_run_to_summary(
                &mut conn,
                &parent.run_id,
                parent.state_version,
                pipeline.components(&FixtureRuntime),
            )
            .expect("retry should complete from the ingested checkpoint");
            assert_ne!(completed.run_id, parent.run_id);
            assert_eq!(completed.document.document_id, parent.document_id);
            assert_eq!(
                get_pipeline_run(&conn, &parent.run_id)
                    .expect("parent should reload")
                    .expect("parent should exist"),
                parent
            );
            assert_eq!(
                list_pipeline_events(&conn, &parent.run_id).expect("parent events should reload"),
                parent_events
            );
            assert_eq!(
                conn.query_row("SELECT COUNT(*) FROM documents", [], |row| row
                    .get::<_, u32>(0))
                    .expect("document count should load"),
                1
            );

            let lineage = get_retry_lineage_for_retry(&conn, &completed.run_id)
                .expect("lineage should load")
                .expect("lineage should exist");
            assert_eq!(lineage.source_run_id, parent.run_id);
            assert_eq!(lineage.checkpoint, RetryCheckpoint::Ingested);
            assert_eq!(
                get_retry_lineage_for_source(&conn, &parent.run_id)
                    .expect("child lineage should load")
                    .expect("child lineage should exist"),
                lineage
            );

            let completed_events =
                list_pipeline_events(&conn, &completed.run_id).expect("retry events should load");
            assert_eq!(
                completed_events[0].reason.as_deref(),
                Some("retry_run_created")
            );
            assert_eq!(
                completed_events[1].reason.as_deref(),
                Some("retry_checkpoint_reused")
            );
            assert_eq!(
                completed_events[2].reason.as_deref(),
                Some("retry_checkpoint_reused")
            );
            assert_eq!(
                completed_events[3].reason.as_deref(),
                Some("retry_processing_started")
            );

            let history = crate::pipeline::workspace::list_recent_runs(&conn)
                .expect("history should load after retry");
            let parent_item = history
                .iter()
                .find(|item| item.run_id == parent.run_id)
                .expect("parent history should remain");
            let child_item = history
                .iter()
                .find(|item| item.run_id == completed.run_id)
                .expect("child history should exist");
            assert!(!parent_item.can_retry);
            assert_eq!(
                parent_item.retry_run_id.as_deref(),
                Some(completed.run_id.as_str())
            );
            assert_eq!(
                child_item.retry_of_run_id.as_deref(),
                Some(parent.run_id.as_str())
            );
            assert!(!child_item.can_retry);
            (parent, parent_events, completed, completed_events)
        };

        let reopened = init_db(&database.0).expect("database should independently reopen");
        assert_eq!(
            reopened
                .query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
                .expect("quick check should run"),
            "ok"
        );
        assert_eq!(
            get_pipeline_run(&reopened, &parent.run_id)
                .expect("parent should load after reopen")
                .expect("parent should persist"),
            parent
        );
        assert_eq!(
            list_pipeline_events(&reopened, &parent.run_id).expect("parent events should persist"),
            parent_events
        );
        assert_eq!(
            list_pipeline_events(&reopened, &completed.run_id)
                .expect("retry events should persist"),
            completed_events
        );
        assert!(get_summary_artifact(&reopened, &completed.run_id)
            .expect("retry summary should load")
            .is_some());
        assert!(get_retry_lineage_for_retry(&reopened, &completed.run_id)
            .expect("retry lineage should load after reopen")
            .is_some());
    }

    #[test]
    fn committed_retry_is_active_and_restart_recovers_it_before_parser_work() {
        let database = TestDatabase::new();
        let source = TestSource::from_fixture();
        let pipeline = TestPipeline::default();
        let (parent, parent_events, retry_run_id) = {
            let mut conn = init_db(&database.0).expect("schema should initialize");
            let parent = create_recoverable_failed_run(&mut conn, &source, &pipeline);
            let parent_events =
                list_pipeline_events(&conn, &parent.run_id).expect("parent events should load");
            let retry = prepare_received_run(parent.document_id.clone(), Utc::now());
            let (parsing, document, lineage) =
                db::create_retry_run(&mut conn, &parent.run_id, parent.state_version, &retry)
                    .expect("retry creation should commit an active child");

            assert_eq!(parsing.state, PipelineState::Parsing);
            assert_eq!(parsing.state_version, 4);
            assert_eq!(parsing.current_stage, Some(PipelineStage::Parse));
            assert_eq!(document.document_id, parent.document_id);
            assert_eq!(lineage.retry_run_id, parsing.run_id);
            let child_events =
                list_pipeline_events(&conn, &parsing.run_id).expect("child events should load");
            assert_eq!(child_events.len(), 4);
            assert_eq!(
                child_events[3].previous_state,
                Some(PipelineState::Ingested)
            );
            assert_eq!(child_events[3].next_state, PipelineState::Parsing);
            assert_eq!(
                child_events[3].reason.as_deref(),
                Some("retry_processing_started")
            );
            assert_eq!(
                get_pipeline_run(&conn, &parent.run_id)
                    .expect("parent should reload")
                    .expect("parent should exist"),
                parent
            );
            assert_eq!(
                list_pipeline_events(&conn, &parent.run_id).expect("parent events should reload"),
                parent_events
            );
            (parent, parent_events, parsing.run_id)
        };

        let mut reopened = init_db(&database.0).expect("database should independently reopen");
        let recovered = crate::pipeline::recovery::reconcile_interrupted_runs(&mut reopened)
            .expect("startup recovery should reconcile the active retry");
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].run_id, retry_run_id);

        let failed_child = get_pipeline_run(&reopened, &retry_run_id)
            .expect("child should reload")
            .expect("child should exist");
        assert_eq!(failed_child.state, PipelineState::Failed);
        assert_eq!(failed_child.current_stage, Some(PipelineStage::Parse));
        assert_eq!(
            failed_child
                .failure
                .as_ref()
                .map(|failure| (failure.code.as_str(), failure.recoverable)),
            Some(("PROCESS_INTERRUPTED", true))
        );
        assert_eq!(
            failed_child.retry_checkpoint(),
            Some(RetryCheckpoint::Ingested)
        );
        assert_eq!(
            get_pipeline_run(&reopened, &parent.run_id)
                .expect("parent should reload")
                .expect("parent should exist"),
            parent
        );
        assert_eq!(
            list_pipeline_events(&reopened, &parent.run_id)
                .expect("parent events should remain immutable"),
            parent_events
        );
        let child_events =
            list_pipeline_events(&reopened, &retry_run_id).expect("child events should reload");
        assert_eq!(child_events.len(), 5);
        assert_eq!(child_events[4].previous_state, Some(PipelineState::Parsing));
        assert_eq!(child_events[4].next_state, PipelineState::Failed);

        let history = crate::pipeline::workspace::list_recent_runs(&reopened)
            .expect("history should load after recovery");
        let parent_item = history
            .iter()
            .find(|item| item.run_id == parent.run_id)
            .expect("parent history should exist");
        let child_item = history
            .iter()
            .find(|item| item.run_id == retry_run_id)
            .expect("child history should exist");
        assert!(!parent_item.can_retry);
        assert_eq!(
            parent_item.retry_run_id.as_deref(),
            Some(retry_run_id.as_str())
        );
        assert!(child_item.can_retry);
        assert_eq!(
            child_item.retry_of_run_id.as_deref(),
            Some(parent.run_id.as_str())
        );
    }

    #[test]
    fn retry_rejects_stale_nonfailed_nonrecoverable_and_duplicate_sources() {
        let source = TestSource::from_fixture();
        let pipeline = TestPipeline::default();
        let mut conn = init_db(":memory:").expect("schema should initialize");
        let parent = create_recoverable_failed_run(&mut conn, &source, &pipeline);
        let mut ingestion_failure = parent.clone();
        ingestion_failure
            .failure
            .as_mut()
            .expect("failure should exist")
            .stage = Some(PipelineStage::Ingest);
        assert!(ingestion_failure.retry_checkpoint().is_none());
        let mut stage_missing = parent.clone();
        stage_missing
            .failure
            .as_mut()
            .expect("failure should exist")
            .stage = None;
        assert!(stage_missing.retry_checkpoint().is_none());

        let stale = retry_failed_run_to_summary(
            &mut conn,
            &parent.run_id,
            parent.state_version - 1,
            pipeline.components(&FixtureRuntime),
        )
        .expect_err("stale retry should fail");
        assert_eq!(stale.code(), "RETRY_STALE_STATE");
        assert!(get_retry_lineage_for_source(&conn, &parent.run_id)
            .expect("lineage lookup should succeed")
            .is_none());

        let (_, ingested) = ingest_pdf(
            &mut conn,
            source.0.to_str().expect("fixture path should be UTF-8"),
        )
        .expect("second source should ingest");
        let nonfailed = retry_failed_run_to_summary(
            &mut conn,
            &ingested.run_id,
            ingested.state_version,
            pipeline.components(&FixtureRuntime),
        )
        .expect_err("nonfailed run should not retry");
        assert_eq!(nonfailed.code(), "RETRY_NOT_ALLOWED");

        let completed = retry_failed_run_to_summary(
            &mut conn,
            &parent.run_id,
            parent.state_version,
            pipeline.components(&FixtureRuntime),
        )
        .expect("first retry should succeed");
        let duplicate = retry_failed_run_to_summary(
            &mut conn,
            &parent.run_id,
            parent.state_version,
            pipeline.components(&FixtureRuntime),
        )
        .expect_err("source should permit only one direct retry");
        assert_eq!(duplicate.code(), "RETRY_ALREADY_EXISTS");
        assert_eq!(
            get_retry_lineage_for_source(&conn, &parent.run_id)
                .expect("lineage should load")
                .expect("lineage should exist")
                .retry_run_id,
            completed.run_id
        );

        let malformed_path =
            std::env::temp_dir().join(format!("doc-sum-nonrecoverable-{}.pdf", Uuid::new_v4()));
        fs::write(&malformed_path, b"%PDF-not-structurally-valid")
            .expect("malformed fixture should write");
        let (_, malformed_run) = ingest_pdf(
            &mut conn,
            malformed_path
                .to_str()
                .expect("malformed path should be UTF-8"),
        )
        .expect("candidate should ingest");
        process_ingested_to_summary(
            &mut conn,
            &malformed_run.run_id,
            pipeline.components(&FixtureRuntime),
        )
        .expect_err("malformed PDF should fail parsing");
        let malformed_failed = get_pipeline_run(&conn, &malformed_run.run_id)
            .expect("malformed run should load")
            .expect("malformed run should exist");
        assert!(
            !malformed_failed
                .failure
                .as_ref()
                .expect("failure should persist")
                .recoverable
        );
        let nonrecoverable = retry_failed_run_to_summary(
            &mut conn,
            &malformed_failed.run_id,
            malformed_failed.state_version,
            pipeline.components(&FixtureRuntime),
        )
        .expect_err("nonrecoverable failure should not retry");
        assert_eq!(nonrecoverable.code(), "RETRY_NOT_ALLOWED");
        fs::remove_file(malformed_path).expect("malformed fixture should remove");
    }

    #[test]
    fn retry_lineage_failure_rolls_back_run_state_and_events() {
        let source = TestSource::from_fixture();
        let pipeline = TestPipeline::default();
        let mut conn = init_db(":memory:").expect("schema should initialize");
        let parent = create_recoverable_failed_run(&mut conn, &source, &pipeline);
        let parent_events =
            list_pipeline_events(&conn, &parent.run_id).expect("parent events should load");
        let retry = prepare_received_run(parent.document_id.clone(), Utc::now());
        conn.execute_batch(&format!(
            "CREATE TRIGGER fail_retry_lineage
             BEFORE INSERT ON pipeline_run_retries
             WHEN NEW.retry_run_id = '{}'
             BEGIN
                 SELECT RAISE(ABORT, 'injected retry lineage failure');
             END;",
            retry.run_id
        ))
        .expect("failure trigger should install");

        let result = db::create_retry_run(&mut conn, &parent.run_id, parent.state_version, &retry);
        assert!(matches!(result, Err(StoreError::Sqlite(_))));
        assert!(get_pipeline_run(&conn, &retry.run_id)
            .expect("retry lookup should succeed")
            .is_none());
        assert!(get_retry_lineage_for_source(&conn, &parent.run_id)
            .expect("lineage lookup should succeed")
            .is_none());
        assert_eq!(
            get_pipeline_run(&conn, &parent.run_id)
                .expect("parent should reload")
                .expect("parent should exist"),
            parent
        );
        assert_eq!(
            list_pipeline_events(&conn, &parent.run_id).expect("parent events should reload"),
            parent_events
        );
    }

    #[test]
    fn changed_source_fails_the_child_and_restored_source_can_retry_the_child() {
        let source = TestSource::from_fixture();
        let original_bytes = fs::read(&source.0).expect("fixture should read");
        let pipeline = TestPipeline::default();
        let mut conn = init_db(":memory:").expect("schema should initialize");
        let parent = create_recoverable_failed_run(&mut conn, &source, &pipeline);
        let parent_snapshot = parent.clone();
        fs::write(&source.0, b"%PDF-changed-after-ingestion")
            .expect("source mutation should succeed");

        let changed = retry_failed_run_to_summary(
            &mut conn,
            &parent.run_id,
            parent.state_version,
            pipeline.components(&FixtureRuntime),
        )
        .expect_err("changed source should fail at the parser boundary");
        assert_eq!(changed.code(), "SOURCE_CONTENT_CHANGED");
        let first_child_id = get_retry_lineage_for_source(&conn, &parent.run_id)
            .expect("lineage should load")
            .expect("lineage should exist")
            .retry_run_id;
        let first_child = get_pipeline_run(&conn, &first_child_id)
            .expect("child should load")
            .expect("child should exist");
        assert_eq!(first_child.state, PipelineState::Failed);
        assert_eq!(
            first_child
                .failure
                .as_ref()
                .map(|failure| failure.code.as_str()),
            Some("SOURCE_CONTENT_CHANGED")
        );
        assert_eq!(
            get_pipeline_run(&conn, &parent.run_id)
                .expect("parent should reload")
                .expect("parent should exist"),
            parent_snapshot
        );

        fs::write(&source.0, original_bytes).expect("source restoration should succeed");
        let second_child = retry_failed_run_to_summary(
            &mut conn,
            &first_child.run_id,
            first_child.state_version,
            pipeline.components(&FixtureRuntime),
        )
        .expect("restored source should support a new child attempt");
        assert_eq!(second_child.document.document_id, parent.document_id);
        assert_eq!(
            get_retry_lineage_for_retry(&conn, &second_child.run_id)
                .expect("second lineage should load")
                .expect("second lineage should exist")
                .source_run_id,
            first_child.run_id
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM documents", [], |row| row
                .get::<_, u32>(0))
                .expect("document count should load"),
            1
        );
    }

    #[test]
    #[ignore = "requires the configured local Ollama runtime and selected model"]
    fn live_ollama_pipeline_persists_exact_citations_across_database_reopen() {
        let database = TestDatabase::new();
        let source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/structured_report.pdf");
        let runtime = OllamaRuntime::from_environment().expect("Ollama runtime should configure");
        let parser = PdfExtractParser::new();
        let normalizer = CanonicalNormalizer::new();
        let interpreter = DeterministicStructureInterpreter::new();
        let chunker = DeterministicDocumentChunker::new();
        let (run_id, summary_hash, citation_hash, claim_count, evidence_count) = {
            let mut conn = init_db(&database.0).expect("live database should initialize");
            let result = process_pdf_to_summary(
                &mut conn,
                source.to_str().expect("fixture path should be UTF-8"),
                SummaryComponents {
                    parser: &parser,
                    normalizer: &normalizer,
                    interpreter: &interpreter,
                    chunker: &chunker,
                    runtime: &runtime,
                },
            )
            .expect("configured Ollama should complete the real PDF pipeline");
            let normalized = get_normalized_document(&conn, &result.run_id)
                .expect("normalized artifact should load")
                .expect("normalized artifact should exist");
            let blocks = normalized
                .pages
                .iter()
                .flat_map(|page| page.content.iter())
                .map(|block| (block.block_id.as_str(), block))
                .collect::<HashMap<_, _>>();
            assert!(!result.citations.claims.is_empty());
            assert!(!result.citations.evidence.is_empty());
            for evidence in &result.citations.evidence {
                let block = blocks
                    .get(evidence.block_id.as_str())
                    .expect("live citation block should exist");
                assert!(block.text.contains(&evidence.exact_quote));
                assert_eq!(evidence.source_span, block.source);
            }
            let synthesized = get_synthesized_document(&conn, &result.run_id)
                .expect("synthesis should load")
                .expect("synthesis should exist");
            assert_eq!(synthesized.model_id, runtime.model_id());
            (
                result.run_id,
                result.summary.integrity_hash,
                result.citations.integrity_hash,
                result.citations.claims.len(),
                result.citations.evidence.len(),
            )
        };

        let reopened = init_db(&database.0).expect("live database should independently reopen");
        assert_eq!(
            reopened
                .query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
                .expect("quick check should run"),
            "ok"
        );
        let summary = get_summary_artifact(&reopened, &run_id)
            .expect("summary should load")
            .expect("summary should persist");
        let citations = get_citation_artifact(&reopened, &run_id)
            .expect("citations should load")
            .expect("citations should persist");
        assert_eq!(summary.integrity_hash, summary_hash);
        assert_eq!(citations.integrity_hash, citation_hash);
        assert_eq!(citations.summary_integrity_hash, summary.integrity_hash);
        assert_eq!(citations.claims.len(), claim_count);
        assert_eq!(citations.evidence.len(), evidence_count);
        assert_eq!(
            get_pipeline_run(&reopened, &run_id)
                .expect("run should load")
                .expect("run should persist")
                .state,
            PipelineState::CompleteWithWarnings
        );
    }
}
