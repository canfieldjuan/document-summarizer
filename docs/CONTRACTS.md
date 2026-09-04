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

`resumable` records checkpoint eligibility metadata. Desktop startup detects
implemented active-stage states and reconciles work interrupted by a previous
process to a retryable `FAILED` result. An eligible failed run can create a
separate retry run from the durable `INGESTED` checkpoint, while stable
checkpoints can be continued explicitly on the same run. Startup never replays
work automatically.

`cancellation_requested` is a durable marker, not a frontend-owned flag. It is
set only in the same compare-and-set transaction that enters `CANCELLING`,
increments `state_version`, and appends the request event. It remains true in
terminal `CANCELLED` history.

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

## Desktop process ownership and interrupted-run recovery

The desktop registers Tauri's single-instance plugin before setup or any other
plugin. Only the owning process opens the application workflow. A second launch
exits after its callback asks the existing window to unminimize, show, and take
focus, preventing startup recovery from mistaking another live desktop
process's work for an interruption.

After single-instance ownership is established and schema migration completes,
startup scans persisted runs in deterministic `run_id` order. The implemented
active states `INGESTING`, `PARSING`, `NORMALIZING`, `STRUCTURING`, `CHUNKING`,
`ANALYZING`, `SYNTHESIZING`, and `VERIFYING` map explicitly to their pipeline
stages. Each receives a structured, recoverable `PROCESS_INTERRUPTED` failure.
A durable `CANCELLING` run instead completes as `CANCELLED` without replaying
work. Recovery requires its durable cancellation marker; an inconsistent
`CANCELLING` row is rejected rather than rewritten.

All recovered state/version updates and immutable events commit in one SQLite
transaction; a stale expectation or any event/write failure rolls back the
entire recovery batch. Stable and terminal states are untouched, and a repeated
startup adds no event or version increment.

Recovery preserves the document record, original source, completed checkpoint
artifacts, warnings, and prior history. It never invokes a parser or model and
does not delete partial files. `init_db` remains schema/persistence-only; the
desktop invokes reconciliation explicitly after acquiring process ownership so
an ordinary second database connection cannot steal active work. Reserved
visual-analysis execution remains outside this policy.

## Explicit retry lineage

Retry never changes a failed run or appends new events to it. A retry creates a
new `PipelineRun` for the same `document_id` and persists an immutable
`RetryLineage` relation:

```rust
pub struct RetryLineage {
    pub retry_run_id: String,
    pub source_run_id: String,
    pub checkpoint: RetryCheckpoint,
    pub created_at: DateTime<Utc>,
}
```

Slice 9 admits only the `INGESTED` checkpoint. The source must be `FAILED`,
`resumable`, carry a recoverable failure from a stage after ingestion, and have
no existing direct retry. One source attempt can create one direct child;
another retry may originate from that child if it later fails. This forms an
auditable chain without rewriting terminal history.

The child run, lineage row, `RECEIVED -> INGESTING -> INGESTED -> PARSING`
state/version updates, and four immutable events commit in one SQLite
transaction. The events identify retry creation, checkpoint reuse, and parser
work admission. Committing the child as active closes the crash window between
checkpoint creation and parser startup: an interruption before parsing
completes is handled by ordinary active-stage recovery. A stale source version,
invalid source, duplicate child, event failure, or lineage write failure leaves
no partial child.

After the atomic admission transaction, the ordinary parser/runtime-neutral
pipeline continues from active `PARSING`. The parser reads the persisted source
path, verifies byte size and SHA-256 against the shared document identity, and
parses those verified bytes. A missing or changed source therefore fails the
child attempt durably without altering its parent. Retry is user initiated;
startup never invokes a parser or model.

## Long-term replaceability seams

- Parser implementations implement `DocumentParser::parse(&IngestedDocument)`
  and return either `ParsedDocument` or structured `PipelineFailure`. No
  PDF-library type crosses or is persisted at this boundary.
- `DocumentNormalizer` accepts only parser-neutral `ParsedDocument` and emits
  the canonical `NormalizedDocument` consumed by future downstream stages.
  PDF-library types are not part of either normalization contract. Future DOCX,
  OCR, and vision paths must converge into this canonical representation with
  an appropriate `SourceType`.
- `ModelRuntime` accepts a provider-neutral `ModelRequest` and returns either a
  `ModelResponse` or structured `ModelRuntimeFailure`. Generic run state and
  artifact persistence contain no model-family or SDK types. The first adapter
  uses an OpenAI-compatible exact-loopback HTTP endpoint and can be replaced
  without changing analysis, synthesis, verification, or Connect contracts.

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

## `NormalizedDocument` (Slice 3)

```rust
pub struct NormalizedDocument {
    pub document_id: String,
    pub normalization_version: String,
    pub pages: Vec<NormalizedPage>,
    pub warnings: Vec<PipelineWarning>,
}

pub struct NormalizedPage {
    pub page_number: u32,
    pub content: Vec<NormalizedBlock>,
    pub warnings: Vec<PipelineWarning>,
    pub requires_visual_processing: bool,
}

pub struct NormalizedBlock {
    pub block_id: String,
    pub kind: NormalizedBlockKind,
    pub text: String,
    pub source: SourceSpan,
}

pub struct SourceSpan {
    pub page_start: u32,
    pub page_end: u32,
    pub section_id: Option<String>,
    pub source_type: SourceType,
}
```

Normalization version `1.0.0` creates one `Text` block for each non-empty
native-text page. Empty pages remain in position with no blocks and retain their
warnings and visual-processing marker. Block IDs are deterministic SHA-256
identities derived from document ID, normalization version, page number, and
block order. Current spans are page-local with `source_type = NativeText` and no
section ID.

