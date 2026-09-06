use crate::pipeline::contracts::{
    CitationArtifact, ContinuationCheckpoint, ModelRuntime, ModelRuntimeFailure, PipelineFailure,
    PipelineRun, PipelineState, PipelineWarning, SummaryArtifact,
};
use crate::pipeline::db::{self, StoreError};
use crate::pipeline::model_settings::runtime_from_settings;
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use serde::Serialize;
use std::collections::HashMap;
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
    pub retry_of_run_id: Option<String>,
    pub retry_run_id: Option<String>,
    pub can_retry: bool,
    pub continuation_checkpoint: Option<ContinuationCheckpoint>,
    pub can_continue: bool,
    pub continuation_requires_runtime: bool,
    pub cancellation_requested: bool,
    pub background_active: bool,
    pub can_cancel: bool,
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
    pub claims: Vec<CitedClaimView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CitedClaimView {
    pub claim_id: String,
    pub text: String,
    pub citations: Vec<CitationView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CitationView {
    pub evidence_id: String,
    pub label: String,
    pub page_start: u32,
    pub page_end: u32,
    pub exact_quote: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletedSummaryView {
    pub run_id: String,
    pub original_filename: String,
    pub byte_size: u64,
    pub summary: SummaryView,
}

impl From<PersistedSummary> for CompletedSummaryView {
    fn from(persisted: PersistedSummary) -> Self {
        Self {
            run_id: persisted.run.run_id,
            original_filename: persisted.run.original_filename,
            byte_size: persisted.run.byte_size,
            summary: persisted.summary,
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
    #[error("Citation artifact is missing or inconsistent for document: {0}")]
    CitationMismatch(String),
}

impl WorkspaceError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Store(_) => "PIPELINE_STORE_ERROR",
            Self::SummaryNotFound(_) => "SUMMARY_NOT_FOUND",
            Self::SummaryStateMismatch { .. } => "SUMMARY_STATE_MISMATCH",
            Self::CitationMismatch(_) => "CITATION_ARTIFACT_MISMATCH",
        }
    }
}

pub fn ollama_runtime_status(settings_path: &std::path::Path) -> RuntimeStatus {
    match runtime_from_settings(settings_path) {
        Ok(runtime) => assess_runtime(&runtime),
        Err(failure) => unavailable_runtime_status(None, None, failure),
    }
}

pub fn list_recent_runs(conn: &Connection) -> Result<Vec<RunHistoryItem>, WorkspaceError> {
    db::list_recent_pipeline_runs(conn, RECENT_RUN_LIMIT)?
        .into_iter()
        .map(|run| run_history_item_from_run(conn, run))
        .collect()
}

pub fn get_run(conn: &Connection, run_id: &str) -> Result<RunHistoryItem, WorkspaceError> {
    let run = db::get_pipeline_run(conn, run_id)?
        .ok_or_else(|| StoreError::RunNotFound(run_id.to_string()))?;
    run_history_item_from_run(conn, run)
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
    let citations = db::get_citation_artifact(conn, run_id)?;
    validate_summary_state(&run, true)?;
    validate_citations_against_sources(conn, run_id, &summary, citations.as_ref())?;
    let summary = summary_view(summary, citations)?;

    Ok(PersistedSummary {
        run: run_history_item(conn, run, document, true)?,
        summary,
    })
}

fn run_history_item(
    conn: &Connection,
    run: PipelineRun,
    document: crate::pipeline::contracts::IngestedDocument,
    has_summary: bool,
) -> Result<RunHistoryItem, WorkspaceError> {
    let retry_of = db::get_retry_lineage_for_retry(conn, &run.run_id)?;
    let retry_child = db::get_retry_lineage_for_source(conn, &run.run_id)?;
    let can_retry = run.retry_checkpoint().is_some() && retry_child.is_none();
    let continuation_checkpoint = run.continuation_checkpoint();
    let continuation_profile_available = !continuation_checkpoint
        .is_some_and(ContinuationCheckpoint::requires_existing_model_profile)
        || db::get_run_model_profile(conn, &run.run_id)?.is_some();
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
        retry_of_run_id: retry_of.map(|lineage| lineage.source_run_id),
        retry_run_id: retry_child.map(|lineage| lineage.retry_run_id),
        can_retry,
        continuation_checkpoint,
        can_continue: continuation_checkpoint.is_some() && continuation_profile_available,
        continuation_requires_runtime: continuation_checkpoint
            .is_some_and(ContinuationCheckpoint::requires_runtime),
        cancellation_requested: run.cancellation_requested,
        background_active: false,
        can_cancel: false,
    })
}

