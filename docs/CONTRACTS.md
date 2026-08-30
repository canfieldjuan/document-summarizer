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
requires each chunk response to contain structured evidence. Rust accepts an
evidence item only when its block ID belongs to that chunk and its quotation is
a bounded, contiguous, exact substring of the authoritative normalized block.
The application derives the `SourceSpan`, chunk association, and deterministic
evidence ID; the model cannot supply or override those fields.

Synthesis version `3.0.0` receives only the validated evidence catalog and
returns structured claims. A catalog that fits one bounded request keeps the
direct path. Larger catalogs are partitioned deterministically in source order,
with at most eight items and 16,000 Unicode characters per request. Each first
pass produces at most four intermediate claims. Candidate reductions use the
same request bounds and must reduce every non-final batch by at least half.
The complete plan is conservatively capped at 256 model requests.

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
`qwen3-30b-a3b:latest`; `DOC_SUM_MODEL_BASE_URL`, `DOC_SUM_MODEL_NAME`,
`DOC_SUM_MODEL_TIMEOUT_SECONDS`, and optional
`DOC_SUM_MODEL_API_TOKEN_FILE` remain deployment overrides. The adapter accepts
only plain HTTP on exact IPv4 or IPv6 loopback, uses bounded connect, health, and
response limits, disables proxies and redirects, and reads only this
application's optional bounded token file. It never reads another application's
credential store. Document and evidence text are marked as untrusted data in
both prompts. The adapter maps the schema request to Ollama's OpenAI-compatible
structured-output field; Rust parses and validates the returned JSON before it
can become a pipeline artifact. Model output cannot invoke pipeline actions.
The selected imported Qwen model may make Ollama report the exact server error
`failed to load model vocabulary required for format`. Only for that exact
HTTP-500 response, the adapter caches the incompatibility and retries without
server-side grammar enforcement. The prompt still carries the explicit JSON
shape and the same Rust schema, identity, quotation, and provenance checks
remain mandatory. Other HTTP failures do not activate the fallback.

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

Release binaries must be produced through the Tauri build command with the
`custom-protocol` feature so the Vite output is embedded. A release compilation
without that feature fails at compile time rather than producing an executable
that silently depends on the development server.

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

## Connect v1 provider

Document Summarizer advertises `document.summarize` version `1.0`, accepting
`application/pdf` and producing
`application/vnd.local-connect.document-summary+json`. The wire shapes follow
the executable contracts committed in the separate `connect-contracts`
repository. Protocol, application, capability, and summary versions are
separate fields.

When `XDG_RUNTIME_DIR` is available, the Tauri process binds an ephemeral exact
IPv4-loopback HTTP endpoint and atomically writes an owner-only registration at
`$XDG_RUNTIME_DIR/local-connect/v1/providers/`. Each process uses a fresh UUID
and bearer token. Manifest, submission, and status routes require that token;
browser `Origin` requests are rejected. Missing runtime-directory or provider
startup failures are logged and do not prevent standalone startup.

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