Permitted cleaning is limited to normalizing CRLF/CR line endings to LF and
removing NUL while preserving all other controls, tabs, spaces, blank lines,
Unicode, punctuation, and substantive source text. Any cleanup adds
`REPRESENTATION_CLEANUP_APPLIED`; no repeated header/footer removal or structural
interpretation occurs.

Validation requires matching document identity and normalization version,
unchanged page count/order/numbers, preserved warnings and visual-routing
markers, exact text after the permitted cleanup, deterministic unique block
IDs, and page-local source spans that reference existing pages. The schema-v3
normalized artifact stores its version, document/run association, serialized
artifact JSON, and SHA-256 integrity hash. Artifact insertion, `NORMALIZING ->
NORMALIZED`, state-version increment, and event append commit in one SQLite
transaction; retrieval verifies the stored hash, artifact metadata, and run-to-document association.

## `StructuredDocument` (Slice 4)

```rust
pub struct StructuredDocument {
    pub document_id: String,
    pub structure_version: String,
    pub pages: Vec<StructurePage>,
    pub nodes: Vec<StructureNode>,
    pub warnings: Vec<PipelineWarning>,
}

pub struct StructureNode {
    pub node_id: String,
    pub kind: StructureNodeKind,
    pub title: Option<String>,
    pub level: u32,
    pub block_ids: Vec<String>,
    pub source_spans: Vec<SourceSpan>,
    pub children: Vec<StructureNode>,
}
```

`StructureInterpreter` accepts only `NormalizedDocument`; no parser or PDF type
crosses the boundary. Structure version `1.0.0` emits one empty `Document` root
whose ordered children are `Section`, `Subsection`, or `Unstructured` nodes.
Each normalized block has exactly one canonical owning node. Nodes store block
IDs and their unchanged `SourceSpan` values, not rewritten source text.

Version 1 recognizes only a numeric prefix on the first non-empty block line.
Supported forms include `1`, `1. Introduction`, `1 Introduction`, `1) Introduction`,
and nested forms such as `1.1 Purpose`. A titled signal must begin with an
uppercase character, remain within 12 words and 120 characters, and not end as
a sentence. Components are `1..=999` and hierarchy depth is capped at six. A
nested signal is accepted only while its complete parent prefix is active;
duplicate, orphaned, malformed, year-like, lowercase-sentence, and other
ambiguous signals remain ordinary content. All-caps, short-line, filename, and
layout-based title guessing are not implemented.

Unnumbered blocks before the first accepted section are grouped as
`Unstructured`. After a section begins, ordinary blocks remain owned by the
active deepest section until another accepted heading changes the hierarchy.
This permits deterministic multi-page continuation. Page metadata is copied to
`StructurePage`, so empty/visual pages retain their order, warnings, and visual
routing without invented nodes.

Validation requires matching document/version identity, exact page metadata,
one deterministic root, valid parent/level relationships, deterministic unique
node IDs, heading labels and levels re-derived from their first source block,
exact source spans, and depth-first block references equal to every normalized
block exactly once in canonical order. The owned recursive value
representation cannot encode reference cycles. Schema v4 persists the artifact,
document/run association, structure version, serialized JSON, and SHA-256
integrity hash. Artifact insertion, `STRUCTURING -> STRUCTURED`, state-version
increment, and immutable event append share one transaction.

## `ChunkedDocument` (Slice 5)

```rust
pub struct ChunkedDocument {
    pub document_id: String,
    pub chunking_version: String,
    pub chunks: Vec<DocumentChunk>,
    pub warnings: Vec<PipelineWarning>,
}

pub struct DocumentChunk {
    pub chunk_id: String,
    pub ordinal: u32,
    pub structure_node_id: String,
    pub text: String,
    pub block_ids: Vec<String>,
    pub source_spans: Vec<SourceSpan>,
    pub warnings: Vec<PipelineWarning>,
}
```

`DocumentChunker` accepts canonical `NormalizedDocument` plus its
`StructuredDocument`; it has no parser, PDF, UI, or model dependency. Chunking
version `1.0.0` groups blocks only within a top-level structural owner and
splits between normalized blocks when adding another block would exceed the
12,000-character target. A single oversized block remains intact and receives
`CHUNK_EXCEEDS_TARGET`; chunking never rewrites or truncates authoritative
normalized text to satisfy the target.

Chunk text is the exact ordered block text joined by two LF characters for
later prompt construction. Each deterministic chunk ID is derived from the
document, chunking version, ordinal, structural owner, block IDs, and resulting
text. Validation requires canonical ordinals, unique deterministic identities,
one top-level owner per chunk, exact block/source-span association, and 100%
normalized-block coverage exactly once in source order. Documents without
native text produce no invented chunks and receive `NO_TEXT_TO_CHUNK`.

Explicit schema v5 persists the chunking version, artifact JSON, document/run
association, creation time, and SHA-256 integrity hash. Artifact insertion,
`CHUNKING -> CHUNKED`, state-version update, and immutable event append share
one transaction. Retrieval verifies integrity and stored metadata before the
artifact is returned.

## Local summary artifacts

`ModelRuntime` is the only inference boundary. A request may select plain text
or a named, bounded JSON Schema output contract. Analysis version `2.0.0`
requires each chunk response to contain one to five structured evidence items;
the prompt requests three to five material items when the source supports that
coverage, asks for the shortest sufficient source quotation, and uses a
1,024-token response budget. Rust accepts an evidence item
only when its block ID belongs to that chunk and its non-whitespace characters
match a bounded contiguous region of the authoritative normalized block
exactly. PDF line-wrap whitespace may be reconciled deterministically, but the
persisted quotation is always copied from the normalized source itself.
Punctuation, case, word, order, identity, and size changes remain invalid. The
application derives the `SourceSpan`, chunk association, exact stored quote,
and deterministic evidence ID; the model cannot supply or override those
fields.

