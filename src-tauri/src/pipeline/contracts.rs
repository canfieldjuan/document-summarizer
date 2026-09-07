use crate::pipeline::control::ExecutionControl;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PipelineState {
    Received,
    Ingesting,
    Ingested,
    Parsing,
    Parsed,
    VisualAnalysisRequired,
    VisualAnalyzing,
    VisualAnalyzed,
    Normalizing,
    Normalized,
    Structuring,
    Structured,
    Chunking,
    Chunked,
    Analyzing,
    Analyzed,
    Synthesizing,
    Synthesized,
    Verifying,
    Verified,
    Complete,
    CompleteWithWarnings,
    Failed,
    Cancelling,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PipelineStage {
    Ingest,
    Parse,
    Normalize,
    Structure,
    Chunk,
    Analyze,
    Synthesize,
    Verify,
}

impl PipelineState {
    pub(crate) const IMPLEMENTED_ACTIVE_STATES: [Self; 8] = [
        Self::Ingesting,
        Self::Parsing,
        Self::Normalizing,
        Self::Structuring,
        Self::Chunking,
        Self::Analyzing,
        Self::Synthesizing,
        Self::Verifying,
    ];

    pub(crate) fn active_stage(&self) -> Option<PipelineStage> {
        match self {
            Self::Ingesting => Some(PipelineStage::Ingest),
            Self::Parsing => Some(PipelineStage::Parse),
            Self::Normalizing => Some(PipelineStage::Normalize),
            Self::Structuring => Some(PipelineStage::Structure),
            Self::Chunking => Some(PipelineStage::Chunk),
            Self::Analyzing => Some(PipelineStage::Analyze),
            Self::Synthesizing => Some(PipelineStage::Synthesize),
            Self::Verifying => Some(PipelineStage::Verify),
            _ => None,
        }
    }

    pub(crate) fn can_request_cancellation(&self) -> bool {
        self.active_stage().is_some()
            || matches!(
                self,
                Self::Ingested
                    | Self::Parsed
                    | Self::Normalized
                    | Self::Structured
                    | Self::Chunked
                    | Self::Analyzed
                    | Self::Synthesized
                    | Self::Verified
            )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineProgress {
    pub total_units: u32,
    pub completed_units: u32,
    pub failed_units: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineWarning {
    pub code: String,
    pub message: String,
    pub stage: Option<PipelineStage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineFailure {
    pub code: String,
    pub message: String,
    pub stage: Option<PipelineStage>,
    pub recoverable: bool,
}

impl fmt::Display for PipelineFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for PipelineFailure {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineRun {
    pub run_id: String,
    pub document_id: String,
    pub state: PipelineState,
    pub state_version: u32,
    pub pipeline_version: String,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub current_stage: Option<PipelineStage>,
    pub progress: PipelineProgress,
    pub warnings: Vec<PipelineWarning>,
    pub failure: Option<PipelineFailure>,
    pub cancellation_requested: bool,
    pub resumable: bool,
}

impl PipelineRun {
    pub(crate) fn continuation_checkpoint(&self) -> Option<ContinuationCheckpoint> {
        if self.failure.is_some()
            || self.completed_at.is_some()
            || self.cancellation_requested
            || !self.resumable
        {
            return None;
        }

        match self.state {
            PipelineState::Ingested => Some(ContinuationCheckpoint::Ingested),
            PipelineState::Parsed => Some(ContinuationCheckpoint::Parsed),
            PipelineState::Normalized => Some(ContinuationCheckpoint::Normalized),
            PipelineState::Structured => Some(ContinuationCheckpoint::Structured),
            PipelineState::Chunked => Some(ContinuationCheckpoint::Chunked),
            PipelineState::Analyzed => Some(ContinuationCheckpoint::Analyzed),
            PipelineState::Synthesized => Some(ContinuationCheckpoint::Synthesized),
            PipelineState::Verified => Some(ContinuationCheckpoint::Verified),
            _ => None,
        }
    }

    pub(crate) fn retry_checkpoint(&self) -> Option<RetryCheckpoint> {
        let failure = self.failure.as_ref()?;
        if self.state != PipelineState::Failed
            || !self.resumable
            || !failure.recoverable
            || failure.stage.is_none()
            || failure.stage.as_ref() == Some(&PipelineStage::Ingest)
        {
            return None;
        }
        Some(RetryCheckpoint::Ingested)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContinuationCheckpoint {
    Ingested,
    Parsed,
    Normalized,
    Structured,
    Chunked,
    Analyzed,
    Synthesized,
    Verified,
}

impl ContinuationCheckpoint {
    pub fn requires_runtime(self) -> bool {
        !matches!(self, Self::Verified)
    }

    pub fn requires_existing_model_profile(self) -> bool {
        matches!(self, Self::Analyzed | Self::Synthesized)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineEvent {
    pub event_id: String,
    pub run_id: String,
    pub sequence_no: u32,
    pub previous_state: Option<PipelineState>,
    pub next_state: PipelineState,
    pub timestamp: DateTime<Utc>,
    pub stage: Option<PipelineStage>,
    pub work_unit_id: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RetryCheckpoint {
    Ingested,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryLineage {
    pub retry_run_id: String,
    pub source_run_id: String,
    pub checkpoint: RetryCheckpoint,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentInput {
    pub file_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestedDocument {
    pub document_id: String,
    pub original_filename: String,
    pub file_type: String,
    pub byte_size: u64,
    pub content_hash: String,
    pub local_source_path: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedPage {
    pub page_number: u32,
    pub text: String,
    pub warnings: Vec<PipelineWarning>,
    pub requires_visual_processing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedDocument {
    pub document_id: String,
    pub parser_id: String,
    pub parser_version: String,
    pub pages: Vec<ParsedPage>,
    pub warnings: Vec<PipelineWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceType {
    NativeText,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpan {
    pub page_start: u32,
    pub page_end: u32,
    pub section_id: Option<String>,
    pub source_type: SourceType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NormalizedBlockKind {
    Text,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedBlock {
    pub block_id: String,
    pub kind: NormalizedBlockKind,
    pub text: String,
    pub source: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedPage {
    pub page_number: u32,
    pub content: Vec<NormalizedBlock>,
    pub warnings: Vec<PipelineWarning>,
    pub requires_visual_processing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedDocument {
    pub document_id: String,
    pub normalization_version: String,
    pub pages: Vec<NormalizedPage>,
    pub warnings: Vec<PipelineWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructurePage {
    pub page_number: u32,
    pub warnings: Vec<PipelineWarning>,
    pub requires_visual_processing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StructureNodeKind {
    Document,
    Section,
    Subsection,
    Unstructured,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructureNode {
    pub node_id: String,
    pub kind: StructureNodeKind,
    pub title: Option<String>,
    pub level: u32,
    pub block_ids: Vec<String>,
    pub source_spans: Vec<SourceSpan>,
    pub children: Vec<StructureNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredDocument {
    pub document_id: String,
    pub structure_version: String,
    pub pages: Vec<StructurePage>,
    pub nodes: Vec<StructureNode>,
    pub warnings: Vec<PipelineWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentChunk {
    pub chunk_id: String,
    pub ordinal: u32,
    pub structure_node_id: String,
    pub text: String,
    pub block_ids: Vec<String>,
    pub source_spans: Vec<SourceSpan>,
    pub warnings: Vec<PipelineWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkedDocument {
    pub document_id: String,
    pub chunking_version: String,
    pub chunks: Vec<DocumentChunk>,
    pub warnings: Vec<PipelineWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRequest {
    pub stage: PipelineStage,
    pub ordinal: u32,
    pub system_prompt: String,
    pub user_prompt: String,
    pub seed: u64,
    pub max_output_tokens: u32,
    pub output_format: ModelOutputFormat,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ModelOutputFormat {
    #[default]
    Text,
    JsonSchema {
        name: String,
        schema: Value,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelTransportAttempt {
    Primary,
    SchemaFallbackRetry,
    CachedSchemaFallback,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ModelTokenUsage {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRequestAttemptDiagnostic {
    pub stage: PipelineStage,
    pub request_ordinal: u32,
    pub attempt_ordinal: u32,
    pub transport_attempt: ModelTransportAttempt,
    pub elapsed_milliseconds: u64,
    pub configured_output_tokens: u32,
    pub provider_usage: ModelTokenUsage,
    pub succeeded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelResponse {
    pub text: String,
    pub runtime_id: String,
    pub model_id: String,
    pub request_attempts: Vec<ModelRequestAttemptDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelStageProfileSnapshot {
    #[serde(default)]
    pub runtime_kind: ModelRuntimeKind,
    pub profile_id: String,
    pub model_name: String,
    pub model_digest: String,
    pub context_tokens: u32,
    pub tokenizer_version: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRuntimeKind {
    #[default]
    OllamaNative,
    LlamaCppGguf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelProfileSnapshot {
    pub version: u32,
    pub preset_id: String,
    pub analysis: ModelStageProfileSnapshot,
    pub verification: ModelStageProfileSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRuntimeFailure {
    pub code: String,
    pub message: String,
    pub recoverable: bool,
    pub request_attempts: Vec<ModelRequestAttemptDiagnostic>,
}

impl fmt::Display for ModelRuntimeFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ModelRuntimeFailure {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkAnalysis {
    pub chunk_id: String,
    pub summary_text: String,
    pub source_spans: Vec<SourceSpan>,
    #[serde(default)]
    pub evidence: Vec<EvidenceItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceItem {
    pub evidence_id: String,
    pub chunk_id: String,
    pub block_id: String,
    pub claim_text: String,
    pub exact_quote: String,
    pub source_span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CitedClaim {
    pub claim_id: String,
    pub text: String,
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimVerdict {
    Supported,
    Unsupported,
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimVerification {
    pub claim_id: String,
    pub evidence_ids: Vec<String>,
    pub verdict: ClaimVerdict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalyzedDocument {
    pub document_id: String,
    pub analysis_version: String,
    pub runtime_id: String,
    pub model_id: String,
    pub chunks: Vec<ChunkAnalysis>,
    pub warnings: Vec<PipelineWarning>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub omissions: Vec<AnalysisPageOmission>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inspected_pages: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnalysisOmissionOrigin {
    ScanNoise,
    DatePageStamp,
    ModelBareHeading,
    ModelNoSubstantiveContent,
    ParaphraseUnrepairable,
    QuoteBoundaryUnusable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnalysisOmissionReason {
    NonSubstantivePageFurniture,
    NoSubstantiveContent,
    ParaphraseUnrepairable,
    QuoteBoundaryUnusable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisPageOmission {
    pub page_number: u32,
    pub chunk_id: String,
    pub reason: AnalysisOmissionReason,
    pub origin: AnalysisOmissionOrigin,
    pub filter_version: String,
    pub source_fingerprint: String,
    pub catalog_fingerprint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SynthesizedDocument {
    pub document_id: String,
    pub synthesis_version: String,
    pub runtime_id: String,
    pub model_id: String,
    pub summary_text: String,
    pub source_chunk_ids: Vec<String>,
    #[serde(default)]
    pub claims: Vec<CitedClaim>,
    pub warnings: Vec<PipelineWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedDocument {
    pub document_id: String,
    pub verification_version: String,
    #[serde(default)]
    pub synthesis_attempt_ordinal: u32,
    #[serde(default)]
    pub runtime_id: String,
    #[serde(default)]
    pub model_id: String,
    pub summary_text: String,
    pub source_chunk_ids: Vec<String>,
    #[serde(default)]
    pub claims: Vec<CitedClaim>,
    #[serde(default)]
    pub claim_verifications: Vec<ClaimVerification>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub key_point_claim_ids: Vec<String>,
    pub warnings: Vec<PipelineWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CitationArtifact {
    pub document_id: String,
    pub citation_version: String,
    pub summary_integrity_hash: String,
    pub rendered_text: String,
    pub claims: Vec<CitedClaim>,
    pub evidence: Vec<EvidenceItem>,
    pub created_at: DateTime<Utc>,
    pub integrity_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SummaryArtifact {
    pub document_id: String,
    pub summary_version: String,
    pub text: String,
    pub warnings: Vec<PipelineWarning>,
    pub created_at: DateTime<Utc>,
    pub integrity_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletedSummary {
    pub run_id: String,
    pub document: IngestedDocument,
    pub summary: SummaryArtifact,
    pub citations: CitationArtifact,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryArtifacts {
    pub summary: SummaryArtifact,
    pub citations: CitationArtifact,
}

impl SummaryArtifact {
    pub fn calculate_integrity_hash(&self) -> Result<String, serde_json::Error> {
        let canonical = serde_json::to_vec(&(
            &self.document_id,
            &self.summary_version,
            &self.text,
            &self.warnings,
            self.created_at,
        ))?;
        Ok(format!("{:x}", Sha256::digest(canonical)))
    }
}

impl CitationArtifact {
    pub fn calculate_integrity_hash(&self) -> Result<String, serde_json::Error> {
        let canonical = serde_json::to_vec(&(
            &self.document_id,
            &self.citation_version,
            &self.summary_integrity_hash,
            &self.rendered_text,
            &self.claims,
            &self.evidence,
            self.created_at,
        ))?;
        Ok(format!("{:x}", Sha256::digest(canonical)))
    }
}

/// Replaceable boundary between an ingested source and a parser-specific
/// implementation. Downstream stages depend on `ParsedDocument`, never on a
/// PDF crate's types.
pub trait DocumentParser {
    fn parse(&self, document: &IngestedDocument) -> Result<ParsedDocument, PipelineFailure>;
    fn id(&self) -> &'static str;
    fn version(&self) -> &'static str;
}

/// Parser-independent boundary between persisted parser output and every
/// downstream pipeline stage.
pub trait DocumentNormalizer {
    fn normalize(&self, parsed: &ParsedDocument) -> Result<NormalizedDocument, PipelineFailure>;
    fn version(&self) -> &'static str;
}

/// Parser-independent boundary between canonical normalized content and the
/// deterministic structural representation consumed by later stages.
pub trait StructureInterpreter {
    fn interpret(
        &self,
        normalized: &NormalizedDocument,
    ) -> Result<StructuredDocument, PipelineFailure>;
    fn version(&self) -> &'static str;
}

/// Parser- and model-independent boundary between deterministic structure and
/// the bounded source units consumed by later inference stages.
pub trait DocumentChunker {
    fn chunk(
        &self,
        normalized: &NormalizedDocument,
        structured: &StructuredDocument,
    ) -> Result<ChunkedDocument, PipelineFailure>;
    fn version(&self) -> &'static str;
}

/// Replaceable local inference boundary. Generic pipeline state and storage do
/// not depend on a concrete server, model family, or SDK.
pub trait ModelRuntime: Send + Sync {
    fn generate(&self, request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure>;
    fn generate_with_control(
        &self,
        request: &ModelRequest,
        control: &dyn ExecutionControl,
    ) -> Result<ModelResponse, ModelRuntimeFailure> {
        if control.cancellation_requested() {
            return Err(ModelRuntimeFailure {
                code: "MODEL_REQUEST_CANCELLED".to_string(),
                message: "Model request was cancelled before inference".to_string(),
                recoverable: true,
                request_attempts: Vec::new(),
            });
        }
        self.generate(request)
    }
    fn health(&self) -> Result<(), ModelRuntimeFailure>;
    fn runtime_id(&self) -> &str;
    fn model_id(&self) -> &str;
    fn runtime_id_for_stage(&self, _stage: PipelineStage) -> &str {
        self.runtime_id()
    }
    fn model_id_for_stage(&self, _stage: PipelineStage) -> &str {
        self.model_id()
    }
    fn context_tokens(&self, _stage: PipelineStage) -> u32 {
        8_192
    }

    fn profile_snapshot(&self) -> Option<ModelProfileSnapshot> {
        None
    }
}
