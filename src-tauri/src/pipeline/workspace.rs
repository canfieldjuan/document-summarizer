use crate::pipeline::contracts::{
    CompletedSummary, ModelRuntime, ModelRuntimeFailure, PipelineFailure, PipelineRun,
    PipelineState, PipelineWarning, SummaryArtifact,
};
use crate::pipeline::db::{self, StoreError};
use crate::pipeline::model::OllamaRuntime;
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use serde::Serialize;
use thiserror::Error;

pub const RECENT_RUN_LIMIT: u32 = 30;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    pub provider_name: String,
    pub ready: bool,
    pub runtime_id: Option<String>,
    pub model_id: Option<String>,
    pub code: Option<String>,
    pub message: String,
    pub recoverable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunHistoryItem {
    pub run_id: String,
    pub document_id: String,
    pub original_filename: String,
    pub byte_size: u64,
    pub state: PipelineState,
    pub state_version: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub warnings: Vec<PipelineWarning>,
    pub failure: Option<PipelineFailure>,
    pub has_summary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedSummary {
    pub run: RunHistoryItem,
    pub summary: SummaryView,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryView {
    pub text: String,
    pub warnings: Vec<PipelineWarning>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletedSummaryView {
    pub run_id: String,
    pub original_filename: String,
    pub byte_size: u64,
    pub summary: SummaryView,
}

impl From<SummaryArtifact> for SummaryView {
    fn from(summary: SummaryArtifact) -> Self {
        Self {
            text: summary.text,
            warnings: summary.warnings,
            created_at: summary.created_at,
        }
    }
}

impl From<CompletedSummary> for CompletedSummaryView {
    fn from(completed: CompletedSummary) -> Self {
        Self {
            run_id: completed.run_id,
            original_filename: completed.document.original_filename,
            byte_size: completed.document.byte_size,
            summary: completed.summary.into(),
        }
    }
}

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("Summary artifact not found for pipeline run: {0}")]
    SummaryNotFound(String),
    #[error("Pipeline run {run_id} and its summary artifact have inconsistent completion state")]
    SummaryStateMismatch { run_id: String },
}

impl WorkspaceError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Store(_) => "PIPELINE_STORE_ERROR",
            Self::SummaryNotFound(_) => "SUMMARY_NOT_FOUND",
            Self::SummaryStateMismatch { .. } => "SUMMARY_STATE_MISMATCH",
        }
    }
}

pub fn ollama_runtime_status() -> RuntimeStatus {
    match OllamaRuntime::from_environment() {
        Ok(runtime) => assess_runtime(&runtime),
        Err(failure) => unavailable_runtime_status(None, None, failure),
    }
}

pub fn list_recent_runs(conn: &Connection) -> Result<Vec<RunHistoryItem>, WorkspaceError> {
    db::list_recent_pipeline_runs(conn, RECENT_RUN_LIMIT)?
        .into_iter()
        .map(|run| {
            let document = db::get_document(conn, &run.document_id)?
                .ok_or_else(|| StoreError::DocumentNotFound(run.document_id.clone()))?;
            let has_summary = db::summary_artifact_exists(conn, &run.run_id)?;
            validate_summary_state(&run, has_summary)?;
            Ok(RunHistoryItem {
                run_id: run.run_id,
                document_id: document.document_id,
                original_filename: document.original_filename,
                byte_size: document.byte_size,
                state: run.state,
                state_version: run.state_version,
                created_at: run.created_at,
                updated_at: run.updated_at,
                completed_at: run.completed_at,
                warnings: run.warnings,
                failure: run.failure,
                has_summary,
            })
        })
        .collect()
}

pub fn get_persisted_summary(
    conn: &Connection,
    run_id: &str,
) -> Result<PersistedSummary, WorkspaceError> {
    let run = db::get_pipeline_run(conn, run_id)?
        .ok_or_else(|| StoreError::RunNotFound(run_id.to_string()))?;
    let document = db::get_document(conn, &run.document_id)?
        .ok_or_else(|| StoreError::DocumentNotFound(run.document_id.clone()))?;
    let summary = db::get_summary_artifact(conn, run_id)?
        .ok_or_else(|| WorkspaceError::SummaryNotFound(run_id.to_string()))?;
    validate_summary_state(&run, true)?;

    Ok(PersistedSummary {
        run: RunHistoryItem {
            run_id: run.run_id,
            document_id: document.document_id,
            original_filename: document.original_filename,
            byte_size: document.byte_size,
            state: run.state,
            state_version: run.state_version,
            created_at: run.created_at,
            updated_at: run.updated_at,
            completed_at: run.completed_at,
            warnings: run.warnings,
            failure: run.failure,
            has_summary: true,
        },
        summary: summary.into(),
    })
}