If a generated chunk response fails the evidence JSON/source contract, analysis
may make exactly one replacement request. Rust first derives a deterministic
catalog of at most 48 bounded, contiguous quotations from that chunk's
authoritative normalized blocks, with at most 12,000 quotation characters in
the catalog. Every candidate has an application-issued quote ID and fixed block
provenance. The repair model may return one to three supplied quote IDs plus
claim text; it cannot supply or alter quotation text or block identity. Rust
rejects unknown or duplicate IDs and materializes the exact quote, block,
`SourceSpan`, and evidence ID from the catalog. Nothing from the rejected
response is persisted. The replacement must validate in full or the run fails
normally; a successful replacement adds `MODEL_EVIDENCE_RESPONSE_REPAIRED` to
the durable warning set. Runtime and response-identity failures are not retried
by this contract.

Synthesis version `3.0.0` receives only the validated evidence catalog and
returns structured claims. A catalog that fits one bounded request keeps the
direct path. Larger catalogs are partitioned deterministically in source order,
with at most eight items and 16,000 Unicode characters per request. Each first
pass produces at most four intermediate claims. Candidate reductions use the
same request bounds and must reduce every non-final batch by at least half.
The exact per-request `maximum_claims` is present in both the JSON Schema and
the serialized user prompt, so Ollama's validated JSON-object fallback receives
the same bound even when server-side grammar loading is unavailable. The
complete plan is conservatively capped at 256 model requests.

Intermediate candidates are ephemeral and have deterministic IDs derived from
the document, synthesis version, reduction round, batch/order, text, and
original evidence IDs. A model response may reference only evidence or
candidate IDs present in that exact request. Rust expands candidate references
back to unique original evidence IDs, restores canonical source order, and
rejects claims expanding beyond 16 evidence items. Only final claims enter the
durable `SynthesizedDocument`; Rust derives their deterministic IDs and renders
page labels from authoritative source spans. The artifact still references
every chunk ID exactly once in source order. Historical synthesis version
`2.0.0` artifacts remain validation-compatible and retain their original claim
identity derivation.

Verification version `3.0.0` asks `ModelRuntime` to classify every synthesized
claim against only its validated exact quotations. Rust validates the complete
catalog and its aggregate size before checking runtime health, then processes
it in deterministic batches of at most 16 claims with a bounded output budget.
Each response may contain only the application-issued claim ID and one of
`supported`, `unsupported`, or `ambiguous`. Rust requires exactly one unique
verdict for every synthesized claim in canonical order and restores the
evidence IDs from the persisted synthesis; the model cannot add a claim, choose
provenance, or rewrite source text. `VerifiedDocument` records runtime/model
identity and the complete claim-to-evidence verdict catalog for audit. Its
displayed claims and rendered text contain only claims classified as supported.

This is model-assisted evidence-entailment screening, not independent fact
checking. `supported` means the selected runtime classified every material
detail as directly entailed by the supplied quotations. It does not certify
that the source itself is factually correct. Unsupported or ambiguous claims
remain in the durable verdict artifact but are withheld from the final summary
and add `SEMANTIC_CLAIMS_WITHHELD`. If no claim is supported, the verdict
artifact commits at `VERIFIED`, then the run transitions to `FAILED` with
`NO_SEMANTICALLY_SUPPORTED_CLAIMS`; no summary or citation artifact is created.

The supported runtime adapter is Ollama through its loopback OpenAI-compatible
API. It defaults to `http://127.0.0.1:11434/v1/` and
`qwen3-30b-a3b:latest`. Each generation request has a 900-second default
deadline, while connection and health checks retain separate shorter limits;
`DOC_SUM_MODEL_BASE_URL`, `DOC_SUM_MODEL_NAME`,
`DOC_SUM_MODEL_TIMEOUT_SECONDS`, and optional
`DOC_SUM_MODEL_API_TOKEN_FILE` remain deployment overrides. The adapter accepts
only plain HTTP on exact IPv4 or IPv6 loopback, uses bounded connect, health, and
response limits, disables proxies and redirects, and reads only this
application's optional bounded token file. It never reads another application's
credential store. Document and evidence text are marked as untrusted data in
both prompts. Generation requests use temperature zero, fixed seed `42`, and no
reasoning effort. The adapter maps the schema request to Ollama's
OpenAI-compatible structured-output
field; Rust parses and validates the returned JSON before it can become a
pipeline artifact. Before transport, the adapter derives a non-mutating decoder
projection of the canonical schema. It omits `uniqueItems`, which vLLM does not
implement, and `maxLength`, whose large bounded values Ollama expands into
grammar repetitions that it refuses to compile. The projection retains object
closure, required fields, enums, non-empty strings, and array count bounds.
Rust remains authoritative for string size and reference uniqueness and rejects
oversized text, duplicate or foreign references, non-source quotations, and
other contract-invalid output before persistence. Model output cannot invoke
pipeline actions.
The adapter retains a defensive compatibility path for the exact Ollama server
error `failed to load model vocabulary required for format`: it caches that
failure and retries with Ollama's JSON-object mode rather than unconstrained
text. Current pipeline schemas are expected to use the compatible projected
schema instead. The prompt still carries the explicit JSON shape and the same
Rust schema, identity, quotation, provenance, and size checks remain mandatory.
Other HTTP failures do not activate the fallback.

Analysis remains version `2.0.0`; synthesis, verification, and summary are
version `3.0.0`, and citation is version `2.0.0`. No schema migration is
required because the existing synthesis, verification, summary, and citation
tables already persist explicitly versioned JSON plus row hashes. Each artifact
row records the run/document, stage version, serialized artifact, creation
timestamp, and SHA-256 row hash. The final `SummaryArtifact` and
`CitationArtifact` each carry their own content integrity hash, and the
citation artifact binds to the exact summary integrity hash and rendered text.
Retrieval revalidates the verdict artifact against the original synthesized
claim catalog before accepting current citations. Historical summary version
`1.0.0` rows remain readable without inventing citations. Historical mechanical
verification `2.0.0` remains readable and can produce its paired summary
`2.0.0` and citation `1.0.0` with `SEMANTIC_VERIFICATION_DEFERRED` intact.