fn run_history_item_from_run(
    conn: &Connection,
    run: PipelineRun,
) -> Result<RunHistoryItem, WorkspaceError> {
    let document = db::get_document(conn, &run.document_id)?
        .ok_or_else(|| StoreError::DocumentNotFound(run.document_id.clone()))?;
    let has_summary = db::summary_artifact_exists(conn, &run.run_id)?;
    validate_summary_state(&run, has_summary)?;
    run_history_item(conn, run, document, has_summary)
}

fn validate_citations_against_sources(
    conn: &Connection,
    run_id: &str,
    summary: &SummaryArtifact,
    citations: Option<&CitationArtifact>,
) -> Result<(), WorkspaceError> {
    let Some(citations) = citations else {
        return if summary.summary_version == "1.0.0" {
            Ok(())
        } else {
            Err(WorkspaceError::CitationMismatch(
                summary.document_id.clone(),
            ))
        };
    };
    let normalized = db::get_normalized_document(conn, run_id)?
        .ok_or_else(|| WorkspaceError::CitationMismatch(summary.document_id.clone()))?;
    let chunked = db::get_chunked_document(conn, run_id)?
        .ok_or_else(|| WorkspaceError::CitationMismatch(summary.document_id.clone()))?;
    let analyzed = db::get_analyzed_document(conn, run_id)?
        .ok_or_else(|| WorkspaceError::CitationMismatch(summary.document_id.clone()))?;
    let verified = db::get_verified_document(conn, run_id)?
        .ok_or_else(|| WorkspaceError::CitationMismatch(summary.document_id.clone()))?;
    let synthesized = db::get_synthesis_attempt(conn, run_id, verified.synthesis_attempt_ordinal)?
        .ok_or_else(|| WorkspaceError::CitationMismatch(summary.document_id.clone()))?;
    crate::pipeline::summary::validate_citation_artifact(
        citations,
        summary,
        &verified,
        &synthesized,
        &analyzed,
        &chunked,
        &normalized,
    )
    .map_err(|_| WorkspaceError::CitationMismatch(summary.document_id.clone()))
}

fn summary_view(
    summary: SummaryArtifact,
    citations: Option<CitationArtifact>,
) -> Result<SummaryView, WorkspaceError> {
    if summary.calculate_integrity_hash().ok().as_deref() != Some(summary.integrity_hash.as_str()) {
        return Err(WorkspaceError::CitationMismatch(summary.document_id));
    }
    let claims = match citations {
        Some(citations) => {
            if citations.document_id != summary.document_id
                || crate::pipeline::summary::expected_citation_version(&summary.summary_version)
                    != Some(citations.citation_version.as_str())
                || citations.summary_integrity_hash != summary.integrity_hash
                || citations.rendered_text != summary.text
                || citations.claims.is_empty()
                || citations.evidence.is_empty()
                || citations.calculate_integrity_hash().ok().as_deref()
                    != Some(citations.integrity_hash.as_str())
            {
                return Err(WorkspaceError::CitationMismatch(summary.document_id));
            }
            let evidence_count = citations.evidence.len();
            let evidence = citations
                .evidence
                .into_iter()
                .map(|item| (item.evidence_id.clone(), item))
                .collect::<HashMap<_, _>>();
            if evidence.len() != evidence_count
                || evidence.values().any(|item| {
                    item.evidence_id.trim().is_empty()
                        || item.exact_quote.trim().is_empty()
                        || item.source_span.page_start == 0
                        || item.source_span.page_end < item.source_span.page_start
                })
            {
                return Err(WorkspaceError::CitationMismatch(summary.document_id));
            }
            let mut claim_ids = std::collections::HashSet::new();
            let mut referenced_evidence = std::collections::HashSet::new();
            let claims = citations
                .claims
                .into_iter()
                .map(|claim| {
                    let mut claim_evidence = std::collections::HashSet::new();
                    if claim.claim_id.trim().is_empty()
                        || claim.text.trim().is_empty()
                        || !claim_ids.insert(claim.claim_id.clone())
                        || claim
                            .evidence_ids
                            .iter()
                            .any(|evidence_id| !claim_evidence.insert(evidence_id.clone()))
                    {
                        return Err(WorkspaceError::CitationMismatch(
                            summary.document_id.clone(),
                        ));
                    }
                    referenced_evidence.extend(claim.evidence_ids.iter().cloned());
                    let citations = claim
                        .evidence_ids
                        .iter()
                        .map(|evidence_id| {
                            let item = evidence.get(evidence_id).ok_or_else(|| {
                                WorkspaceError::CitationMismatch(summary.document_id.clone())
                            })?;
                            Ok(CitationView {
                                evidence_id: item.evidence_id.clone(),
                                label: source_label(
                                    item.source_span.page_start,
                                    item.source_span.page_end,
                                ),
                                page_start: item.source_span.page_start,
                                page_end: item.source_span.page_end,
                                exact_quote: item.exact_quote.clone(),
                            })
                        })
                        .collect::<Result<Vec<_>, WorkspaceError>>()?;
                    if citations.is_empty() {
                        return Err(WorkspaceError::CitationMismatch(
                            summary.document_id.clone(),
                        ));
                    }
                    Ok(CitedClaimView {
                        claim_id: claim.claim_id,
                        text: claim.text,
                        citations,
                    })
                })
                .collect::<Result<Vec<_>, WorkspaceError>>()?;
            if referenced_evidence != evidence.keys().cloned().collect() {
                return Err(WorkspaceError::CitationMismatch(summary.document_id));
            }
            claims
        }
        None if summary.summary_version == "1.0.0" => Vec::new(),
        None => return Err(WorkspaceError::CitationMismatch(summary.document_id)),
    };
    Ok(SummaryView {
        text: summary.text,
        warnings: summary.warnings,
        created_at: summary.created_at,
        claims,
    })
}

