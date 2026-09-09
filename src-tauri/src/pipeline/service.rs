use crate::pipeline::chunk::{chunk_document, ChunkPipelineError};
use crate::pipeline::contracts::{
    CompletedSummary, ContinuationCheckpoint, DocumentChunker, DocumentNormalizer, DocumentParser,
    IngestedDocument, ModelProfileSnapshot, ModelRuntime, PipelineRun, PipelineState,
    StructureInterpreter, SummaryProfile,
};
use crate::pipeline::control::{ExecutionControl, UNCONTROLLED_EXECUTION};
use crate::pipeline::db::{self, StoreError};
use crate::pipeline::ingest::prepare_received_run;
use crate::pipeline::ingest::{ingest_pdf, ingest_pdf_with_profiles, IngestError};
use crate::pipeline::normalize::{normalize_document, NormalizePipelineError};
use crate::pipeline::parser::{parse_document, parse_started_document, ParsePipelineError};
use crate::pipeline::structure::{structure_document, StructurePipelineError};
#[cfg(test)]
use crate::pipeline::summary::{
    analyze_chunked_document, synthesize_analyzed_document, verify_synthesized_document,
};
use crate::pipeline::summary::{
    analyze_chunked_document_controlled, analyze_chunked_document_controlled_with_delivery,
    complete_verified_document, complete_verified_document_with_delivery,
    synthesize_analyzed_document_controlled, synthesize_analyzed_document_controlled_with_delivery,
    verify_synthesized_document_controlled, verify_synthesized_document_controlled_with_delivery,
    SummaryDeliveryPolicy, SummaryPipelineError,
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
    #[error(transparent)]
    Continuation(#[from] ContinuationPipelineError),
    #[error(transparent)]
    ModelProfile(#[from] StoreError),
    #[error("Pipeline cancellation was observed at a safe work boundary")]
    CancellationObserved,
    #[error("Pipeline run {0} requires the local model runtime for background processing")]
    RuntimeRequiredForBackground(String),
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
            Self::Continuation(error) => error.code(),
            Self::ModelProfile(StoreError::ModelProfileMismatch { .. }) => "MODEL_PROFILE_MISMATCH",
            Self::ModelProfile(_) => "PIPELINE_STORE_ERROR",
            Self::CancellationObserved => "PIPELINE_CANCELLATION_OBSERVED",
            Self::RuntimeRequiredForBackground(_) => "BACKGROUND_RUNTIME_REQUIRED",
        }
    }

    pub(crate) fn is_concurrent_ownership_loss(&self) -> bool {
        match self {
            Self::Ingest(IngestError::Store(error)) => is_stale_store_error(error),
            Self::Parse(
                ParsePipelineError::Store(error)
                | ParsePipelineError::ArtifactPersistence(error)
                | ParsePipelineError::FailurePersistence {
                    persistence: error, ..
                },
            ) => is_stale_store_error(error),
            Self::Normalize(
                NormalizePipelineError::Store(error)
                | NormalizePipelineError::ArtifactPersistence(error)
                | NormalizePipelineError::FailurePersistence {
                    persistence: error, ..
                },
            ) => is_stale_store_error(error),
            Self::Structure(
                StructurePipelineError::Store(error)
                | StructurePipelineError::ArtifactPersistence(error)
                | StructurePipelineError::FailurePersistence {
                    persistence: error, ..
                },
            ) => is_stale_store_error(error),
            Self::Chunk(
                ChunkPipelineError::Store(error)
                | ChunkPipelineError::ArtifactPersistence(error)
                | ChunkPipelineError::FailurePersistence {
                    persistence: error, ..
                },
            ) => is_stale_store_error(error),
            Self::Summary(
                SummaryPipelineError::Store(error)
                | SummaryPipelineError::ArtifactPersistence { source: error, .. }
                | SummaryPipelineError::FailurePersistence {
                    persistence: error, ..
                },
            ) => is_stale_store_error(error),
            Self::Retry(RetryPipelineError::Store(error))
            | Self::Continuation(ContinuationPipelineError::Store(error)) => {
                is_stale_store_error(error)
            }
            Self::ModelProfile(error) => is_stale_store_error(error),
            Self::Continuation(ContinuationPipelineError::StaleState { .. }) => true,
            _ => false,
        }
    }
}

fn is_stale_store_error(error: &StoreError) -> bool {
    error.is_stale_transition()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContinuationPlan {
    pub checkpoint: ContinuationCheckpoint,
    pub requires_runtime: bool,
}

#[derive(Debug, Error)]
pub enum ContinuationPipelineError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(
        "Pipeline run {run_id} changed: expected state version {expected_version}, found {found_version}"
    )]
    StaleState {
        run_id: String,
        expected_version: u32,
        found_version: u32,
    },
    #[error("Pipeline run {run_id} cannot continue from {state:?}")]
    NotAllowed {
        run_id: String,
        state: PipelineState,
    },
    #[error("Pipeline run {run_id} requires the local model runtime to continue")]
    RuntimeRequired { run_id: String },
}

impl ContinuationPipelineError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Store(StoreError::RunNotFound(_)) => "CONTINUATION_RUN_NOT_FOUND",
            Self::Store(_) => "PIPELINE_STORE_ERROR",
            Self::StaleState { .. } => "CONTINUATION_STALE_STATE",
            Self::NotAllowed { .. } => "CONTINUATION_NOT_ALLOWED",
            Self::RuntimeRequired { .. } => "CONTINUATION_RUNTIME_REQUIRED",
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

#[derive(Clone, Copy)]
pub struct ContinuationComponents<'a> {
    pub parser: &'a dyn DocumentParser,
    pub normalizer: &'a dyn DocumentNormalizer,
    pub interpreter: &'a dyn StructureInterpreter,
    pub chunker: &'a dyn DocumentChunker,
    pub runtime: Option<&'a dyn ModelRuntime>,
}

pub fn continuation_plan(
    conn: &Connection,
    run_id: &str,
    expected_state_version: u32,
) -> Result<ContinuationPlan, ContinuationPipelineError> {
    let run = db::get_pipeline_run(conn, run_id)?
        .ok_or_else(|| StoreError::RunNotFound(run_id.to_string()))?;
    if run.state_version != expected_state_version {
        return Err(ContinuationPipelineError::StaleState {
            run_id: run_id.to_string(),
            expected_version: expected_state_version,
            found_version: run.state_version,
        });
    }
    let checkpoint =
        run.continuation_checkpoint()
            .ok_or_else(|| ContinuationPipelineError::NotAllowed {
                run_id: run_id.to_string(),
                state: run.state,
            })?;
    Ok(ContinuationPlan {
        checkpoint,
        requires_runtime: checkpoint.requires_runtime(),
    })
}

pub fn continue_run_to_summary(
    conn: &mut Connection,
    run_id: &str,
    expected_state_version: u32,
    components: ContinuationComponents<'_>,
) -> Result<CompletedSummary, DocumentServiceError> {
    continue_run_to_summary_controlled(
        conn,
        run_id,
        expected_state_version,
        components,
        &UNCONTROLLED_EXECUTION,
    )
}