The application service composes ingestion, parsing, normalization, structural
interpretation, chunking, analysis, synthesis, verification, and completion.
Thin Tauri commands select concrete adapters, invoke that service, report
Ollama readiness, and expose UI-neutral read models for recent runs and
persisted summaries. The frontend owns selection and display only; it cannot
query SQLite, mutate pipeline state, or construct a summary artifact. Command
responses expose only presentation fields. For current summaries this includes
claim text, application-derived page labels, and exact source quotations;
private source paths, normalized block IDs, chunk IDs, and artifact integrity
metadata remain inside the Rust/persistence boundary. The frontend selects and
displays citations but does not calculate provenance or validate model output.

Recent-run history is capped at 30 items and ordered deterministically by
persisted `updated_at` then `run_id`, both descending. Its read model includes
document identity, filename, byte size, state/version, timestamps, warnings,
failure, and whether a summary row exists, but not the private source path.
Opening a result reuses the integrity-validating summary retrieval path and
requires a completed run. A corrupted artifact therefore remains visible as a
historical run but fails closed when opened. Connection reopen reconstructs the
same read model from durable state; history is not a frontend cache.

The Tauri CLI owns the `custom-protocol` mode switch. The Cargo feature is in the
crate's default feature set, so Tauri release builds embed `frontendDist`; Tauri
development invokes Cargo with `--no-default-features` and loads the configured
Vite `devUrl`. The application configuration must not force the feature for both
modes. A raw featureless release compilation fails at compile time rather than
producing an executable that silently depends on the development server.

The release product identity is `Document Summarizer`; its Cargo and installed
binary name is `document-summarizer`. Cargo automatic binary discovery remains
disabled, and diagnostic PDF probes live under `src-tauri/tools/legacy` rather
than `src/bin`, so they cannot become application or package targets. The base
Tauri bundle configuration remains portable. Its Linux overlay selects only the
currently verified Debian package; AppImage, RPM, macOS, and Windows packaging
remain separate target-platform work. A supported Linux package must contain the
desktop executable, desktop entry, and icons without legacy probe executables.

Current limits are conservative: source chunks and the aggregate
claim-verification input are capped at 100,000 Unicode characters. Synthesis
has no single aggregate prompt; every direct, evidence-batch, and candidate
request is capped at eight items and 16,000 Unicode characters, and a hierarchy
is capped at 256 requests. A native-text-free document, an individually
oversized synthesis item, or a plan beyond those limits fails with a structured
domain error; no summary text is invented. Intermediate reductions are not
persisted or treated as checkpoints; any later synthesis attempt must recompute
them under the existing failure/retry policy. Cancellation is checked before
and after every model request.
Analysis evidence, summary claims, quotation length, evidence references per
claim, response schemas, and model response bytes are also bounded. Independent
source-fact verification, OCR/vision routing, PDF-viewer navigation, and
citations for visual-only pages remain deferred.

## Pending summary coverage and model-request contract

**Status:** behavioral contract proposed for review, not yet implemented. The
preceding section remains the description of current behavior until a separate
implementation commit satisfies every requirement below. When that
implementation is accepted, its documentation change must fold these rules into
the preceding current-behavior section and delete this entire pending section;
the two descriptions must not remain as parallel sources of truth.

### Root cause

The current stage limits do not compose into a document-level coverage
guarantee. Analysis admits only a small fixed evidence set per chunk, while the
hierarchical synthesis path repeatedly reduces candidates and can offer fewer
final claim slots than the direct path. Verification can then withhold any
number of claims while still allowing a non-empty result to complete. A larger
document can therefore receive a thinner accepted summary than a smaller one.

A page-derived claim budget alone does not repair that mismatch. Evidence is
currently produced per character-bounded chunk, so sparse documents can have a
large page-derived claim budget but only one small evidence set. Requiring every
available evidence item to survive semantic verification would make a thin but
supported result fail only after the full model spend. Evidence production and
verification-shortfall behavior must therefore change together with the claim
budget. Page scopes must also be bounded by the analysis output-token allowance:
a page-derived floor that cannot fit as complete JSON would turn coverage into a
new deterministic failure. Finally, evidence coverage and claim count are
different obligations. Requiring one claim per evidence item would prohibit the
consolidation that synthesis exists to perform.

Exact quotation bytes are also still model-authored on the primary analysis
path. Deterministic whitespace reconciliation and a second, catalog-backed
repair request mitigate invalid copies but do not remove that source of
failure. The repair catalog is bounded while traversing source blocks in order,
so later blocks can be absent. Finally, every run uses one fixed generation
seed, verification partitions only by claim count after applying an aggregate
character ceiling, and model requests expose neither measured duration nor
token usage.

### Required behavior

For this contract, `N` is the number of normalized pages containing native text
and `E` is the number of validated evidence items available to synthesis. The
document claim budget is:

`B = min(64, max(8, ceil(3 * N / 5)))`.