fn source_label(page_start: u32, page_end: u32) -> String {
    if page_start == page_end {
        format!("p. {page_start}")
    } else {
        format!("pp. {page_start}–{page_end}")
    }
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
    use crate::pipeline::contracts::{
        CitationArtifact, CompletedSummary, ModelRequest, ModelResponse,
    };
    use crate::pipeline::db::init_db;
    use crate::pipeline::ingest::ingest_pdf;
    use crate::pipeline::normalize::CanonicalNormalizer;
    use crate::pipeline::parser::PdfExtractParser;
    use crate::pipeline::service::{process_pdf_to_summary, SummaryComponents};
    use crate::pipeline::structure::DeterministicStructureInterpreter;
    use rusqlite::params;
    use sha2::{Digest, Sha256};
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
            Ok(ModelResponse {
                text: crate::pipeline::summary::fixture_model_output(request),
                runtime_id: self.runtime_id().to_string(),
                model_id: self.model_id().to_string(),
                request_attempts: Vec::new(),
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
                request_attempts: Vec::new(),
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

    fn replace_citation_artifact(conn: &Connection, run_id: &str, citations: &CitationArtifact) {
        let artifact_json =
            serde_json::to_string(citations).expect("citation artifact should serialize");
        let row_hash = format!("{:x}", Sha256::digest(artifact_json.as_bytes()));
        conn.execute(
            "UPDATE citation_artifacts
             SET artifact_hash = ?1, citation_artifact = ?2
             WHERE run_id = ?3",
            params![row_hash, artifact_json, run_id],
        )
        .expect("citation artifact should be replaced");
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
    fn missing_citation_artifact_fails_closed_for_current_summaries() {
        let mut conn = init_db(":memory:").expect("database should initialize");
        let source = fixture_path();
        let completed = complete_fixture_summary(
            &mut conn,
            source.to_str().expect("fixture path should be UTF-8"),
        );
        conn.execute(
            "DELETE FROM citation_artifacts WHERE run_id = ?1",
            [&completed.run_id],
        )
        .expect("test citation should be removed");

        let error = get_persisted_summary(&conn, &completed.run_id)
            .expect_err("a current summary without citations must fail closed");
        assert_eq!(error.code(), "CITATION_ARTIFACT_MISMATCH");
    }

    #[test]
    fn legacy_v1_summary_remains_readable_without_fabricated_citations() {
        let mut conn = init_db(":memory:").expect("database should initialize");
        let source = fixture_path();
        let completed = complete_fixture_summary(
            &mut conn,
            source.to_str().expect("fixture path should be UTF-8"),
        );
        let mut legacy = completed.summary;
        legacy.summary_version = "1.0.0".to_string();
        legacy.integrity_hash = legacy
            .calculate_integrity_hash()
            .expect("legacy integrity hash should compute");
        let artifact_json =
            serde_json::to_string(&legacy).expect("legacy summary should serialize");
        let row_hash = format!("{:x}", Sha256::digest(artifact_json.as_bytes()));
        conn.execute(
            "UPDATE summary_artifacts
             SET summary_version = ?1, artifact_hash = ?2, summary_artifact = ?3
             WHERE run_id = ?4",
            params![
                legacy.summary_version,
                row_hash,
                artifact_json,
                completed.run_id
            ],
        )
        .expect("legacy summary should replace the fixture artifact");
        conn.execute(
            "DELETE FROM citation_artifacts WHERE run_id = ?1",
            [&completed.run_id],
        )
        .expect("legacy fixture should not retain a citation artifact");

        let persisted = get_persisted_summary(&conn, &completed.run_id)
            .expect("legacy summary should remain readable");
        assert_eq!(persisted.summary.text, legacy.text);
        assert!(persisted.summary.claims.is_empty());
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

        let persisted = get_persisted_summary(&conn, &completed.run_id)
            .expect("completed result should pass citation validation");
        let serialized = serde_json::to_value(CompletedSummaryView::from(persisted))
            .expect("presentation result should serialize");
        assert_eq!(serialized["originalFilename"], expected_filename);
        assert_eq!(serialized["summary"]["text"], expected_summary);
        assert!(serialized["summary"]["claims"]
            .as_array()
            .is_some_and(|claims| !claims.is_empty()));
        assert_eq!(
            serialized["summary"]["claims"][0]["citations"][0]["label"],
            "p. 1"
        );
        assert!(
            serialized["summary"]["claims"][0]["citations"][0]["exactQuote"]
                .as_str()
                .is_some_and(|quote| !quote.is_empty())
        );
        assert!(serialized.get("document").is_none());
        assert!(serialized.get("localSourcePath").is_none());
        assert!(serialized["summary"].get("integrityHash").is_none());
        assert!(serialized["summary"].get("documentId").is_none());
        assert!(serialized["summary"]["claims"][0]["citations"][0]
            .get("blockId")
            .is_none());
        assert!(serialized["summary"]["claims"][0]["citations"][0]
            .get("chunkId")
            .is_none());
    }

    #[test]
    fn presentation_boundary_rejects_invalid_spans_and_duplicate_claim_references() {
        let mut conn = init_db(":memory:").expect("database should initialize");
        let source = fixture_path();
        let completed = complete_fixture_summary(
            &mut conn,
            source.to_str().expect("fixture path should be UTF-8"),
        );

        let original = completed.citations;
        let mut invalid_span = original.clone();
        invalid_span.evidence[0].source_span.page_start = 0;
        invalid_span.integrity_hash = invalid_span
            .calculate_integrity_hash()
            .expect("modified citation hash should compute");
        replace_citation_artifact(&conn, &completed.run_id, &invalid_span);
        let error = get_persisted_summary(&conn, &completed.run_id)
            .expect_err("page zero must fail the presentation boundary");
        assert_eq!(error.code(), "CITATION_ARTIFACT_MISMATCH");

        let mut duplicate_reference = original;
        let evidence_id = duplicate_reference.claims[0].evidence_ids[0].clone();
        duplicate_reference.claims[0].evidence_ids.push(evidence_id);
        duplicate_reference.integrity_hash = duplicate_reference
            .calculate_integrity_hash()
            .expect("modified citation hash should compute");
        replace_citation_artifact(&conn, &completed.run_id, &duplicate_reference);
        let error = get_persisted_summary(&conn, &completed.run_id)
            .expect_err("duplicate evidence within one claim must fail");
        assert_eq!(error.code(), "CITATION_ARTIFACT_MISMATCH");
    }

    #[test]
    fn presentation_boundary_revalidates_checksum_valid_content_against_sources() {
        let mut conn = init_db(":memory:").expect("database should initialize");
        let source = fixture_path();
        let completed = complete_fixture_summary(
            &mut conn,
            source.to_str().expect("fixture path should be UTF-8"),
        );
        let original = completed.citations;

        let mut altered_claim = original.clone();
        altered_claim.claims[0].text.push_str(" altered");
        altered_claim.integrity_hash = altered_claim
            .calculate_integrity_hash()
            .expect("modified citation hash should compute");
        replace_citation_artifact(&conn, &completed.run_id, &altered_claim);
        assert_eq!(
            crate::pipeline::db::get_citation_artifact(&conn, &completed.run_id)
                .expect("checksum-valid citation should load")
                .expect("citation should exist"),
            altered_claim
        );
        let error = get_persisted_summary(&conn, &completed.run_id)
            .expect_err("altered claim must fail authoritative validation");
        assert_eq!(error.code(), "CITATION_ARTIFACT_MISMATCH");

        let mut altered_quote = original;
        altered_quote.evidence[0].exact_quote =
            "This quotation is absent from the normalized source.".to_string();
        altered_quote.integrity_hash = altered_quote
            .calculate_integrity_hash()
            .expect("modified citation hash should compute");
        replace_citation_artifact(&conn, &completed.run_id, &altered_quote);
        assert_eq!(
            crate::pipeline::db::get_citation_artifact(&conn, &completed.run_id)
                .expect("checksum-valid citation should load")
                .expect("citation should exist"),
            altered_quote
        );
        let error = get_persisted_summary(&conn, &completed.run_id)
            .expect_err("altered quote must fail authoritative validation");
        assert_eq!(error.code(), "CITATION_ARTIFACT_MISMATCH");
    }
}
