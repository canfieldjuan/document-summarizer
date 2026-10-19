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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SummaryProfile {
    General,
    Story,
    Contract,
}

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
    pub source_type: SourceType,
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
    #[serde(default)]
    pub source_type: SourceType,
    pub pages: Vec<ParsedPage>,
    pub warnings: Vec<PipelineWarning>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceType {
    #[default]
    NativeText,
    OcrText,
}

impl SourceType {
    pub fn is_textual(self) -> bool {
        matches!(self, Self::NativeText | Self::OcrText)
    }
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRequest {
    pub stage: PipelineStage,
    pub ordinal: u32,
    pub system_prompt: String,
    pub user_prompt: String,
    pub seed: u64,
    pub max_output_tokens: u32,
    pub output_format: ModelOutputFormat,
}

impl ModelRequest {
    pub(crate) fn integrity_sha256(&self) -> Result<String, serde_json::Error> {
        Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(self)?)))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    InferenceGateway,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admission_withhold: Option<AdmissionWithhold>,
    pub claim_id: String,
    pub evidence_ids: Vec<String>,
    pub verdict: ClaimVerdict,
}

impl ClaimVerification {
    pub(crate) fn has_valid_admission_carrier(&self) -> bool {
        self.admission_withhold.is_none() || self.verdict == ClaimVerdict::Ambiguous
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmissionLimits {
    pub context_tokens: u32,
    pub schema_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionWithholdReason {
    GoverningClauseScalars,
    CombinedScalars,
    SourceEnumValues,
    ClaimEnumValues,
    SchemaBytes,
    RequestCharacters,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmissionWithhold {
    pub verification_version: String,
    pub input_sha256: String,
    pub limits: AdmissionLimits,
    pub reason: AdmissionWithholdReason,
    pub observed: usize,
    pub minimum: usize,
    pub maximum: usize,
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SummaryPresentationMode {
    #[default]
    LegacyClaimList,
    Coherent,
    ClaimLedgerFallback,
    StructuredExtraction,
}

/// Exact source extraction, not generated legal interpretation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ContractExtraction {
    pub clauses: Vec<ExtractedContractClause>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub key_terms: Vec<ContractKeyTerm>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContractTermCategory {
    #[serde(rename = "parties")]
    Parties,
    #[serde(rename = "payment")]
    Payment,
    #[serde(rename = "term/renewal")]
    TermRenewal,
    #[serde(rename = "termination")]
    Termination,
    #[serde(rename = "insurance")]
    Insurance,
    #[serde(rename = "liability/indemnity")]
    LiabilityIndemnity,
}

impl ContractTermCategory {
    pub const ALL: [Self; 6] = [
        Self::Parties,
        Self::Payment,
        Self::TermRenewal,
        Self::Termination,
        Self::Insurance,
        Self::LiabilityIndemnity,
    ];
    pub const fn label(self) -> &'static str {
        match self {
            Self::Parties => "parties",
            Self::Payment => "payment",
            Self::TermRenewal => "term/renewal",
            Self::Termination => "termination",
            Self::Insurance => "insurance",
            Self::LiabilityIndemnity => "liability/indemnity",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContractSelectionRule {
    Heading,
    OpeningBetween,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ContractSectionSelection {
    pub rule: ContractSelectionRule,
    pub root_clause_id: String,
    pub clause_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ContractKeyTerm {
    pub category: ContractTermCategory,
    pub selections: Vec<ContractSectionSelection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ExtractedContractClause {
    pub clause_id: String,
    pub heading: Option<String>,
    pub text: String,
    pub evidence_ids: Vec<String>,
}

pub(crate) const WHOLE_DOCUMENT_SYNTHESIS_VERSION: &str = "15.0.0";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WholeDocumentProgress {
    pub document_id: String,
    pub policy_version: String,
    pub source_sha256: String,
    pub runtime_id: String,
    pub model_id: String,
    pub context_tokens: u32,
    pub verification_limits: AdmissionLimits,
    pub generation_seed: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_profile: Option<ModelProfileSnapshot>,
    pub sections: Vec<WholeDocumentSection>,
    pub assembly_lineage: Option<Vec<(usize, usize, usize)>>,
}

impl WholeDocumentProgress {
    pub(crate) fn has_canonical_status_order(&self) -> bool {
        let mut open = false;
        for section in &self.sections {
            match section.state {
                WholeDocumentSectionState::Completed { .. }
                | WholeDocumentSectionState::Split { .. }
                    if !open => {}
                WholeDocumentSectionState::Pending => open = true,
                WholeDocumentSectionState::Dispatched
                | WholeDocumentSectionState::Failed { .. }
                | WholeDocumentSectionState::Saturated { .. }
                    if !open =>
                {
                    open = true
                }
                _ => return false,
            }
        }
        !self.sections.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WholeDocumentSection {
    pub primary: Vec<usize>,
    pub request: ModelRequest,
    pub request_sha256: String,
    pub state: WholeDocumentSectionState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum WholeDocumentSectionState {
    Pending,
    Dispatched,
    Completed {
        response: WholeDocumentSectionResponse,
    },
    Saturated {
        response: WholeDocumentSectionResponse,
    },
    Split {
        response: WholeDocumentSectionResponse,
    },
    Failed {
        failure: PipelineFailure,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WholeDocumentSectionResponse {
    pub text: String,
    pub response_sha256: String,
    pub runtime_id: String,
    pub model_id: String,
    pub request_attempts: Vec<ModelRequestAttemptDiagnostic>,
}

impl WholeDocumentSection {
    pub(crate) fn permits_dispatch(&self, next: &Self) -> bool {
        if self.primary != next.primary
            || !matches!(self.state, WholeDocumentSectionState::Pending)
            || !matches!(next.state, WholeDocumentSectionState::Dispatched)
        {
            return false;
        }
        let mut expected = self.request.clone();
        let (
            ModelOutputFormat::JsonSchema { schema: old, .. },
            ModelOutputFormat::JsonSchema { schema: new, .. },
        ) = (&mut expected.output_format, &next.request.output_format)
        else {
            return false;
        };
        let (Some(old_count), Some(new_count)) = (
            old["properties"]["units"]["maxItems"].as_u64(),
            new["properties"]["units"]["maxItems"].as_u64(),
        ) else {
            return false;
        };
        if new_count == 0 || new_count > old_count {
            return false;
        }
        old["properties"]["units"]["maxItems"] = serde_json::json!(new_count);
        let Ok(mut payload) = serde_json::from_str::<serde_json::Value>(&expected.user_prompt)
        else {
            return false;
        };
        payload["maximum_units"] = serde_json::json!(new_count);
        expected.user_prompt = payload.to_string();
        expected.max_output_tokens = next.request.max_output_tokens;
        expected == next.request
            && next.request.integrity_sha256().ok().as_ref() == Some(&next.request_sha256)
    }
}

impl WholeDocumentProgress {
    /// A saturated request is immutable history. Only its smaller pending
    /// children are new work; completed siblings and all header pins are kept.
    pub(crate) fn permits_split(&self, next: &Self) -> bool {
        let Some(index) = self
            .sections
            .iter()
            .position(|s| matches!(s.state, WholeDocumentSectionState::Saturated { .. }))
        else {
            return false;
        };
        let added = next.sections.len().saturating_sub(self.sections.len());
        if added < 2 || self.sections[..index] != next.sections[..index] {
            return false;
        }
        let parent = &self.sections[index];
        let mut expected = parent.clone();
        let WholeDocumentSectionState::Saturated { response } = &parent.state else {
            return false;
        };
        expected.state = WholeDocumentSectionState::Split {
            response: response.clone(),
        };
        if next.sections[index] != expected || parent.primary.len() < 2 {
            return false;
        }
        let children = &next.sections[index + 1..index + 1 + added];
        if children.iter().any(|s| {
            s.primary.is_empty()
                || s.primary.len() >= parent.primary.len()
                || !matches!(s.state, WholeDocumentSectionState::Pending)
        }) || children
            .iter()
            .flat_map(|s| s.primary.iter())
            .ne(parent.primary.iter())
        {
            return false;
        }
        for (old, new) in self.sections[index + 1..]
            .iter()
            .zip(&next.sections[index + 1 + added..])
        {
            if !matches!(old.state, WholeDocumentSectionState::Pending) {
                return false;
            }
            let mut expected = old.clone();
            let Some(ordinal) = expected.request.ordinal.checked_add(added as u32) else {
                return false;
            };
            expected.request.ordinal = ordinal;
            let Ok(hash) = expected.request.integrity_sha256() else {
                return false;
            };
            expected.request_sha256 = hash;
            if expected != *new {
                return false;
            }
        }
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SynthesizedDocument {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub whole_document_progress: Option<WholeDocumentProgress>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract_extraction: Option<ContractExtraction>,
    pub document_id: String,
    pub synthesis_version: String,
    pub runtime_id: String,
    pub model_id: String,
    #[serde(default)]
    pub presentation_mode: SummaryPresentationMode,
    pub summary_text: String,
    pub source_chunk_ids: Vec<String>,
    #[serde(default)]
    pub summary_claims: Vec<CitedClaim>,
    #[serde(default)]
    pub synthesis_evidence: Vec<EvidenceItem>,
    #[serde(default)]
    pub claims: Vec<CitedClaim>,
    pub warnings: Vec<PipelineWarning>,
}

impl SynthesizedDocument {
    pub(crate) fn has_consistent_progress_role(&self) -> bool {
        match &self.whole_document_progress {
            Some(progress) => {
                self.synthesis_version == WHOLE_DOCUMENT_SYNTHESIS_VERSION
                    && progress.policy_version == self.synthesis_version
                    && progress.document_id == self.document_id
                    && progress.runtime_id == self.runtime_id
                    && progress.model_id == self.model_id
            }
            None => self.synthesis_version != WHOLE_DOCUMENT_SYNTHESIS_VERSION,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedDocument {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract_extraction: Option<ContractExtraction>,
    pub document_id: String,
    pub verification_version: String,
    #[serde(default)]
    pub synthesis_attempt_ordinal: u32,
    #[serde(default)]
    pub runtime_id: String,
    #[serde(default)]
    pub model_id: String,
    #[serde(default)]
    pub presentation_mode: SummaryPresentationMode,
    pub summary_text: String,
    pub source_chunk_ids: Vec<String>,
    #[serde(default)]
    pub summary_claims: Vec<CitedClaim>,
    #[serde(default)]
    pub synthesis_evidence: Vec<EvidenceItem>,
    #[serde(default)]
    pub summary_claim_verifications: Vec<ClaimVerification>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract_extraction: Option<ContractExtraction>,
    pub document_id: String,
    pub citation_version: String,
    pub summary_integrity_hash: String,
    pub rendered_text: String,
    #[serde(default)]
    pub presentation_mode: SummaryPresentationMode,
    #[serde(default)]
    pub summary_claims: Vec<CitedClaim>,
    pub claims: Vec<CitedClaim>,
    pub evidence: Vec<EvidenceItem>,
    pub created_at: DateTime<Utc>,
    pub integrity_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SummaryArtifact {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract_extraction: Option<ContractExtraction>,
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
        if let Some(extraction) = &self.contract_extraction {
            return Ok(format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&(
                    &self.document_id,
                    &self.summary_version,
                    &self.text,
                    &self.warnings,
                    self.created_at,
                    extraction,
                ))?)
            ));
        }
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
        let canonical = if self.citation_version == "5.0.0" {
            serde_json::to_vec(&(
                &self.document_id,
                &self.citation_version,
                &self.summary_integrity_hash,
                &self.rendered_text,
                &self.presentation_mode,
                &self.summary_claims,
                &self.claims,
                &self.evidence,
                self.created_at,
                &self.contract_extraction,
            ))?
        } else if self.citation_version == "4.0.0" {
            serde_json::to_vec(&(
                &self.document_id,
                &self.citation_version,
                &self.summary_integrity_hash,
                &self.rendered_text,
                &self.presentation_mode,
                &self.summary_claims,
                &self.claims,
                &self.evidence,
                self.created_at,
            ))?
        } else {
            serde_json::to_vec(&(
                &self.document_id,
                &self.citation_version,
                &self.summary_integrity_hash,
                &self.rendered_text,
                &self.claims,
                &self.evidence,
                self.created_at,
            ))?
        };
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

pub(crate) const CLAIM_COMPARISON_SCHEMA_NAME: &str = "document_claim_comparisons_v5";
pub(crate) const MAX_CLAIM_COMPARISON_SCHEMA_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_RESPONSE_SCHEMA_BYTES: usize = 64 * 1024;

pub(crate) fn response_schema_byte_limit(name: &str) -> usize {
    if name == CLAIM_COMPARISON_SCHEMA_NAME {
        MAX_CLAIM_COMPARISON_SCHEMA_BYTES
    } else {
        MAX_RESPONSE_SCHEMA_BYTES
    }
}

/// Replaceable local inference boundary. Generic pipeline state and storage do
/// not depend on a concrete server, model family, or SDK.
pub trait ModelRuntime: Send + Sync {
    /// Scopes a pre-run operation's durable identity to this runtime's request
    /// contract. The default preserves existing operation identities.
    fn request_owner_contract(&self, operation_contract: &str) -> String {
        operation_contract.to_string()
    }
    /// Binds the durable owner of future requests to this worker-local runtime.
    /// Direct runtimes do not require the identity and retain the inert default.
    fn bind_request_owner(&mut self, _owner_id: &str) {}
    /// Binds a pipeline run as the request owner while preserving existing
    /// worker call sites and direct-runtime behavior.
    fn bind_run(&mut self, run_id: &str) {
        self.bind_request_owner(run_id);
    }
    fn generate(&self, request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure>;
    /// Runs runtime-specific request admission without starting generation.
    /// Runtimes without a stricter admission boundary retain the caller's bound.
    fn preflight_request(&self, _request: &ModelRequest) -> Result<(), ModelRuntimeFailure> {
        Ok(())
    }
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
    /// Whether the runtime task can represent a named response protocol.
    /// Size limits remain independent of protocol support.
    fn supports_response_schema(&self, _name: &str) -> bool {
        true
    }
    /// The caller must respect any tighter transport limit before generation.
    fn response_schema_byte_limit(&self, _stage: PipelineStage, name: &str) -> usize {
        response_schema_byte_limit(name)
    }
    fn context_tokens(&self, _stage: PipelineStage) -> u32 {
        8_192
    }

    fn profile_snapshot(&self) -> Option<ModelProfileSnapshot> {
        None
    }
}
