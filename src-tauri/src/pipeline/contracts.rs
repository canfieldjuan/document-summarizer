use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
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
    pub system_prompt: String,
    pub user_prompt: String,
    pub max_output_tokens: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelResponse {
    pub text: String,
    pub runtime_id: String,
    pub model_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRuntimeFailure {
    pub code: String,
    pub message: String,
    pub recoverable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkAnalysis {
    pub chunk_id: String,
    pub summary_text: String,
    pub source_spans: Vec<SourceSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalyzedDocument {
    pub document_id: String,
    pub analysis_version: String,
    pub runtime_id: String,
    pub model_id: String,
    pub chunks: Vec<ChunkAnalysis>,
    pub warnings: Vec<PipelineWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SynthesizedDocument {
    pub document_id: String,
    pub synthesis_version: String,
    pub runtime_id: String,
    pub model_id: String,
    pub summary_text: String,
    pub source_chunk_ids: Vec<String>,
    pub warnings: Vec<PipelineWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedDocument {
    pub document_id: String,
    pub verification_version: String,
    pub summary_text: String,
    pub source_chunk_ids: Vec<String>,
    pub warnings: Vec<PipelineWarning>,
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
    fn health(&self) -> Result<(), ModelRuntimeFailure>;
    fn runtime_id(&self) -> &str;
    fn model_id(&self) -> &str;
}