Analysis is partitioned into page-scoped requests whose page count is derived
from the configured analysis output allowance. Let `A` be
`ANALYSIS_OUTPUT_TOKENS`, reserve `R_A = 192` tokens for the response envelope,
and allocate `I_A = 92` tokens for each bounded quote-ID/claim item. The maximum
evidence quota for one response is
`Q_A = floor((A - R_A) / I_A)`; `A < R_A + I_A` is invalid configuration.
Without tokenizer admission, the item allowance reserves four tokens for a
scope-local quote ID of at most eight ASCII characters at two characters per
token, 64 tokens for at most 192 claim-text characters at three characters per
token, and 24 tokens for JSON field syntax and escaping. The resulting 92-token
item equals `I_A`. With the current 1,024-token allowance, `Q_A` is nine and
`S_max` below is 15.
The quote-ID response schema and parser must enforce those identifier and text
bounds rather than applying the general 2,000-character claim limit. A page
scope may contain no more than
`S_max = floor(5 * Q_A / 3)` native-text pages, and character limits may split
it further.

For each resulting scope containing `S` native-text pages, the validated
analysis result must contain at least
`F_scope = max(1, ceil(3 * S / 5))` and no more than `Q_A` distinct evidence
items drawn from at least `F_scope` distinct pages. Candidate construction must
make at least one eligible exact quote available for every native-text page in
the scope. A response below either floor is invalid and fails in analysis; it
must not silently reduce `E`. Because `F_scope <= Q_A` by construction and the
sum of the per-scope floors is at least `ceil(3 * N / 5)`, `E` continues to grow
with native-text page count without asking one response to exceed its output
budget.

The budget is monotonic in native-text page count. The direct and hierarchical
paths must both use `B`; crossing an item or character partition boundary must
not lower the document's final claim capacity. Claim count has its own lower
bound, independent of the evidence-production floor:

`K = min(B, E, max(3, ceil(B / 2)))`.

The `E` term makes the bound valid for small catalogs: `E = 1` requires one
claim and `E = 2` requires two, while larger catalogs retain room to consolidate
related evidence. Synthesis must request and validate at least `K` distinct,
non-duplicative claims and no more than `B`. Every supplied evidence ID must
appear at least once across the deterministically accepted synthesis claims,
and every claim must cite supplied evidence. IDs must be copied exactly;
related evidence should be consolidated into coherent claims rather than
repeated as separate ideas. Hierarchical batching may consolidate inside a
batch, but its intermediate quotas, lineage, and final composition must preserve
both the same document-level claim bound and complete evidence-ID coverage. It
must not funnel the whole document through a final request whose item count
silently becomes the claim ceiling.

Let `V` be the supported-claim count after semantic verification, and let
`E_V` be the validated evidence IDs cited by those supported claims. `V = 0`
remains a structured hard failure. If `0 < V < K` or `E_V` does not cover all
`E` validated evidence items, the run records a durable coverage-shortfall
warning and performs exactly one bounded re-synthesis from the original
validated evidence; unsupported claim text is not evidence. The retry uses the
same document budget, claim floor, complete evidence-coverage target, and
request-size limits and receives one new semantic-verification pass. If that
pass supports zero claims, the run fails. If it remains below `K` or leaves
evidence uncovered but supports at least one claim, the run completes with the
durable warning and only those supported claims. There is no third synthesis or
verification attempt.

The first synthesis, its verification verdicts, the retry synthesis, and the
accepted verdicts must remain auditable and linked to the same run without
overwriting an earlier artifact. The final verified artifact must validate
against the synthesis attempt it actually filters. The current one-synthesis
persisted shape cannot be treated as permission to erase or misidentify the
first attempt.

Primary analysis must use application-built quote candidates. Every candidate
is a bounded, contiguous exact substring of one authoritative normalized block
and carries a short scope-local selection ID plus fixed block and page
provenance. Selection IDs are canonical ordinals such as `q1` and `q2`, are at
most eight ASCII characters, and are mapped by Rust to full content-derived
candidate identities that are never model-authored. The response schema's
`quote_id` field must enumerate exactly the selection IDs supplied in that
request; decoder-compatible schema projection must preserve the enum. The model
returns one of those quote IDs and claim text; it never returns quote bytes or
chooses block/page identity. Rust still rejects empty, duplicate, foreign,
mixed-validity, or over-limit selections and materializes quotation bytes,
`SourceSpan`, and durable evidence identity from the selected candidate.
Exactness is therefore true by construction, and durable artifact identity does
not depend on the scope-local ID. The existing quote-copy repair model call and
its repair-only response schema are removed; historical repair warnings remain
readable.

Candidate construction and request partitioning must cover the full chunk in
source order. Each native-text page/block receives candidate opportunity before
any earlier page/block receives additional capacity. When the whole catalog
does not fit one bounded request, analysis partitions it into page-scoped
requests rather than truncating the tail. The prompt must ask for comprehensive,
non-redundant evidence from the beginning, middle, and final third of each
scope, including material conclusions, checklists, tables, exceptions, risks,
amounts, deadlines, and recommendations. Claims must preserve attribution,
negation, qualifications, and modal language such as `may`, `should`,
`generally`, `typically`, and `recommended` instead of strengthening them into
unconditional requirements.

Verification requests remain in canonical claim order and are partitioned by
both claim count and context-derived input size. Let `C` be the configured model
context in tokens, `O` the configured maximum output tokens, and `R_V = 512`
tokens reserved for the chat template and adapter framing. The complete
model-facing verification input, including system instructions and the user
payload, must fit within
`V_request = min(16_000, 3 * (C - O - R_V))` serialized Unicode characters;
`C <= O + R_V` is invalid configuration. The three-character proxy is required
because the current adapter has no exact tokenizer preflight and
identifier-heavy JSON tokenizes more densely than ordinary prose. With the
current 8,192-token context and 4,096-token output allowance, the ceiling is
10,752 characters, not 100,000. An adapter with measured tokenizer admission
may replace the proxy in a later, separately contracted change. Each request
also contains at most 16 claims.