fn assess_runtime(runtime: &dyn ModelRuntime) -> RuntimeStatus {
    match runtime.health() {
        Ok(()) => RuntimeStatus {
            provider_name: "Ollama".to_string(),
            ready: true,
            runtime_id: Some(runtime.runtime_id().to_string()),
            model_id: Some(runtime.model_id().to_string()),
            code: None,
            message: "Ollama is ready for local summarization.".to_string(),
            recoverable: false,
        },
        Err(failure) => unavailable_runtime_status(
            Some(runtime.runtime_id().to_string()),
            Some(runtime.model_id().to_string()),
            failure,
        ),
    }
}

fn unavailable_runtime_status(
    runtime_id: Option<String>,
    model_id: Option<String>,
    failure: ModelRuntimeFailure,
) -> RuntimeStatus {
    RuntimeStatus {
        provider_name: "Ollama".to_string(),
        ready: false,
        runtime_id,
        model_id,
        code: Some(failure.code),
        message: failure.message,
        recoverable: failure.recoverable,
    }
}

fn validate_summary_state(run: &PipelineRun, has_summary: bool) -> Result<(), WorkspaceError> {
    let is_complete = matches!(
        run.state,
        PipelineState::Complete | PipelineState::CompleteWithWarnings
    );
    if has_summary != is_complete {
        return Err(WorkspaceError::SummaryStateMismatch {
            run_id: run.run_id.clone(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::chunk::DeterministicDocumentChunker;
    use crate::pipeline::contracts::{CompletedSummary, ModelRequest, ModelResponse};
    use crate::pipeline::db::init_db;
    use crate::pipeline::ingest::ingest_pdf;
    use crate::pipeline::normalize::CanonicalNormalizer;
    use crate::pipeline::parser::PdfExtractParser;
    use crate::pipeline::service::{process_pdf_to_summary, SummaryComponents};
    use crate::pipeline::structure::DeterministicStructureInterpreter;
    use std::fs;
    use std::path::PathBuf;
    use uuid::Uuid;

    struct TestDatabase(PathBuf);

    impl TestDatabase {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("doc-sum-workspace-{}.db", Uuid::new_v4())))
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
            let text = if request.system_prompt.contains("synthesize chunk notes") {
                "Persisted workspace summary."
            } else {
                "Persisted workspace source notes."
            };
            Ok(ModelResponse {
                text: text.to_string(),
                runtime_id: self.runtime_id().to_string(),
                model_id: self.model_id().to_string(),
            })
        }

        fn health(&self) -> Result<(), ModelRuntimeFailure> {
            Ok(())
        }

        fn runtime_id(&self) -> &str {
            "workspace-fixture-runtime"
        }

        fn model_id(&self) -> &str {
            "workspace-fixture-model"
        }
    }

    struct UnavailableRuntime;

    impl ModelRuntime for UnavailableRuntime {
        fn generate(&self, _request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
            unreachable!("runtime readiness must not generate")
        }

        fn health(&self) -> Result<(), ModelRuntimeFailure> {
            Err(ModelRuntimeFailure {
                code: "MODEL_RUNTIME_UNAVAILABLE".to_string(),
                message: "Ollama is not listening.".to_string(),
                recoverable: true,
            })
        }

        fn runtime_id(&self) -> &str {
            "unavailable-runtime"
        }

        fn model_id(&self) -> &str {
            "unavailable-model"
        }
    }

    fn fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/structured_report.pdf")
    }

    fn complete_fixture_summary(conn: &mut Connection, source: &str) -> CompletedSummary {
        let parser = PdfExtractParser::new();
        let normalizer = CanonicalNormalizer::new();
        let interpreter = DeterministicStructureInterpreter::new();
        let chunker = DeterministicDocumentChunker::new();
        process_pdf_to_summary(
            conn,
            source,
            SummaryComponents {
                parser: &parser,
                normalizer: &normalizer,
                interpreter: &interpreter,
                chunker: &chunker,
                runtime: &FixtureRuntime,
            },
        )
        .expect("fixture summary should complete")
    }

    #[test]
    fn runtime_status_distinguishes_ready_and_unavailable_without_generating() {
        let ready = assess_runtime(&FixtureRuntime);
        assert!(ready.ready);
        assert_eq!(ready.provider_name, "Ollama");
        assert_eq!(ready.model_id.as_deref(), Some("workspace-fixture-model"));
        assert!(ready.code.is_none());

        let unavailable = assess_runtime(&UnavailableRuntime);
        assert!(!unavailable.ready);
        assert_eq!(
            unavailable.code.as_deref(),
            Some("MODEL_RUNTIME_UNAVAILABLE")
        );
        assert!(unavailable.recoverable);
    }

    #[test]
    fn completed_summary_history_survives_an_independent_database_reopen() {
        let database = TestDatabase::new();
        let source = fixture_path();
        let source = source.to_str().expect("fixture path should be UTF-8");

        let expected = {
            let mut conn = init_db(&database.0).expect("database should initialize");
            let completed = complete_fixture_summary(&mut conn, source);
            let history = list_recent_runs(&conn).expect("history should load");
            assert_eq!(history.len(), 1);
            assert_eq!(history[0].run_id, completed.run_id);
            assert!(history[0].has_summary);
            get_persisted_summary(&conn, &completed.run_id).expect("persisted summary should load")
        };

        let reopened = init_db(&database.0).expect("database should reopen independently");
        let history = list_recent_runs(&reopened).expect("reopened history should load");
        let actual = get_persisted_summary(&reopened, &expected.run.run_id)
            .expect("reopened summary should pass integrity checks");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0], expected.run);
        assert_eq!(actual, expected);
    }

    #[test]
    fn recent_run_order_is_deterministic_when_update_times_match() {
        let mut conn = init_db(":memory:").expect("database should initialize");
        let source = fixture_path();
        let source = source.to_str().expect("fixture path should be UTF-8");
        let first = complete_fixture_summary(&mut conn, source);
        let second = complete_fixture_summary(&mut conn, source);
        conn.execute(
            "UPDATE pipeline_runs SET updated_at = '2026-08-29T12:00:00+00:00'",
            [],
        )
        .expect("test timestamps should align");

        let mut expected = vec![first.run_id, second.run_id];
        expected.sort_by(|left, right| right.cmp(left));
        let actual = list_recent_runs(&conn)
            .expect("history should load")
            .into_iter()
            .map(|run| run.run_id)
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }

    #[test]
    fn corrupt_summary_does_not_hide_history_but_fails_closed_when_opened() {
        let mut conn = init_db(":memory:").expect("database should initialize");
        let source = fixture_path();
        let completed = complete_fixture_summary(
            &mut conn,
            source.to_str().expect("fixture path should be UTF-8"),
        );
        conn.execute(
            "UPDATE summary_artifacts SET summary_artifact = '{}' WHERE run_id = ?1",
            [&completed.run_id],
        )
        .expect("test artifact should be corrupted");

        let history = list_recent_runs(&conn).expect("metadata history should remain available");
        assert_eq!(history.len(), 1);
        assert!(history[0].has_summary);
        let error = get_persisted_summary(&conn, &completed.run_id)
            .expect_err("corrupt summary must fail its integrity check");
        assert!(matches!(
            error,
            WorkspaceError::Store(StoreError::DownstreamArtifactIntegrityMismatch { .. })
        ));
    }

    #[test]
    fn incomplete_run_is_visible_but_cannot_masquerade_as_a_summary() {
        let mut conn = init_db(":memory:").expect("database should initialize");
        let source = fixture_path();
        let (_, run) = ingest_pdf(
            &mut conn,
            source.to_str().expect("fixture path should be UTF-8"),
        )
        .expect("fixture should ingest");

        let history = list_recent_runs(&conn).expect("history should load");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].state, PipelineState::Ingested);
        assert!(!history[0].has_summary);

        let error = get_persisted_summary(&conn, &run.run_id)
            .expect_err("an incomplete run must not return a summary");
        assert_eq!(error.code(), "SUMMARY_NOT_FOUND");
    }

    #[test]
    fn completed_summary_view_includes_display_data_and_hides_private_pipeline_fields() {
        let mut conn = init_db(":memory:").expect("database should initialize");
        let source = fixture_path();
        let completed = complete_fixture_summary(
            &mut conn,
            source.to_str().expect("fixture path should be UTF-8"),
        );
        let expected_filename = completed.document.original_filename.clone();
        let expected_summary = completed.summary.text.clone();

        let serialized = serde_json::to_value(CompletedSummaryView::from(completed))
            .expect("presentation result should serialize");
        assert_eq!(serialized["originalFilename"], expected_filename);
        assert_eq!(serialized["summary"]["text"], expected_summary);
        assert!(serialized.get("document").is_none());
        assert!(serialized.get("localSourcePath").is_none());
        assert!(serialized["summary"].get("integrityHash").is_none());
        assert!(serialized["summary"].get("documentId").is_none());
    }
}