pub(crate) fn continue_run_to_summary_controlled(
    conn: &mut Connection,
    run_id: &str,
    expected_state_version: u32,
    components: ContinuationComponents<'_>,
    control: &dyn ExecutionControl,
) -> Result<CompletedSummary, DocumentServiceError> {
    cancellation_checkpoint(control)?;
    let plan = continuation_plan(conn, run_id, expected_state_version)?;
    let run = db::get_pipeline_run(conn, run_id)
        .map_err(ContinuationPipelineError::from)?
        .ok_or_else(|| {
            ContinuationPipelineError::Store(StoreError::RunNotFound(run_id.to_string()))
        })?;
    let document = db::get_document(conn, &run.document_id)
        .map_err(ContinuationPipelineError::from)?
        .ok_or_else(|| {
            ContinuationPipelineError::Store(StoreError::DocumentNotFound(run.document_id.clone()))
        })?;

    let summary = match plan.checkpoint {
        ContinuationCheckpoint::Ingested => process_ingested_to_summary_controlled(
            conn,
            run_id,
            continuation_summary_components(run_id, components)?,
            control,
        )?,
        ContinuationCheckpoint::Parsed => process_parsed_to_summary_controlled(
            conn,
            run_id,
            continuation_summary_components(run_id, components)?,
            control,
        )?,
        ContinuationCheckpoint::Normalized => process_normalized_to_summary_controlled(
            conn,
            run_id,
            continuation_summary_components(run_id, components)?,
            control,
        )?,
        ContinuationCheckpoint::Structured => process_structured_to_summary_controlled(
            conn,
            run_id,
            continuation_summary_components(run_id, components)?,
            control,
        )?,
        ContinuationCheckpoint::Chunked => process_chunked_to_summary_controlled(
            conn,
            run_id,
            required_continuation_runtime(run_id, components.runtime)?,
            control,
        )?,
        ContinuationCheckpoint::Analyzed => process_analyzed_to_summary_controlled(
            conn,
            run_id,
            required_continuation_runtime(run_id, components.runtime)?,
            control,
        )?,
        ContinuationCheckpoint::Synthesized => process_synthesized_to_summary_controlled(
            conn,
            run_id,
            required_continuation_runtime(run_id, components.runtime)?,
            control,
        )?,
        ContinuationCheckpoint::Verified => {
            cancellation_checkpoint(control)?;
            complete_verified_document(conn, run_id)?
        }
    };

    Ok(CompletedSummary {
        run_id: run_id.to_string(),
        document,
        summary: summary.summary,
        citations: summary.citations,
    })
}

fn continuation_summary_components<'a>(
    run_id: &str,
    components: ContinuationComponents<'a>,
) -> Result<SummaryComponents<'a>, ContinuationPipelineError> {
    Ok(SummaryComponents {
        parser: components.parser,
        normalizer: components.normalizer,
        interpreter: components.interpreter,
        chunker: components.chunker,
        runtime: required_continuation_runtime(run_id, components.runtime)?,
    })
}

fn required_continuation_runtime<'a>(
    run_id: &str,
    runtime: Option<&'a dyn ModelRuntime>,
) -> Result<&'a dyn ModelRuntime, ContinuationPipelineError> {
    runtime.ok_or_else(|| ContinuationPipelineError::RuntimeRequired {
        run_id: run_id.to_string(),
    })
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

pub(crate) fn admit_pdf_for_background(
    conn: &mut Connection,
    file_path: &str,
    profile_snapshot: Option<&ModelProfileSnapshot>,
    summary_profile: SummaryProfile,
    expected_content_hash: Option<&str>,
) -> Result<(IngestedDocument, PipelineRun), DocumentServiceError> {
    let (document, ingested) = ingest_pdf_with_profiles(
        conn,
        file_path,
        profile_snapshot,
        summary_profile,
        expected_content_hash,
    )?;
    let (parsing, persisted_document) =
        db::start_parsing(conn, &ingested.run_id, ingested.state_version)
            .map_err(ParsePipelineError::from)?;
    debug_assert_eq!(document, persisted_document);
    Ok((document, parsing))
}

pub(crate) fn admit_retry_for_background(
    conn: &mut Connection,
    source_run_id: &str,
    expected_source_version: u32,
) -> Result<(IngestedDocument, PipelineRun), DocumentServiceError> {
    create_retry_processing_run(conn, source_run_id, expected_source_version)
}

pub(crate) fn validate_retry_for_background(
    conn: &Connection,
    source_run_id: &str,
    expected_source_version: u32,
) -> Result<(), DocumentServiceError> {
    db::validate_retry_source(conn, source_run_id, expected_source_version)
        .map_err(RetryPipelineError::from)?;
    Ok(())
}

pub fn retry_failed_run_to_summary(
    conn: &mut Connection,
    source_run_id: &str,
    expected_source_version: u32,
    components: SummaryComponents<'_>,
) -> Result<CompletedSummary, DocumentServiceError> {
    let (_document, retry_run) =
        create_retry_processing_run(conn, source_run_id, expected_source_version)?;
    process_started_parsing_to_summary_controlled(
        conn,
        &retry_run.run_id,
        components,
        &UNCONTROLLED_EXECUTION,
    )
}

fn create_retry_processing_run(
    conn: &mut Connection,
    source_run_id: &str,
    expected_source_version: u32,
) -> Result<(IngestedDocument, PipelineRun), DocumentServiceError> {
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
    Ok((document, retry_run))
}