The aggregate verification input for one semantic-verification pass must not
exceed `V_aggregate = V_request * ceil(B / 16)`, which is at most 64,000
characters by formula and 43,008 characters under the current model settings.
Both ceilings are enforced before the
first inference request, and one claim that cannot fit alone fails before
inference. A bounded re-synthesis receives a fresh aggregate allowance for its
single new verification pass. The verifier must treat matching words as
insufficient when a claim swaps table or matrix columns, assigns an action or
consequence to the wrong actor, reverses or drops negation, or strengthens
source modality. Every request and response retains the existing exact claim-ID
coverage, verdict-enum, source-exactness, provenance, and fail-closed
validation.

Each processing run derives its generation seed deterministically from its
`run_id` with a domain-separated SHA-256-to-`u64` mapping. Every `ModelRequest`
carries that seed explicitly. Replaying the same run is reproducible, while a
retry run for the same document receives a different seed and is a genuine
second generation attempt. Stage and request ordinals remain explicit request
metadata so diagnostics can distinguish otherwise similar calls.

Every logical model request records its stage, ordinal, locally measured elapsed
time, configured output-token limit, and provider-reported prompt, completion,
and total token counts. A schema-fallback transport retry is recorded as a
separate attempt. Missing provider usage is represented as unreported, never as
zero or an application estimate. Structured diagnostics must not contain source
text, prompts, quotations, model output, credentials, or private source paths.
The current 900-second request deadline is not raised by this work.

Behavior-version constants must advance for changed analysis, synthesis,
verification, summary, and citation semantics while historical artifacts remain
readable. The bounded re-synthesis must not overwrite the already-persisted
first synthesis, and the final verification must identify the synthesis attempt
it validates. A narrowly scoped persisted-artifact or schema change needed to
represent that immutable attempt lineage is in scope; unrelated storage changes
are not.

### Required change surface

- `pipeline/summary.rs`: document budget, page-complete quote-candidate
  partitioning, quote-ID analysis, budget-preserving synthesis, character-aware
  verification, verifier wording, and boundary validation.
- `pipeline/contracts.rs` and `pipeline/model.rs`: per-request seed/context,
  measured duration, provider token usage, and privacy-safe request-attempt
  diagnostics, including immutable synthesis-attempt identity.
- `pipeline/db.rs` and a narrowly scoped migration if required: preserve both
  bounded synthesis attempts and associate each verification with its input
  attempt without overwriting historical artifacts. This immutable-attempt
  storage work is sequenced last and must land as its own PR after the other
  accepted summary-quality work.
- `pipeline/service.rs`: supply the current run identity to every model stage
  without changing document identity or retry lineage.
- `tests/office_acceptance.rs`: quality floors and request metrics in the live
  acceptance report.
- `docs/CONTRACTS.md`: promote the accepted behavior into the current contract
  and remove this pending section in the implementation commit.
- Focused deterministic tests for both sides of every new budget, candidate,
  seed, verification-size, and metrics boundary.

### Acceptance evidence

- A live native-text run asserts that final supported claim count is at least
  `K` and no greater than `B`, and that the union of evidence IDs cited by those
  supported claims covers every validated evidence item.
- A live native-text run asserts that cited native-text pages divided by all
  native-text pages is at least 60 percent. Visual-only pages remain excluded
  from both numerator and denominator and remain uncited by the native-text
  path.
- A tail-heavy fixture proves that a material conclusion or checklist on the
  last page remains eligible and cited; a head-only catalog cannot pass.
- Direct and hierarchical fixtures with the same `N` and `E` prove the same
  `K` claim bound and complete evidence-ID coverage on both sides of the
  partition threshold.
- Sparse, multi-page fixtures prove every page-scoped analysis result meets its
  output-derived evidence quota plus its evidence-item and distinct-page floors;
  a 25-page scope is partitioned before its required output can exceed
  `ANALYSIS_OUTPUT_TOKENS`, and `E` cannot collapse to the old per-chunk maximum
  when `N` grows without proportional characters.
- Quote-ID fixtures prove the response schema enumerates exactly the supplied
  scope-local IDs, decoder projection preserves that enum, maximum-length IDs
  pass, over-length and foreign IDs fail application validation, and selected
  ordinals map back to the correct full candidate identity and exact source.
- Claim-bound fixtures cover `E = 1`, `E = 2`, `E < ceil(B / 2)`, and
  `E >= ceil(B / 2)`, and prove that multiple evidence IDs may consolidate into
  one accepted claim without losing evidence-ID coverage.
- A first-pass claim-count or evidence-coverage shortfall proves exactly one
  bounded re-synthesis occurs and the warning is durable. A second positive
  shortfall completes with only supported claims and the warning; zero supported
  claims still fails.
- Retrying one failed document proves equal document identity and different
  run-derived seeds; reconstructing requests for one run proves seed stability.
- Verification probes cover 15, 16, and 17 claims; one character below, at, and
  above the three-character-derived per-request and aggregate limits; and a
  mixed catalog requiring both count and character partitioning.
- Request diagnostics prove elapsed time is present for success and failure,
  provider token counts are captured when supplied, missing usage remains
  explicit, and no prompt, source, output, token, or path content is logged.
- A live Ollama analysis run over the repository fixture records request
  metrics and evidence counts and completes without any invalid quote-ID
  response before budget or page-scope implementation begins.
- Existing negative tests for exact quotation bytes, provenance, foreign and
  duplicate IDs, mixed-validity selections, invalid verdict coverage, and
  fail-closed artifact persistence continue to pass unchanged in intent.

### Explicit non-scope

Connect, entitlement, packaging, OCR/vision, parsing, model selection, and
customer-visible summary presentation remain unchanged. The delivered UI may
continue to render cited claim cards; changing it to prose is a separate product
decision. No timeout increase, dependency upgrade, broad refactor, generated
file churn, or opportunistic storage migration belongs in this implementation.

### Follow-on Ollama-native evaluation

