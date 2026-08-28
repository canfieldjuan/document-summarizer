# Domain Contracts

This document describes contracts implemented by the Rust core in
`src-tauri/src/pipeline`. The TypeScript frontend selects files, invokes Tauri
commands, and renders results; it does not assign identity, hash bytes, write
SQLite, or mutate pipeline state.

## `PipelineRun`
Represents an instance of processing a document.

```rust
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
```

`resumable` records checkpoint eligibility metadata only. Automatic active-run
detection and a resume command are not implemented in the current slice.

## `PipelineEvent`
An immutable ledger entry indicating a transition from one state to another.

```rust
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
```

## `PipelineState`
The absolute truth of what step is currently active or completed.

```rust
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
```

## `IngestedDocument`

An ingested document stores a generated `document_id`, original filename, file
type, exact number of bytes read and hashed, SHA-256 content hash, canonical
source path, and creation timestamp. Ingestion opens the source read-only and
does not copy, rewrite, or otherwise modify it.

Before parsing, the source is read again and both its byte size and SHA-256 are
compared with this durable identity. A missing or changed source becomes a
structured parse failure; bytes from a different file are never persisted under
the original document ID.

The Slice 1 duplicate policy is intentionally trace-first: ingesting identical
bytes again creates a new document and a new pipeline run. Their content hashes
match, while both document IDs and run IDs remain independent.

PDF ingestion performs candidate validation only: the selected path must have a
`.pdf` extension and the opened bytes must begin with `%PDF-`. Structural PDF
validation belongs to the parser stage.

## Durable transition boundary

Normal pipeline code changes state only through the SQLite-backed transition
operation, which requires `run_id`, expected state, expected version, and next
state. A successful transaction updates state/version and appends its event.
An invalid or stale transition changes neither. `pipeline_events` has a unique
per-run sequence and SQLite triggers reject update or deletion.

## Long-term replaceability seams

- Parser implementations implement `DocumentParser::parse(&IngestedDocument)`
  and return either `ParsedDocument` or structured `PipelineFailure`. No
  PDF-library type crosses or is persisted at this boundary.
- `NormalizedDocument` is deliberately deferred. Future PDF, DOCX, OCR, and
  vision paths will converge there after parsing or visual extraction.
- `ModelRuntime` is deliberately deferred. Generic run state and SQLite schema
  contain no llama.cpp, ONNX, model, prompt, or inference-runtime assumptions.

## `ParsedDocument` (Slice 2)

```rust
pub struct ParsedDocument {
    pub document_id: String,
    pub parser_id: String,
    pub parser_version: String,
    pub pages: Vec<ParsedPage>,
    pub warnings: Vec<PipelineWarning>,
}

pub struct ParsedPage {
    pub page_number: u32,
    pub text: String,
    pub warnings: Vec<PipelineWarning>,
    pub requires_visual_processing: bool,
}
```

Pages are one-based and stored in canonical source order. Empty native text is
valid: the page remains present, receives `NO_NATIVE_TEXT`, and sets
`requires_visual_processing = true`. If every page lacks native text, the
document also receives `NO_NATIVE_TEXT_IN_DOCUMENT`. These markers route future
OCR/vision work; they do not perform it.

Parsed artifacts are stored by pipeline run, preserving the run-to-document
relationship and parser identity/version. Artifact insertion, the transition
to `PARSED`, state-version increment, and `PARSED` event append share one SQLite
transaction.