pub(crate) fn process_started_parsing_to_summary_controlled(
    conn: &mut Connection,
    run_id: &str,
    components: SummaryComponents<'_>,
    control: &dyn ExecutionControl,
) -> Result<CompletedSummary, DocumentServiceError> {
    cancellation_checkpoint(control)?;
    let run = db::get_pipeline_run(conn, run_id)
        .map_err(ParsePipelineError::from)?
        .ok_or_else(|| StoreError::RunNotFound(run_id.to_string()))
        .map_err(ParsePipelineError::from)?;
    let document = db::get_document(conn, &run.document_id)
        .map_err(ParsePipelineError::from)?
        .ok_or_else(|| StoreError::DocumentNotFound(run.document_id.clone()))
        .map_err(ParsePipelineError::from)?;
    parse_started_document(
        conn,
        components.parser,
        run_id,
        run.state_version,
        &document,
    )?;
    let summary = process_parsed_to_summary_controlled(conn, run_id, components, control)?;
    Ok(CompletedSummary {
        run_id: run_id.to_string(),
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
    process_ingested_to_summary_controlled(conn, run_id, components, &UNCONTROLLED_EXECUTION)
}

pub fn process_ingested_to_summary_with_delivery_policy(
    conn: &mut Connection,
    run_id: &str,
    components: SummaryComponents<'_>,
    delivery_policy: SummaryDeliveryPolicy,
) -> Result<crate::pipeline::contracts::SummaryArtifacts, DocumentServiceError> {
    parse_document(conn, components.parser, run_id)?;
    normalize_document(conn, components.normalizer, run_id)?;
    structure_document(conn, components.interpreter, run_id)?;
    chunk_document(conn, components.chunker, run_id)?;
    ensure_runtime_profile(conn, run_id, components.runtime)?;
    analyze_chunked_document_controlled_with_delivery(
        conn,
        components.runtime,
        run_id,
        &UNCONTROLLED_EXECUTION,
        Some(delivery_policy),
    )?;
    synthesize_analyzed_document_controlled_with_delivery(
        conn,
        components.runtime,
        run_id,
        &UNCONTROLLED_EXECUTION,
        Some(delivery_policy),
    )?;
    verify_synthesized_document_controlled_with_delivery(
        conn,
        components.runtime,
        run_id,
        &UNCONTROLLED_EXECUTION,
        Some(delivery_policy),
    )?;
    Ok(complete_verified_document_with_delivery(
        conn,
        run_id,
        Some(delivery_policy),
    )?)
}

pub(crate) fn process_ingested_to_summary_controlled(
    conn: &mut Connection,
    run_id: &str,
    components: SummaryComponents<'_>,
    control: &dyn ExecutionControl,
) -> Result<crate::pipeline::contracts::SummaryArtifacts, DocumentServiceError> {
    cancellation_checkpoint(control)?;
    parse_document(conn, components.parser, run_id)?;
    process_parsed_to_summary_controlled(conn, run_id, components, control)
}

fn process_parsed_to_summary_controlled(
    conn: &mut Connection,
    run_id: &str,
    components: SummaryComponents<'_>,
    control: &dyn ExecutionControl,
) -> Result<crate::pipeline::contracts::SummaryArtifacts, DocumentServiceError> {
    cancellation_checkpoint(control)?;
    normalize_document(conn, components.normalizer, run_id)?;
    process_normalized_to_summary_controlled(conn, run_id, components, control)
}

fn process_normalized_to_summary_controlled(
    conn: &mut Connection,
    run_id: &str,
    components: SummaryComponents<'_>,
    control: &dyn ExecutionControl,
) -> Result<crate::pipeline::contracts::SummaryArtifacts, DocumentServiceError> {
    cancellation_checkpoint(control)?;
    structure_document(conn, components.interpreter, run_id)?;
    process_structured_to_summary_controlled(conn, run_id, components, control)
}

fn process_structured_to_summary_controlled(
    conn: &mut Connection,
    run_id: &str,
    components: SummaryComponents<'_>,
    control: &dyn ExecutionControl,
) -> Result<crate::pipeline::contracts::SummaryArtifacts, DocumentServiceError> {
    cancellation_checkpoint(control)?;
    chunk_document(conn, components.chunker, run_id)?;
    process_chunked_to_summary_controlled(conn, run_id, components.runtime, control)
}

fn process_chunked_to_summary_controlled(
    conn: &mut Connection,
    run_id: &str,
    runtime: &dyn ModelRuntime,
    control: &dyn ExecutionControl,
) -> Result<crate::pipeline::contracts::SummaryArtifacts, DocumentServiceError> {
    cancellation_checkpoint(control)?;
    ensure_runtime_profile(conn, run_id, runtime)?;
    analyze_chunked_document_controlled(conn, runtime, run_id, control)?;
    process_analyzed_to_summary_controlled(conn, run_id, runtime, control)
}

fn process_analyzed_to_summary_controlled(
    conn: &mut Connection,
    run_id: &str,
    runtime: &dyn ModelRuntime,
    control: &dyn ExecutionControl,
) -> Result<crate::pipeline::contracts::SummaryArtifacts, DocumentServiceError> {
    cancellation_checkpoint(control)?;
    ensure_runtime_profile(conn, run_id, runtime)?;
    synthesize_analyzed_document_controlled(conn, runtime, run_id, control)?;
    process_synthesized_to_summary_controlled(conn, run_id, runtime, control)
}

fn process_synthesized_to_summary_controlled(
    conn: &mut Connection,
    run_id: &str,
    runtime: &dyn ModelRuntime,
    control: &dyn ExecutionControl,
) -> Result<crate::pipeline::contracts::SummaryArtifacts, DocumentServiceError> {
    cancellation_checkpoint(control)?;
    ensure_runtime_profile(conn, run_id, runtime)?;
    verify_synthesized_document_controlled(conn, runtime, run_id, control)?;
    cancellation_checkpoint(control)?;
    Ok(complete_verified_document(conn, run_id)?)
}

fn ensure_runtime_profile(
    conn: &Connection,
    run_id: &str,
    runtime: &dyn ModelRuntime,
) -> Result<(), DocumentServiceError> {
    let snapshot = runtime.profile_snapshot();
    db::ensure_run_model_profile(conn, run_id, snapshot.as_ref())?;
    Ok(())
}

fn cancellation_checkpoint(control: &dyn ExecutionControl) -> Result<(), DocumentServiceError> {
    if control.cancellation_requested() {
        return Err(DocumentServiceError::CancellationObserved);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::chunk::DeterministicDocumentChunker;
    use crate::pipeline::contracts::{
        ModelProfileSnapshot, ModelRequest, ModelResponse, ModelRuntimeFailure,
        ModelStageProfileSnapshot, PipelineStage, PipelineState, PipelineWarning, RetryCheckpoint,
        SummaryPresentationMode,
    };
    use crate::pipeline::db::{
        get_analyzed_document, get_chunked_document, get_citation_artifact, get_document,
        get_normalized_document, get_parsed_document, get_pipeline_run,
        get_retry_lineage_for_retry, get_retry_lineage_for_source, get_structured_document,
        get_summary_artifact, get_synthesis_attempt, get_synthesized_document,
        get_verified_document, init_db, list_pipeline_events,
    };
    use crate::pipeline::model::OllamaRuntime;
    use crate::pipeline::normalize::CanonicalNormalizer;
    use crate::pipeline::parser::PdfExtractParser;
    use crate::pipeline::structure::DeterministicStructureInterpreter;
    use rusqlite::params;
    use sha2::{Digest, Sha256};
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;
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

    struct SynthesisFallbackFixtureRuntime;

    fn fixture_model_profile() -> ModelProfileSnapshot {
        ModelProfileSnapshot {
            version: 1,
            preset_id: "fixture-full-v1".to_string(),
            analysis: ModelStageProfileSnapshot {
                runtime_kind: Default::default(),
                profile_id: "fixture-analysis-v1".to_string(),
                model_name: "fixture-model".to_string(),
                model_digest: "fixture-analysis-digest".to_string(),
                context_tokens: 8_192,
                tokenizer_version: "fixture-tokenizer-v1".to_string(),
            },
            verification: ModelStageProfileSnapshot {
                runtime_kind: Default::default(),
                profile_id: "fixture-verification-v1".to_string(),
                model_name: "fixture-model".to_string(),
                model_digest: "fixture-verification-digest".to_string(),
                context_tokens: 8_192,
                tokenizer_version: "fixture-tokenizer-v1".to_string(),
            },
        }
    }

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
            "fixture-runtime"
        }

        fn model_id(&self) -> &str {
            "fixture-model"
        }

        fn profile_snapshot(&self) -> Option<ModelProfileSnapshot> {
            Some(fixture_model_profile())
        }
    }

    impl ModelRuntime for SynthesisFallbackFixtureRuntime {
        fn generate(&self, request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
            FixtureRuntime.generate(request)
        }

        fn preflight_request(&self, request: &ModelRequest) -> Result<(), ModelRuntimeFailure> {
            if request.stage == PipelineStage::Synthesize {
                return Err(ModelRuntimeFailure {
                    code: "MODEL_CONTEXT_EXCEEDED".to_string(),
                    message: "fixture exact synthesis context rejection".to_string(),
                    recoverable: false,
                    request_attempts: Vec::new(),
                });
            }
            Ok(())
        }

        fn health(&self) -> Result<(), ModelRuntimeFailure> {
            FixtureRuntime.health()
        }

        fn runtime_id(&self) -> &str {
            FixtureRuntime.runtime_id()
        }

        fn model_id(&self) -> &str {
            FixtureRuntime.model_id()
        }

        fn profile_snapshot(&self) -> Option<ModelProfileSnapshot> {
            Some(fixture_model_profile())
        }
    }

    #[derive(Default)]
    struct SeedRecordingFixtureRuntime {
        seeds: Mutex<Vec<u64>>,
    }

    impl SeedRecordingFixtureRuntime {
        fn seeds(&self) -> Vec<u64> {
            self.seeds.lock().expect("seed lock should succeed").clone()
        }
    }

    impl ModelRuntime for SeedRecordingFixtureRuntime {
        fn generate(&self, request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
            self.seeds
                .lock()
                .expect("seed lock should succeed")
                .push(request.seed);
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
            "fixture-runtime"
        }

        fn model_id(&self) -> &str {
            "fixture-model"
        }

        fn profile_snapshot(&self) -> Option<ModelProfileSnapshot> {
            Some(fixture_model_profile())
        }
    }

    struct RecoverableFailureRuntime;

    impl ModelRuntime for RecoverableFailureRuntime {
        fn generate(&self, _request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
            Err(ModelRuntimeFailure {
                code: "FIXTURE_RUNTIME_INTERRUPTED".to_string(),
                message: "Fixture runtime stopped before producing output.".to_string(),
                recoverable: true,
                request_attempts: Vec::new(),
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

        fn profile_snapshot(&self) -> Option<ModelProfileSnapshot> {
            Some(fixture_model_profile())
        }
    }

    #[derive(Default)]
    struct CountingFixtureRuntime {
        generate_calls: AtomicU32,
        health_calls: AtomicU32,
    }

    impl CountingFixtureRuntime {
        fn reset(&self) {
            self.generate_calls.store(0, Ordering::Relaxed);
            self.health_calls.store(0, Ordering::Relaxed);
        }
    }

    impl ModelRuntime for CountingFixtureRuntime {
        fn generate(&self, request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
            self.generate_calls.fetch_add(1, Ordering::Relaxed);
            Ok(ModelResponse {
                text: crate::pipeline::summary::fixture_model_output(request),
                runtime_id: self.runtime_id().to_string(),
                model_id: self.model_id().to_string(),
                request_attempts: Vec::new(),
            })
        }

        fn health(&self) -> Result<(), ModelRuntimeFailure> {
            self.health_calls.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn runtime_id(&self) -> &str {
            "fixture-runtime"
        }

        fn model_id(&self) -> &str {
            "fixture-model"
        }

        fn profile_snapshot(&self) -> Option<ModelProfileSnapshot> {
            Some(fixture_model_profile())
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

        fn continuation_components<'a>(
            &'a self,
            runtime: Option<&'a dyn ModelRuntime>,
        ) -> ContinuationComponents<'a> {
            ContinuationComponents {
                parser: &self.parser,
                normalizer: &self.normalizer,
                interpreter: &self.interpreter,
                chunker: &self.chunker,
                runtime,
            }
        }
    }

    fn prepare_checkpoint(
        conn: &mut Connection,
        source: &TestSource,
        pipeline: &TestPipeline,
        runtime: &dyn ModelRuntime,
        checkpoint: ContinuationCheckpoint,
    ) -> crate::pipeline::contracts::PipelineRun {
        let (_, ingested) = ingest_pdf(
            conn,
            source.0.to_str().expect("fixture path should be UTF-8"),
        )
        .expect("fixture should ingest");
        if checkpoint == ContinuationCheckpoint::Ingested {
            return ingested;
        }

        parse_document(conn, &pipeline.parser, &ingested.run_id)
            .expect("fixture should parse to checkpoint");
        if checkpoint == ContinuationCheckpoint::Parsed {
            return get_pipeline_run(conn, &ingested.run_id)
                .expect("run should load")
                .expect("run should exist");
        }

        normalize_document(conn, &pipeline.normalizer, &ingested.run_id)
            .expect("fixture should normalize to checkpoint");
        if checkpoint == ContinuationCheckpoint::Normalized {
            return get_pipeline_run(conn, &ingested.run_id)
                .expect("run should load")
                .expect("run should exist");
        }

        structure_document(conn, &pipeline.interpreter, &ingested.run_id)
            .expect("fixture should structure to checkpoint");
        if checkpoint == ContinuationCheckpoint::Structured {
            return get_pipeline_run(conn, &ingested.run_id)
                .expect("run should load")
                .expect("run should exist");
        }

        chunk_document(conn, &pipeline.chunker, &ingested.run_id)
            .expect("fixture should chunk to checkpoint");
        if checkpoint == ContinuationCheckpoint::Chunked {
            return get_pipeline_run(conn, &ingested.run_id)
                .expect("run should load")
                .expect("run should exist");
        }

        ensure_runtime_profile(conn, &ingested.run_id, runtime)
            .expect("first model work should persist its runtime profile");
        analyze_chunked_document(conn, runtime, &ingested.run_id)
            .expect("fixture should analyze to checkpoint");
        if checkpoint == ContinuationCheckpoint::Analyzed {
            return get_pipeline_run(conn, &ingested.run_id)
                .expect("run should load")
                .expect("run should exist");
        }

        synthesize_analyzed_document(conn, runtime, &ingested.run_id)
            .expect("fixture should synthesize to checkpoint");
        if checkpoint == ContinuationCheckpoint::Synthesized {
            return get_pipeline_run(conn, &ingested.run_id)
                .expect("run should load")
                .expect("run should exist");
        }

        verify_synthesized_document(conn, runtime, &ingested.run_id)
            .expect("fixture should verify to checkpoint");
        get_pipeline_run(conn, &ingested.run_id)
            .expect("run should load")
            .expect("run should exist")
    }

    fn checkpoint_artifact(
        conn: &Connection,
        run: &crate::pipeline::contracts::PipelineRun,
        checkpoint: ContinuationCheckpoint,
    ) -> serde_json::Value {
        match checkpoint {
            ContinuationCheckpoint::Ingested => serde_json::to_value(
                get_document(conn, &run.document_id)
                    .expect("document should load")
                    .expect("document should exist"),
            ),
            ContinuationCheckpoint::Parsed => serde_json::to_value(
                get_parsed_document(conn, &run.run_id)
                    .expect("parsed artifact should load")
                    .expect("parsed artifact should exist"),
            ),
            ContinuationCheckpoint::Normalized => serde_json::to_value(
                get_normalized_document(conn, &run.run_id)
                    .expect("normalized artifact should load")
                    .expect("normalized artifact should exist"),
            ),
            ContinuationCheckpoint::Structured => serde_json::to_value(
                get_structured_document(conn, &run.run_id)
                    .expect("structured artifact should load")
                    .expect("structured artifact should exist"),
            ),
            ContinuationCheckpoint::Chunked => serde_json::to_value(
                get_chunked_document(conn, &run.run_id)
                    .expect("chunked artifact should load")
                    .expect("chunked artifact should exist"),
            ),
            ContinuationCheckpoint::Analyzed => serde_json::to_value(
                get_analyzed_document(conn, &run.run_id)
                    .expect("analyzed artifact should load")
                    .expect("analyzed artifact should exist"),
            ),
            ContinuationCheckpoint::Synthesized => serde_json::to_value(
                get_synthesized_document(conn, &run.run_id)
                    .expect("synthesized artifact should load")
                    .expect("synthesized artifact should exist"),
            ),
            ContinuationCheckpoint::Verified => serde_json::to_value(
                get_verified_document(conn, &run.run_id)
                    .expect("verified artifact should load")
                    .expect("verified artifact should exist"),
            ),
        }
        .expect("checkpoint artifact should serialize")
    }

    fn create_recoverable_failed_run(
        conn: &mut Connection,
        source: &TestSource,
        pipeline: &TestPipeline,
    ) -> crate::pipeline::contracts::PipelineRun {
        create_recoverable_failed_run_with_profile(conn, source, pipeline, SummaryProfile::General)
    }

    fn create_recoverable_failed_run_with_profile(
        conn: &mut Connection,
        source: &TestSource,
        pipeline: &TestPipeline,
        summary_profile: SummaryProfile,
    ) -> crate::pipeline::contracts::PipelineRun {
        let (_, ingested) = ingest_pdf_with_profiles(
            conn,
            source.0.to_str().expect("fixture path should be UTF-8"),
            None,
            summary_profile,
            None,
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
            if result.summary.warnings.is_empty() {
                PipelineState::Complete
            } else {
                PipelineState::CompleteWithWarnings
            }
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
    fn connect_delivery_policy_preserves_the_direct_claim_ledger_coverage_contract() {
        let source = TestSource::from_fixture();
        let pipeline = TestPipeline::default();
        let mut conn = init_db(":memory:").expect("schema should initialize");
        let (_, ingested) = ingest_pdf(
            &mut conn,
            source.0.to_str().expect("fixture path should be UTF-8"),
        )
        .expect("fixture should ingest");

        let result = process_ingested_to_summary_with_delivery_policy(
            &mut conn,
            &ingested.run_id,
            pipeline.components(&FixtureRuntime),
            SummaryDeliveryPolicy::connect(),
        )
        .expect("Connect delivery should complete through the direct claim ledger");
        let synthesized = get_synthesized_document(&conn, &ingested.run_id)
            .expect("synthesis should load")
            .expect("synthesis should exist");
        let analyzed = get_analyzed_document(&conn, &ingested.run_id)
            .expect("analysis should load")
            .expect("analysis should exist");
        let normalized = get_normalized_document(&conn, &ingested.run_id)
            .expect("normalization should load")
            .expect("normalization should exist");

        assert_eq!(
            synthesized.presentation_mode,
            SummaryPresentationMode::LegacyClaimList
        );
        assert!(synthesized.summary_claims.is_empty());
        assert_eq!(
            result.citations.presentation_mode,
            SummaryPresentationMode::LegacyClaimList
        );
        assert!(result.citations.summary_claims.is_empty());
        assert!(
            crate::pipeline::summary::delivery_claim_prefix_coverage_satisfied(
                &result.citations,
                result.citations.claims.len(),
                &analyzed.omissions,
                &normalized,
            )
        );
    }

    #[test]
    fn every_stable_checkpoint_continues_the_same_run_without_repeating_completed_work() {
        let source = TestSource::from_fixture();
        let pipeline = TestPipeline::default();
        let checkpoints = [
            ContinuationCheckpoint::Ingested,
            ContinuationCheckpoint::Parsed,
            ContinuationCheckpoint::Normalized,
            ContinuationCheckpoint::Structured,
            ContinuationCheckpoint::Chunked,
            ContinuationCheckpoint::Analyzed,
            ContinuationCheckpoint::Synthesized,
            ContinuationCheckpoint::Verified,
        ];

        for checkpoint in checkpoints {
            let mut conn = init_db(":memory:").expect("schema should initialize");
            let runtime = CountingFixtureRuntime::default();
            let before = prepare_checkpoint(&mut conn, &source, &pipeline, &runtime, checkpoint);
            let before_events =
                list_pipeline_events(&conn, &before.run_id).expect("events should load");
            let before_artifact = checkpoint_artifact(&conn, &before, checkpoint);
            let plan = continuation_plan(&conn, &before.run_id, before.state_version)
                .expect("stable checkpoint should produce a continuation plan");
            assert_eq!(plan.checkpoint, checkpoint);
            assert_eq!(plan.requires_runtime, checkpoint.requires_runtime());

            let history = crate::pipeline::workspace::list_recent_runs(&conn)
                .expect("checkpoint history should load");
            assert_eq!(history.len(), 1);
            assert!(history[0].can_continue);
            assert_eq!(history[0].continuation_checkpoint, Some(checkpoint));
            assert_eq!(
                history[0].continuation_requires_runtime,
                checkpoint.requires_runtime()
            );
            let serialized =
                serde_json::to_value(&history[0]).expect("history contract should serialize");
            assert_eq!(serialized["canContinue"], true);
            assert_eq!(
                serialized["continuationRequiresRuntime"],
                checkpoint.requires_runtime()
            );

            runtime.reset();
            let runtime_component = checkpoint
                .requires_runtime()
                .then_some(&runtime as &dyn ModelRuntime);
            let completed = continue_run_to_summary(
                &mut conn,
                &before.run_id,
                before.state_version,
                pipeline.continuation_components(runtime_component),
            )
            .expect("stable checkpoint should continue to a summary");

            assert_eq!(completed.run_id, before.run_id);
            assert_eq!(completed.document.document_id, before.document_id);
            assert_eq!(
                checkpoint_artifact(&conn, &before, checkpoint),
                before_artifact
            );
            let completed_run = get_pipeline_run(&conn, &before.run_id)
                .expect("completed run should load")
                .expect("completed run should exist");
            assert_eq!(
                completed_run.state,
                if completed.summary.warnings.is_empty() {
                    PipelineState::Complete
                } else {
                    PipelineState::CompleteWithWarnings
                }
            );
            let after_events =
                list_pipeline_events(&conn, &before.run_id).expect("events should reload");
            assert_eq!(
                &after_events[..before_events.len()],
                before_events.as_slice()
            );
            assert_eq!(
                after_events[before_events.len()].previous_state,
                Some(before.state.clone())
            );
            assert_eq!(after_events.len(), completed_run.state_version as usize);
            assert!(get_summary_artifact(&conn, &before.run_id)
                .expect("summary should load")
                .is_some());
            assert!(get_citation_artifact(&conn, &before.run_id)
                .expect("citation should load")
                .is_some());

            match checkpoint {
                ContinuationCheckpoint::Analyzed => {
                    assert_eq!(runtime.generate_calls.load(Ordering::Relaxed), 3);
                    assert_eq!(runtime.health_calls.load(Ordering::Relaxed), 3);
                }
                ContinuationCheckpoint::Synthesized => {
                    assert_eq!(runtime.generate_calls.load(Ordering::Relaxed), 2);
                    assert_eq!(runtime.health_calls.load(Ordering::Relaxed), 2);
                }
                ContinuationCheckpoint::Verified => {
                    assert_eq!(runtime.generate_calls.load(Ordering::Relaxed), 0);
                    assert_eq!(runtime.health_calls.load(Ordering::Relaxed), 0);
                }
                _ => {
                    assert!(runtime.generate_calls.load(Ordering::Relaxed) > 1);
                    assert_eq!(runtime.health_calls.load(Ordering::Relaxed), 4);
                }
            }
        }
    }

    #[test]
    fn pre_disclosure_coherent_checkpoints_fail_recoverably_before_delivery() {
        let source = TestSource::from_fixture();
        let pipeline = TestPipeline::default();

        for checkpoint in [
            ContinuationCheckpoint::Synthesized,
            ContinuationCheckpoint::Verified,
        ] {
            let mut conn = init_db(":memory:").expect("schema should initialize");
            let run =
                prepare_checkpoint(&mut conn, &source, &pipeline, &FixtureRuntime, checkpoint);

            match checkpoint {
                ContinuationCheckpoint::Synthesized => {
                    let mut synthesized = get_synthesized_document(&conn, &run.run_id)
                        .expect("synthesis should load")
                        .expect("synthesis should exist");
                    synthesized.synthesis_version = "6.0.0".to_string();
                    let artifact_json = serde_json::to_string(&synthesized)
                        .expect("pre-disclosure synthesis should serialize");
                    let artifact_hash = format!("{:x}", Sha256::digest(artifact_json.as_bytes()));
                    conn.execute(
                        "UPDATE synthesized_documents
                         SET synthesis_version = ?1, artifact_hash = ?2, synthesized_artifact = ?3
                         WHERE run_id = ?4",
                        params![
                            synthesized.synthesis_version,
                            artifact_hash,
                            artifact_json,
                            run.run_id
                        ],
                    )
                    .expect("pre-disclosure synthesis fixture should install");
                }
                ContinuationCheckpoint::Verified => {
                    let mut verified = get_verified_document(&conn, &run.run_id)
                        .expect("verification should load")
                        .expect("verification should exist");
                    verified.verification_version = "8.0.0".to_string();
                    let artifact_json = serde_json::to_string(&verified)
                        .expect("pre-disclosure verification should serialize");
                    let artifact_hash = format!("{:x}", Sha256::digest(artifact_json.as_bytes()));
                    conn.execute(
                        "UPDATE verified_documents
                         SET verification_version = ?1, artifact_hash = ?2, verified_artifact = ?3
                         WHERE run_id = ?4",
                        params![
                            verified.verification_version,
                            artifact_hash,
                            artifact_json,
                            run.run_id
                        ],
                    )
                    .expect("pre-disclosure verification fixture should install");
                }
                _ => unreachable!("the fixture covers only affected coherent checkpoints"),
            }

            let runtime = (checkpoint == ContinuationCheckpoint::Synthesized)
                .then_some(&FixtureRuntime as &dyn ModelRuntime);
            let error = continue_run_to_summary(
                &mut conn,
                &run.run_id,
                run.state_version,
                pipeline.continuation_components(runtime),
            )
            .expect_err("pre-disclosure coherent checkpoints must not be delivered");
            assert_eq!(error.code(), "COHERENT_CHECKPOINT_REQUIRES_RETRY");
            assert_eq!(
                get_pipeline_run(&conn, &run.run_id)
                    .expect("failed run should load")
                    .expect("failed run should exist")
                    .state,
                PipelineState::Failed
            );
            assert!(get_summary_artifact(&conn, &run.run_id)
                .expect("summary query should succeed")
                .is_none());
            assert!(get_citation_artifact(&conn, &run.run_id)
                .expect("citation query should succeed")
                .is_none());
        }
    }

    #[test]
    fn pre_disclosure_fallback_checkpoints_remain_continuable() {
        let source = TestSource::from_fixture();
        let pipeline = TestPipeline::default();

        for checkpoint in [
            ContinuationCheckpoint::Synthesized,
            ContinuationCheckpoint::Verified,
        ] {
            let mut conn = init_db(":memory:").expect("schema should initialize");
            let run = prepare_checkpoint(
                &mut conn,
                &source,
                &pipeline,
                &SynthesisFallbackFixtureRuntime,
                checkpoint,
            );
            let mut synthesized = get_synthesized_document(&conn, &run.run_id)
                .expect("synthesis should load")
                .expect("synthesis should exist");
            assert_eq!(
                synthesized.presentation_mode,
                SummaryPresentationMode::ClaimLedgerFallback
            );
            synthesized.synthesis_version = "6.0.0".to_string();
            let artifact_json = serde_json::to_string(&synthesized)
                .expect("pre-disclosure fallback synthesis should serialize");
            let artifact_hash = format!("{:x}", Sha256::digest(artifact_json.as_bytes()));
            conn.execute(
                "UPDATE synthesized_documents
                 SET synthesis_version = ?1, artifact_hash = ?2, synthesized_artifact = ?3
                 WHERE run_id = ?4",
                params![
                    synthesized.synthesis_version,
                    artifact_hash,
                    artifact_json,
                    run.run_id
                ],
            )
            .expect("pre-disclosure fallback synthesis fixture should install");
            conn.execute_batch("DROP TRIGGER summary_synthesis_attempts_no_update;")
                .expect("historical fixture should temporarily allow attempt replacement");
            conn.execute(
                "UPDATE summary_synthesis_attempts
                 SET synthesis_version = ?1, artifact_hash = ?2, synthesized_artifact = ?3
                 WHERE run_id = ?4 AND attempt_ordinal = 0",
                params![
                    synthesized.synthesis_version,
                    artifact_hash,
                    artifact_json,
                    run.run_id
                ],
            )
            .expect("pre-disclosure fallback synthesis attempt should install");
            conn.execute_batch(
                "CREATE TRIGGER summary_synthesis_attempts_no_update
                 BEFORE UPDATE ON summary_synthesis_attempts
                 BEGIN
                     SELECT RAISE(ABORT, 'summary_synthesis_attempts are immutable');
                 END;",
            )
            .expect("synthesis-attempt immutability should be restored");

            if checkpoint == ContinuationCheckpoint::Verified {
                let mut verified = get_verified_document(&conn, &run.run_id)
                    .expect("verification should load")
                    .expect("verification should exist");
                verified.verification_version = "8.0.0".to_string();
                let artifact_json = serde_json::to_string(&verified)
                    .expect("pre-disclosure fallback verification should serialize");
                let artifact_hash = format!("{:x}", Sha256::digest(artifact_json.as_bytes()));
                conn.execute(
                    "UPDATE verified_documents
                     SET verification_version = ?1, artifact_hash = ?2, verified_artifact = ?3
                     WHERE run_id = ?4",
                    params![
                        verified.verification_version,
                        artifact_hash,
                        artifact_json,
                        run.run_id
                    ],
                )
                .expect("pre-disclosure fallback verification fixture should install");
            }

            let runtime = (checkpoint == ContinuationCheckpoint::Synthesized)
                .then_some(&SynthesisFallbackFixtureRuntime as &dyn ModelRuntime);
            let completed = continue_run_to_summary(
                &mut conn,
                &run.run_id,
                run.state_version,
                pipeline.continuation_components(runtime),
            )
            .expect("pre-disclosure fallback checkpoint should remain continuable");

            assert_eq!(
                completed.citations.presentation_mode,
                SummaryPresentationMode::ClaimLedgerFallback
            );
            assert!(completed
                .summary
                .warnings
                .iter()
                .any(|warning| warning.code == "COHERENT_SUMMARY_SOURCE_CONTEXT_TOO_LARGE"));
            assert!(completed
                .summary
                .warnings
                .iter()
                .all(|warning| warning.code != "COHERENT_SUMMARY_SOURCE_SELECTION_APPLIED"));
        }
    }

    #[test]
    fn legacy_model_artifact_without_a_profile_is_not_advertised_as_continuable() {
        let source = TestSource::from_fixture();
        let pipeline = TestPipeline::default();
        let runtime = CountingFixtureRuntime::default();
        let mut conn = init_db(":memory:").expect("schema should initialize");
        let chunked = prepare_checkpoint(
            &mut conn,
            &source,
            &pipeline,
            &runtime,
            ContinuationCheckpoint::Chunked,
        );
        analyze_chunked_document(&mut conn, &runtime, &chunked.run_id)
            .expect("legacy-shaped fixture should reach analyzed without a v15 profile");

        let history = crate::pipeline::workspace::list_recent_runs(&conn)
            .expect("legacy history should load without activating a runtime");
        assert_eq!(history.len(), 1);
        assert_eq!(
            history[0].continuation_checkpoint,
            Some(ContinuationCheckpoint::Analyzed)
        );
        assert!(history[0].continuation_requires_runtime);
        assert!(!history[0].can_continue);
    }

    #[test]
    fn continuation_rejects_stale_missing_runtime_active_failed_and_terminal_runs() {
        let source = TestSource::from_fixture();
        let pipeline = TestPipeline::default();
        let runtime = CountingFixtureRuntime::default();
        let mut conn = init_db(":memory:").expect("schema should initialize");
        let ingested = prepare_checkpoint(
            &mut conn,
            &source,
            &pipeline,
            &runtime,
            ContinuationCheckpoint::Ingested,
        );
        let before_events =
            list_pipeline_events(&conn, &ingested.run_id).expect("events should load");

        let stale = continue_run_to_summary(
            &mut conn,
            &ingested.run_id,
            ingested.state_version - 1,
            pipeline.continuation_components(Some(&runtime)),
        )
        .expect_err("stale continuation must fail");
        assert_eq!(stale.code(), "CONTINUATION_STALE_STATE");

        let missing_runtime = continue_run_to_summary(
            &mut conn,
            &ingested.run_id,
            ingested.state_version,
            pipeline.continuation_components(None),
        )
        .expect_err("model-dependent checkpoint must require a runtime");
        assert_eq!(missing_runtime.code(), "CONTINUATION_RUNTIME_REQUIRED");
        assert_eq!(
            get_pipeline_run(&conn, &ingested.run_id)
                .expect("run should load")
                .expect("run should exist"),
            ingested
        );
        assert_eq!(
            list_pipeline_events(&conn, &ingested.run_id).expect("events should reload"),
            before_events
        );

        let (parsing, _) = db::start_parsing(&mut conn, &ingested.run_id, ingested.state_version)
            .expect("parser admission should succeed");
        let active = continuation_plan(&conn, &parsing.run_id, parsing.state_version)
            .expect_err("active run must not expose continuation");
        assert_eq!(active.code(), "CONTINUATION_NOT_ALLOWED");

        let mut failed_conn = init_db(":memory:").expect("schema should initialize");
        let failed = create_recoverable_failed_run(&mut failed_conn, &source, &pipeline);
        let failed_error = continuation_plan(&failed_conn, &failed.run_id, failed.state_version)
            .expect_err("failed run must use new-run retry instead");
        assert_eq!(failed_error.code(), "CONTINUATION_NOT_ALLOWED");

        let mut completed_conn = init_db(":memory:").expect("schema should initialize");
        let verified = prepare_checkpoint(
            &mut completed_conn,
            &source,
            &pipeline,
            &runtime,
            ContinuationCheckpoint::Verified,
        );
        continue_run_to_summary(
            &mut completed_conn,
            &verified.run_id,
            verified.state_version,
            pipeline.continuation_components(None),
        )
        .expect("verified checkpoint should complete without runtime");
        let completed = get_pipeline_run(&completed_conn, &verified.run_id)
            .expect("completed run should load")
            .expect("completed run should exist");
        let terminal =
            continuation_plan(&completed_conn, &completed.run_id, completed.state_version)
                .expect_err("completed run must not continue again");
        assert_eq!(terminal.code(), "CONTINUATION_NOT_ALLOWED");
    }

    #[test]
    fn corrupt_checkpoint_and_atomic_completion_failure_never_claim_success() {
        let source = TestSource::from_fixture();
        let pipeline = TestPipeline::default();
        let runtime = CountingFixtureRuntime::default();
        let mut corrupt_conn = init_db(":memory:").expect("schema should initialize");
        let normalized = prepare_checkpoint(
            &mut corrupt_conn,
            &source,
            &pipeline,
            &runtime,
            ContinuationCheckpoint::Normalized,
        );
        let before_events = list_pipeline_events(&corrupt_conn, &normalized.run_id)
            .expect("events should load before corruption probe");
        corrupt_conn
            .execute(
                "UPDATE normalized_documents SET artifact_hash = 'corrupt' WHERE run_id = ?1",
                [&normalized.run_id],
            )
            .expect("corruption probe should alter the test database");
        let corrupt = continue_run_to_summary(
            &mut corrupt_conn,
            &normalized.run_id,
            normalized.state_version,
            pipeline.continuation_components(Some(&runtime)),
        )
        .expect_err("corrupt checkpoint must fail closed");
        assert_eq!(corrupt.code(), "PIPELINE_STORE_ERROR");
        assert_eq!(
            get_pipeline_run(&corrupt_conn, &normalized.run_id)
                .expect("run should load")
                .expect("run should exist"),
            normalized
        );
        assert_eq!(
            list_pipeline_events(&corrupt_conn, &normalized.run_id)
                .expect("events should remain readable"),
            before_events
        );
        assert!(get_structured_document(&corrupt_conn, &normalized.run_id)
            .expect("structured artifact query should succeed")
            .is_none());

        let mut atomic_conn = init_db(":memory:").expect("schema should initialize");
        let verified = prepare_checkpoint(
            &mut atomic_conn,
            &source,
            &pipeline,
            &runtime,
            ContinuationCheckpoint::Verified,
        );
        let verified_events = list_pipeline_events(&atomic_conn, &verified.run_id)
            .expect("verified events should load");
        atomic_conn
            .execute_batch(
                "CREATE TRIGGER fail_continued_citation
                 BEFORE INSERT ON citation_artifacts
                 BEGIN SELECT RAISE(ABORT, 'injected continued citation failure'); END;",
            )
            .expect("failure trigger should install");
        let persistence = continue_run_to_summary(
            &mut atomic_conn,
            &verified.run_id,
            verified.state_version,
            pipeline.continuation_components(None),
        )
        .expect_err("injected final artifact failure must fail continuation");
        assert_eq!(persistence.code(), "SUMMARY_ARTIFACT_PERSISTENCE_FAILED");
        assert!(get_summary_artifact(&atomic_conn, &verified.run_id)
            .expect("summary query should succeed")
            .is_none());
        assert!(get_citation_artifact(&atomic_conn, &verified.run_id)
            .expect("citation query should succeed")
            .is_none());
        assert!(get_verified_document(&atomic_conn, &verified.run_id)
            .expect("verified artifact should load")
            .is_some());
        let failed = get_pipeline_run(&atomic_conn, &verified.run_id)
            .expect("run should load")
            .expect("run should exist");
        assert_eq!(failed.state, PipelineState::Failed);
        assert_eq!(failed.state_version, verified.state_version + 1);
        let failed_events = list_pipeline_events(&atomic_conn, &verified.run_id)
            .expect("failed events should load");
        assert_eq!(failed_events.len(), verified_events.len() + 1);
        assert_eq!(
            failed_events.last().map(|event| &event.next_state),
            Some(&PipelineState::Failed)
        );
        assert!(!failed_events.iter().any(|event| matches!(
            event.next_state,
            PipelineState::Complete | PipelineState::CompleteWithWarnings
        )));
    }

    #[test]
    fn stable_checkpoint_continuation_survives_independent_database_reopen() {
        let database = TestDatabase::new();
        let source = TestSource::from_fixture();
        let pipeline = TestPipeline::default();
        let runtime = CountingFixtureRuntime::default();
        let (checkpoint_run, expected_checkpoint_artifact, checkpoint_events) = {
            let mut conn = init_db(&database.0).expect("schema should initialize");
            let run = prepare_checkpoint(
                &mut conn,
                &source,
                &pipeline,
                &runtime,
                ContinuationCheckpoint::Synthesized,
            );
            let artifact = checkpoint_artifact(&conn, &run, ContinuationCheckpoint::Synthesized);
            let events =
                list_pipeline_events(&conn, &run.run_id).expect("checkpoint events should load");
            (run, artifact, events)
        };

        runtime.reset();
        let completed = {
            let mut reopened = init_db(&database.0).expect("database should independently reopen");
            assert_eq!(
                checkpoint_artifact(
                    &reopened,
                    &checkpoint_run,
                    ContinuationCheckpoint::Synthesized,
                ),
                expected_checkpoint_artifact
            );
            continue_run_to_summary(
                &mut reopened,
                &checkpoint_run.run_id,
                checkpoint_run.state_version,
                pipeline.continuation_components(Some(&runtime)),
            )
            .expect("reopened synthesized checkpoint should verify with the runtime")
        };
        assert_eq!(runtime.generate_calls.load(Ordering::Relaxed), 2);
        assert_eq!(runtime.health_calls.load(Ordering::Relaxed), 2);

        let reopened = init_db(&database.0).expect("completed database should reopen again");
        assert_eq!(
            reopened
                .query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
                .expect("quick check should run"),
            "ok"
        );
        assert_eq!(completed.run_id, checkpoint_run.run_id);
        assert_eq!(
            checkpoint_artifact(
                &reopened,
                &checkpoint_run,
                ContinuationCheckpoint::Synthesized,
            ),
            expected_checkpoint_artifact
        );
        let persisted_run = get_pipeline_run(&reopened, &checkpoint_run.run_id)
            .expect("run should load after reopen")
            .expect("run should persist");
        assert_eq!(
            persisted_run.state,
            if completed.summary.warnings.is_empty() {
                PipelineState::Complete
            } else {
                PipelineState::CompleteWithWarnings
            }
        );
        let persisted_events =
            list_pipeline_events(&reopened, &checkpoint_run.run_id).expect("events should persist");
        assert_eq!(
            &persisted_events[..checkpoint_events.len()],
            checkpoint_events.as_slice()
        );
        assert_eq!(
            get_summary_artifact(&reopened, &checkpoint_run.run_id)
                .expect("summary should load")
                .expect("summary should persist"),
            completed.summary
        );
        assert_eq!(
            get_citation_artifact(&reopened, &checkpoint_run.run_id)
                .expect("citation should load")
                .expect("citation should persist"),
            completed.citations
        );
    }

    #[test]
    fn legacy_mechanical_verified_checkpoint_remains_readable_and_completes_with_warning() {
        let database = TestDatabase::new();
        let source = TestSource::from_fixture();
        let pipeline = TestPipeline::default();
        let run_id = {
            let mut conn = init_db(&database.0).expect("schema should initialize");
            let run = prepare_checkpoint(
                &mut conn,
                &source,
                &pipeline,
                &FixtureRuntime,
                ContinuationCheckpoint::Verified,
            );
            let synthesized = get_synthesized_document(&conn, &run.run_id)
                .expect("synthesis should load")
                .expect("synthesis should exist");
            let mut legacy = get_verified_document(&conn, &run.run_id)
                .expect("verification should load")
                .expect("verification should exist");
            legacy.verification_version = "2.0.0".to_string();
            legacy.runtime_id.clear();
            legacy.model_id.clear();
            legacy.summary_text = synthesized.summary_text.clone();
            legacy.claims = synthesized.claims.clone();
            legacy.claim_verifications.clear();
            legacy.summary_claim_verifications.clear();
            legacy.key_point_claim_ids.clear();
            legacy.warnings = synthesized.warnings.clone();
            legacy.warnings.push(PipelineWarning {
                code: "SEMANTIC_VERIFICATION_DEFERRED".to_string(),
                message: "Citation provenance and artifact integrity were checked; semantic entailment remains deferred"
                    .to_string(),
                stage: Some(PipelineStage::Verify),
            });
            let artifact_json =
                serde_json::to_string(&legacy).expect("legacy verification should serialize");
            let artifact_hash = format!("{:x}", Sha256::digest(artifact_json.as_bytes()));
            conn.execute(
                "UPDATE verified_documents
                 SET verification_version = ?1, artifact_hash = ?2, verified_artifact = ?3
                 WHERE run_id = ?4",
                params![
                    legacy.verification_version,
                    artifact_hash,
                    artifact_json,
                    run.run_id
                ],
            )
            .expect("legacy verification fixture should install");

            let completed = continue_run_to_summary(
                &mut conn,
                &run.run_id,
                run.state_version,
                pipeline.continuation_components(None),
            )
            .expect("legacy verified checkpoint should complete without a runtime");
            assert_eq!(completed.summary.summary_version, "2.0.0");
            assert_eq!(completed.citations.citation_version, "1.0.0");
            assert!(completed
                .summary
                .warnings
                .iter()
                .any(|warning| warning.code == "SEMANTIC_VERIFICATION_DEFERRED"));
            run.run_id
        };

        let reopened = init_db(&database.0).expect("legacy database should independently reopen");
        let persisted = crate::pipeline::workspace::get_persisted_summary(&reopened, &run_id)
            .expect("legacy summary should remain readable");
        assert!(persisted
            .summary
            .warnings
            .iter()
            .any(|warning| warning.code == "SEMANTIC_VERIFICATION_DEFERRED"));
        assert_eq!(
            get_summary_artifact(&reopened, &run_id)
                .expect("legacy summary artifact should load")
                .expect("legacy summary artifact should exist")
                .summary_version,
            "2.0.0"
        );
        assert_eq!(
            get_citation_artifact(&reopened, &run_id)
                .expect("legacy citation artifact should load")
                .expect("legacy citation artifact should exist")
                .citation_version,
            "1.0.0"
        );
        assert_eq!(
            reopened
                .query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
                .expect("quick check should run"),
            "ok"
        );
    }

    #[test]
    fn retry_creates_a_new_traceable_run_and_preserves_the_failed_parent_across_reopen() {
        let database = TestDatabase::new();
        let source = TestSource::from_fixture();
        let pipeline = TestPipeline::default();
        let (parent, parent_events, completed, completed_events) = {
            let mut conn = init_db(&database.0).expect("schema should initialize");
            let parent = create_recoverable_failed_run_with_profile(
                &mut conn,
                &source,
                &pipeline,
                SummaryProfile::Story,
            );
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

            let retry_runtime = SeedRecordingFixtureRuntime::default();
            let completed = retry_failed_run_to_summary(
                &mut conn,
                &parent.run_id,
                parent.state_version,
                pipeline.components(&retry_runtime),
            )
            .expect("retry should complete from the ingested checkpoint");
            assert_ne!(completed.run_id, parent.run_id);
            assert_eq!(completed.document.document_id, parent.document_id);
            let parent_seed = crate::pipeline::summary::generation_seed_for_run(&parent.run_id);
            let retry_seed = crate::pipeline::summary::generation_seed_for_run(&completed.run_id);
            assert_ne!(retry_seed, parent_seed);
            let request_seeds = retry_runtime.seeds();
            assert!(!request_seeds.is_empty());
            assert!(request_seeds.iter().all(|seed| *seed == retry_seed));
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
            assert_eq!(
                db::get_run_model_profile(&conn, &parent.run_id)
                    .expect("parent profile should load"),
                Some(fixture_model_profile())
            );
            assert_eq!(
                db::get_run_model_profile(&conn, &completed.run_id)
                    .expect("retry profile should load"),
                Some(fixture_model_profile())
            );
            assert_eq!(
                db::get_run_summary_profile(&conn, &parent.run_id)
                    .expect("parent summary profile should load"),
                Some(SummaryProfile::Story)
            );
            assert_eq!(
                db::get_run_summary_profile(&conn, &completed.run_id)
                    .expect("retry summary profile should load"),
                Some(SummaryProfile::Story)
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
            assert_eq!(parent_item.summary_profile, SummaryProfile::Story);
            assert_eq!(child_item.summary_profile, SummaryProfile::Story);
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
        assert_eq!(
            db::get_run_summary_profile(&reopened, &completed.run_id)
                .expect("retry summary profile should survive reopen"),
            Some(SummaryProfile::Story)
        );
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
        let (
            run_id,
            summary_hash,
            citation_hash,
            claim_count,
            evidence_count,
            expected_verification,
            expected_state,
        ) = {
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
            let verified = get_verified_document(&conn, &result.run_id)
                .expect("verification should load")
                .expect("verification should exist");
            let accepted_synthesis =
                get_synthesis_attempt(&conn, &result.run_id, verified.synthesis_attempt_ordinal)
                    .expect("accepted synthesis attempt should load")
                    .expect("accepted synthesis attempt should exist");
            assert_eq!(verified.runtime_id, runtime.runtime_id());
            assert_eq!(verified.model_id, runtime.model_id());
            assert_eq!(
                verified.claim_verifications.len(),
                accepted_synthesis.claims.len()
            );
            assert_eq!(verified.claims, result.citations.claims);
            let expected_state = if result.summary.warnings.is_empty() {
                PipelineState::Complete
            } else {
                PipelineState::CompleteWithWarnings
            };
            (
                result.run_id,
                result.summary.integrity_hash,
                result.citations.integrity_hash,
                result.citations.claims.len(),
                result.citations.evidence.len(),
                verified,
                expected_state,
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
        assert_eq!(
            get_verified_document(&reopened, &run_id)
                .expect("verification should load")
                .expect("verification should persist"),
            expected_verification
        );
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
            expected_state
        );
    }
}