Only after the preceding contract is implemented and accepted may a separate
adapter evaluation compare the OpenAI-compatible endpoint with Ollama's native
`POST /api/chat`. The native endpoint supports a JSON schema in `format`,
generation `options`, and response-side duration and prompt/output token counts;
Ollama's OpenAI compatibility does not expose a request field for context size.
The evaluation must test an explicit `options.num_ctx`, preserve the same Rust
validation and loopback/privacy boundary, and measure end-to-end output quality,
latency, request metrics, and failure behavior before recommending a cutover.

That evaluation must also record current GPU inventory, actual model residency
and CPU/GPU split, configured parallelism, context size, and observed memory at
idle and peak. The decision must account for KV-cache growth and the fact that
parallel requests multiply context-memory demand. A theoretical estimate alone
is not machine acceptance, and this contract does not select or implement the
native adapter.

## Stable checkpoint continuation

A nonterminal run at `INGESTED`, `PARSED`, `NORMALIZED`, `STRUCTURED`,
`CHUNKED`, `ANALYZED`, `SYNTHESIZED`, or `VERIFIED` may be continued explicitly
on the same `run_id`. The core derives the checkpoint from persisted state and
requires the caller's exact `state_version`; callers cannot select a different
checkpoint or request a backward transition. Failed runs remain terminal and
use the separate new-run retry contract.

Continuation executes only the ordinary forward stages that follow the durable
checkpoint. It does not copy or rewrite completed artifacts, create retry
lineage, or add state-machine edges. Each next stage loads and integrity-checks
its required persisted inputs before entering its active state, then continues
to use the existing atomic artifact/state/version/event transactions. A stale
version, unsupported state, missing runtime, or corrupt/missing checkpoint
artifact is rejected without pretending that downstream work completed.

Continuation through `SYNTHESIZED` requires `ModelRuntime` because analysis,
synthesis, or semantic claim verification remains. `VERIFIED` continuation is
deterministic and does not construct or require Ollama: final artifact assembly
uses the already-persisted verdicts, evidence, and claims. Summary-stage entry
points are independently callable at `CHUNKED`, `ANALYZED`, `SYNTHESIZED`, and
`VERIFIED`, so completed model work is not repeated.

The recent-work read model exposes the core-derived checkpoint, `canContinue`,
and whether a runtime is required. This is an availability projection from
authoritative run state; command admission rechecks the state/version and the
stage boundary validates the actual artifact. The frontend supplies only the
run ID and expected version. Startup still performs no automatic replay: a
stable run changes only after an explicit continuation request.

## Desktop background execution and cooperative cancellation

Standalone `summarize_document`, `retry_document`, and `continue_document`
commands admit work through the existing transactional service boundaries,
start one application-owned worker per run, and return a bounded
`BackgroundRunAccepted` projection instead of waiting for the final summary.
Each worker opens its own SQLite connection. Read/status commands likewise use
short-lived independent connections, so a model request does not hold an
application-wide database mutex or prevent status polling.

The in-process worker registry prevents a second desktop worker for the same
run and scopes cancellation to work owned by this desktop process.
`backgroundActive` reports that ephemeral ownership, while `canCancel` is true
only when both the durable state is cancellable and that run has a registered
desktop worker. These are Tauri read-model projections, not persisted pipeline
truth; Connect jobs are not implicitly controlled by the standalone UI. The
frontend polls `get_run_status` by exact run ID and expected-state/version
admission remains authoritative in Rust.

A cancellation request requires the current `state_version`. Its SQLite
transaction moves the run from an implemented active state or stable checkpoint
through `CANCELLING`, sets `cancellation_requested`, increments the version, and
appends `cancellation_requested`. Only after that commit does the manager signal
the in-memory token. The worker checks the token between pipeline stages, before
and after runtime health/generation operations, before each analysis chunk, and
before each verification batch. `CANCELLING -> CANCELLED` is a second atomic
state/version/event transaction after the worker acknowledges the request.

Cancellation is cooperative: the current parser, deterministic stage, SQLite
transaction, or Ollama HTTP request is not forcibly killed. If cancellation
wins the compare-and-set race, a concurrent stage-completion transaction cannot
persist its artifact or success event. If stage completion wins first, the
cancellation request applies to the newly durable checkpoint. Already-completed
checkpoint artifacts remain valid; unfinished model output is not persisted.
If cancellation commits between a failure finalizer's read and write, the same
immediate transaction acknowledges `CANCELLED` rather than stranding
`CANCELLING` or persisting a conflicting failure. A stale worker that lost CAS
ownership never fails the newer state.

A worker-start failure after new/retry admission is converted to structured,
recoverable `FAILED` when SQLite remains writable. A continuation worker that
cannot start leaves its stable checkpoint unchanged. Once a worker is running,
a panic, non-stale error, or unexpected nonterminal return is failed durably
when possible. A process exit while `CANCELLING` is finalized by startup
recovery as described above.

## Connect provider lifecycle and v1 contract

Document Summarizer advertises `document.summarize` version `1.0`, accepting
`application/pdf` and producing
`application/vnd.local-connect.document-summary+json`. The wire shapes follow
the executable contracts committed in the separate `connect-contracts`
repository. Protocol, application, capability, and summary versions are
separate fields.

When `XDG_RUNTIME_DIR` is available, the Tauri process binds an ephemeral exact
IPv4-loopback HTTP endpoint and atomically writes owner-only registrations under
`$XDG_RUNTIME_DIR/local-connect/v1/providers/` and
`$XDG_RUNTIME_DIR/local-connect/v2/providers/`. Protocol v1 uses a fresh
instance UUID for each process. Protocol v2 reuses the provider identity stored
in private application data so accepted jobs remain associated with the same
provider across restart. Both registrations use a fresh bearer token and the
same ephemeral endpoint for each process. Manifest, submission, and status
routes require that token; browser `Origin` requests are rejected. Missing
runtime-directory or provider startup failures are logged and do not prevent
standalone startup.

Provider startup scans only registration filenames that exactly claim the
Document Summarizer app ID and a UUIDv4 instance. It reads bounded regular files
and probes the declared exact-loopback manifest with the registered bearer
token. Any matching live manifest aborts replacement; otherwise those owned
entries are stale or malformed and are removed before new registrations are
published. Foreign and merely similar filenames are not touched. This keeps
cleanup local to Document Summarizer without treating a PID or file's presence
as proof that a capability is available.

On Tauri's final `RunEvent::Exit`, the provider unregisters both protocol files.
Publication, startup scavenging, and removal share an owner-only lifecycle-file
lock. While holding that lock, removal requires the on-disk protocol, instance
ID, endpoint, and bearer token to match the exiting process. This makes cleanup
idempotent and prevents an older process from unlinking a replacement between
its ownership check and deletion. Lock acquisition is bounded; a timeout logs
the cleanup failure and leaves the registration for normal next-start recovery.
Abrupt termination or power loss cannot run exit cleanup; any files left by that
boundary are reclaimed on the next provider startup and remain
non-authoritative to authenticated-manifest discovery in the meantime.

"Optional" in this provider lifecycle means that Connect failure or absence
cannot disable the standalone application. It is not a user-facing toggle.
Connect manifest discovery, new-job admission, and Connect job-status access
require an active signed entitlement containing
`connect.capability_exchange`. The provider authenticates the caller first and
then evaluates the entitlement on every request, so expiry or replacement takes
effect without application restart. Submission authentication and entitlement
checks run before multipart extraction. New-job admission rechecks inside the
acquired SQLite write transaction and again before commit, after the artifact
has been received and validated. A
denied provider retains ownership of its live registration; authenticated discovery treats the stable public
`CONNECT_ENTITLEMENT_REQUIRED` response as unavailable rather than as a stale
file, while another provider process cannot replace it.

The v1 entitlement envelope contains exact signed payload bytes and an Ed25519
signature. Issuer public keys are embedded from the build-time-only
`LOCAL_CONNECT_ENTITLEMENT_KEYRING_FILE`; a build with no keys remains a working
standalone application but fails Connect closed. Runtime environment variables
cannot replace issuer trust. On the verified Linux boundary, the entitlement is
read from `$XDG_CONFIG_HOME/local-connect/entitlement-v1.json` or the equivalent
`$HOME/.config` fallback when the XDG value is unset or empty, and must be an owner-only, owner-owned, regular,
non-symlink file beneath an owner-only directory. The interval is
`issued_at <= not_before <= now < expires_at`, with no grace period. Invalid signatures,
unknown keys, malformed claims, missing features, insecure files, and absent
authority all deny Connect without exposing private claims to callers.

The desktop entitlement-status boundary returns only one of `active`,
`authority_unavailable`, `missing`, `invalid`, `not_yet_valid`, `expired`, or
`feature_missing` plus an active boolean. License installation is a UI-neutral
Rust core operation; the Tauri file picker and commands are adapters. The core
accepts only a bounded, non-symlink regular source file that evaluates `active`
under the compiled authority, derives the destination internally, and never
modifies the source. Candidate, installed-entitlement, and lock opens use
non-following, non-blocking descriptors and verify the opened file identity and
type after open. Participating Unix applications coordinate on the
owner-private persistent `.entitlement-v1.lock` file beneath an exact mode-`700`
directory. Newly created private-directory entries are synced before use. Under
the non-blocking exclusive lock, the provider snapshots any prior entitlement,
revalidates time, writes and syncs a unique same-directory mode-`600` file,
atomically replaces the entitlement, syncs the directory, and re-evaluates the
installed file. Validation, lock, write, and other pre-promotion failures
preserve any existing entitlement byte-for-byte. A post-promotion sync or final
validation failure durably restores the prior bytes, or removes the promoted
candidate when no prior entitlement existed; rollback failure is reported as an
install failure rather than success. Successful replacement affects the next
provider request without an application restart or private-database mutation.

This offline bearer entitlement is not machine-bound and cannot be revoked
before expiry without local replacement. License acquisition and production
issuer-key custody are release operations outside the application repository;
local installation is the app operation described above, and private signing
keys are never packaged. Google OAuth remains owned by Email Watcher: Connect
receives only explicitly handed-off PDF bytes and bounded artifact metadata,
never mailbox credentials or tokens.

`POST /v1/jobs` requires the bounded job-request JSON as the first multipart
field and one `application/pdf` byte stream as the second. No caller path is
accepted. The provider checks protocol/capability versions, UUIDs, display-name
safety, declared size, configured size ceiling, and SHA-256 while streaming to
an owner-only staging file. It flushes, atomically promotes, and revalidates the
provider-owned source before ingestion.

Schema v10 adds private `connect_jobs` persistence. The provider-owned document
and its normal `RECEIVED -> INGESTING -> INGESTED` run are inserted in the same
SQLite transaction as the accepted job-to-run mapping. A partial unique index
permits only one accepted/processing Connect job. Same job ID plus the same
canonical request returns the existing job; different input returns a conflict.
Interrupted active jobs become durable retryable failures on provider restart.

Job processing calls the same UI-neutral Rust application service as standalone
use. Completed wire output contains plain text, bounded warnings, and the input
artifact ID/media type/size/hash, but no provider-private document/run ID,
database shape, filesystem path, email metadata, or credentials. Provider
output is admitted against the v1 byte/count/field limits before completion.

This is a same-OS-user possession boundary, not application authentication. A
hostile process running as the same user can read the registration token; a
future trusted broker or OS package identity would be required to change that
threat model. Launch-on-demand, multiple-provider selection, callbacks,
workflow automation, remote execution, and cross-machine discovery are not
implemented.
