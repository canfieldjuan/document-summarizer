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

## Continuous integration

Pull requests and pushes to `main` run the `Rust checks` GitHub Actions job
on Ubuntu. A clean checkout installs Tauri's Linux build prerequisites and
builds the frontend assets before compiling the desktop targets. From
`src-tauri`, the gate runs `cargo test --locked --all-targets --all-features`,
`cargo clippy --locked --all-targets --all-features -- -D warnings`, and
`cargo fmt --all -- --check`. A failed command fails the job; no path filter
or allowed-failure setting may skip or conceal a required gate.

The workflow uses read-only repository permissions, no application secrets,
and a bounded job timeout. Third-party actions are pinned to commit IDs.
Live Ollama and external-PDF tests remain explicitly ignored: CI proves the
deterministic contracts and migrations, not real-document summary quality.
Publishing installers, changing model behavior, and configuring repository
branch protection are outside this workflow's scope. The first pull request
must exercise the hosted job; local success alone is not hosted-CI evidence.

## Ingestion test isolation

Tests in `pipeline/ingest.rs` own unique directories under the operating
system temporary directory. Every source fixture, missing-file probe, database,
and SQLite sidecar for one test stays inside that directory. Parallel test
processes must not share fixture or database paths, and tests must not change
the process working directory. Scope cleanup removes the owned directory on
normal return or panic unwinding; a process abort may leave only temporary
files, never artifacts in the source checkout.

This changes test storage only. Production ingestion, source-byte checks,
transaction rollback, and database-reopen assertions retain their behavior.
Verification includes concurrent ingestion test processes and a probe proving
normal/unwind cleanup preserves a neighboring test directory.

## Local summary artifacts

### Proposed: substantive evidence, quote-only paraphrase, and synthesis repair

Status: contract only, not implemented. This proposal requires review before
code. Commit implementation separately; fold these rules into current behavior
and delete this subsection when the complete implementation lands.

Root cause and evidence:

- A required one-item page response has no way to record that its entire
  candidate catalog contains only scan artifacts, furniture, or a bare heading.
  In the NARA probe, pages 6 and 12 each have only one such candidate. Those
  claims were nevertheless marked supported: materiality is distinct from
  entailment, and verification cannot serve as the omission classifier.
- Analysis currently writes claim text while seeing all page candidates, then
  binds it to just one selected quotation. The NARA approval claim selected q1
  while the approval language was in q2. A single-claim probe changing only
  quotation choice changed ambiguous to supported. This demonstrates a binding
  defect, not a general guarantee that correct quotations yield support.
- DOL's failing synthesis batch requires exactly four claims from eight evidence
  items. It returned four claims referencing seven items, omitting the bare title
  "Labor Standards in Agriculture". Missing references fail synthesis immediately;
  the later verification-shortfall retry cannot repair this failure.

Required change surface: versioned analysis/omission contracts, selection and
paraphrase requests, analysis planning and reload validation, synthesis ranges,
coverage-error classification and bounded request repair, durable local audit
records, and focused/live acceptance. Do not import the abandoned rewrite's
architecture. Reuse only the narrow non-substantive-omission concept.

Analysis has two separate model operations for a retained page:

1. Selection sees the page's full application-built candidate catalog and returns
   exactly one schema-enumerated decision: a supplied quote ID or the explicit
   non-substantive omission decision. It cannot return claim text, invented IDs,
   quotation bytes, or block identity.
2. Paraphrase sees only the selected exact quotation and fixed task instructions;
   it cannot see other candidates, prior selection messages, other page text, or
   previous claims. It returns only claim_text, still bounded to 192 characters.
   Rust restores the selected quotation and provenance. This eliminates access
   to competing candidates during paraphrase; it does not make hallucination
   impossible. Semantic verification and all existing negative checks remain.

An omitted page makes only the selection call. An accepted omission records the
typed reason NonSubstantivePageFurniture, page/chunk identity, and binding to the
examined candidate catalog. No arbitrary free-form omission reason is accepted.
The entire catalog must contain no substantive usable native-text fact: allowed
cases are scan artifacts, isolated footers/date stamps, and bare headings without
an assertion. Shortness, difficult content, redundancy, uncertainty, or failed
verification alone are not omission reasons. A short obligation, exception,
deadline, table value, or substantive heading must not be omitted as furniture.
Never treat illegibility as proof that the original document contains no facts.
Rust permits a page-wide omission only when the examined catalog covers all of
that page's native-text content. A truncated or input-rejected catalog cannot
justify an omission; missing candidate text must not become missing evidence.

Omissions are not EvidenceItems and never become summary claims or citations.
Only these recorded omissions are outside the all-evidence-cited requirement;
synthesis cannot silently discard or reclassify retained evidence. There is at
most one outcome (evidence or omission) per inspected native-text page. Rust
rejects mixed, duplicate, foreign-page, or incomplete outcomes. The versioned
analysis artifact persists omissions and a durable warning; reopening recomputes
identity and catalog bindings. If a run later fails, accepted omission decisions
must remain auditable, rather than exist only in an in-memory response log.

Coverage accounting is not relaxed. N remains all native-text pages under the
current definition. B = min(64, max(8, ceil(3*N/5))) is unchanged, and E counts
retained evidence only. Keep K = min(B, E, max(3, ceil(B/2))) for positive E.
Start with the existing evenly spaced page plan, then inspect previously
unvisited pages in deterministic source order when omissions leave the target
unmet. Stop when E reaches min(N, max(B, ceil(3*N/5))) or all native-text pages
have outcomes. Thus at most N selections and N paraphrases run; omitted pages
are neither cited nor counted as evidence. Reload must validate the exact
initial/backfill plan and stopping condition, not accept an arbitrary subset.
If usable pages are exhausted first, persist the omissions and a coverage
shortfall warning; do not claim the target was met. If E is zero, retain the
omission audit and fail with a structured no-substantive-evidence result, never
invent a summary. Live acceptance still checks raw native-text page coverage of
at least 60 percent and separately prints omission and inspected-page counts.

Synthesis evidence batches receive a range, not a forced exact allocation:
retain the distributed document-floor allocation as each batch minimum, but
allow up to min(batch evidence count, B) claims. The direct path retains K..B.
The final artifact must still satisfy K..B and cite every retained evidence ID.
Candidate reductions remain progress-making and preserve all original lineage.
Preflight plans against the sum of batch maxima, not their minima, including
worst-case reductions and repair calls under the existing 256-request ceiling.
All input, output, per-claim evidence, and verification-admission limits remain.

Each synthesis request (direct, evidence batch, or candidate reduction) may make
one additional generation only if an otherwise valid response omits required
references. The retry regenerates that request's complete response from the same
catalog and bounds, with explicit missing evidence/candidate identifiers and a
distinct attempt seed. It does not restart completed batches. Missing-reference
diagnostics are typed separately from malformed JSON, foreign/duplicate IDs,
unsupported claims, transport errors, and other failures; those do not enter this
repair path. A second omission fails closed. Never append missing IDs to an
unrelated claim, silently deduplicate, drop evidence, or lower K to pass.
Each repair counts against the same request budget. Preflight reserves at most
two generations per logical synthesis request and includes bounded feedback in
its system-plus-user input calculation before any inference.

This request repair is distinct from the existing one-time re-synthesis after a
verification coverage shortfall. Keep the latter's existing attempt ceiling;
it is not an extra fallback for synthesis errors. This revision does not claim
that changing a seed repairs semantic defects in the retained evidence.

Persist bounded local repair-attempt metadata for both success and failure:
run/synthesis/request/repair ordinals, catalog and response fingerprints, missing
IDs, validation outcome, and existing duration/token diagnostics. Never store an
invalid partial response as a validated SynthesizedDocument or overwrite an
earlier attempt. Selection/omission audit and repair-attempt storage must preserve
expected-state/version checks and atomic append behavior. DB/schema integration,
if needed, lands last as its own PR; the feature is not complete without durable
audit. Historical artifacts retain their old validators and identity/hash rules;
new omission semantics require explicitly versioned artifacts, not retroactive
acceptance of missing evidence in old records.

Explicit non-scope: no additional output tokens, larger context, longer timeout,
model/endpoint change, parser/OCR/vision changes, unbounded retries, relaxed
exactness/provenance/verdict rules, Connect, packaging, dependencies, or output
rendering changes. Existing allowances remain 2,048 analysis, 4,096 synthesis,
4,096 verification, context assumption 8,192, and timeout 900 seconds.

Verification required before claiming completion:

- Selection/paraphrase separation: a decoy candidate contains a fact absent from
  the selected quote, and captured paraphrase input demonstrably excludes it.
  Invalid selection, mixed outcome, extra field, and 192/193-character tests.
- Recorded omission fixtures: noise, bare title and footer/stamp; negative
  controls for short substantive obligations, exceptions, dates and table values.
  Focused live selection probes must exercise those positive and negative cases;
  scripted model responses alone cannot establish correct omission decisions.
  Reopen/tamper, all-omitted, mixed retained/omitted, backfill, tail and stopping
  boundary tests, plus an incomplete catalog with omitted substantive tail text.
  No silent denominator reduction or omitted-page citation.
- Synthesis slack and direct/hierarchical document-bound equivalence; one missing
  reference repaired, a second omission rejected, foreign/mixed input rejected
  without repair, complete coverage requiring no repair. Exact/max-plus-one input
  and total-call budgets include feedback and worst-case candidate reductions.
- Immutable audit on success, failed repair, cancellation and restart, with
  historical artifact/migration regression checks; existing negative tests pass.
- Local tests, strict clippy and formatting. Then live Qwen NARA and DOL runs
  report claims, retained evidence, inspected/omitted pages, raw cited-page
  fraction, request/repair counts, duration and completion tokens, with failures
  first. Neither mocked omission choices nor green unit tests prove live quality.

`ModelRuntime` is the only inference boundary. A request may select plain text
or a named, bounded JSON Schema output contract. Current analysis version
`4.0.0` uses application-built quote candidates rather than model-authored
quotation bytes. Each candidate is a bounded contiguous exact substring of one
authoritative normalized block and carries a scope-local ordinal such as `q1`,
plus fixed block/page provenance and a durable content-derived identity that the
model never sees. The response schema enumerates exactly the supplied ordinals.
The model returns one ordinal and bounded claim text per evidence item; Rust
rejects empty, duplicate, foreign, mixed-validity, or over-limit selections and
materializes the exact quotation, block identity, `SourceSpan`, and durable
evidence ID. Exact quotation and provenance therefore hold by construction, and
historical repair warnings remain readable without a current quote-copy repair
request.

Analysis selects exactly one evidence item per selected native-text page. Each
request contains only that page's candidate quotations, with an enum restricted
to their scope-local IDs and an evidence array bounded by `minItems: 1` and
`maxItems: 1`. Rust rejects empty, multiple, foreign, mixed-validity, malformed,
or overlong responses without deduplication or repair. The model chooses a
material passage and writes claim text; Rust restores exact bytes and provenance.
Candidate construction covers the page's blocks and tail before inference.
Scope-local IDs are at most eight ASCII characters, claim text is at most 192
characters, and the output allowance is 2,048 tokens. Analysis counts system
plus serialized user text against the context-derived input limit below.
Multi-page output quotas and model-enforced page floors no longer govern
current analysis.

For `N` native-text pages and the claim budget `B` below, Rust selects
`P = min(N, max(B, ceil(3*N/5)))` distinct pages. The plan evenly spaces
source-order indices, including first and last when `P > 1`, rather than
dropping the tail with a head-only prefix. Pages without native text are
excluded. Each planned page yields one validated item, then analysis stops;
small documents exhaust all pages when `N < B`. This meets the attainable
budget and 60-percent evidence-page target with eight requests for NARA and
67 for the DOL deck. Final accepted-claim page coverage is measured separately.
Ordered chunk metadata remains intact; chunks with no planned page have empty
evidence. Reload validation requires exactly one item per planned page and none
elsewhere. No database migration is required. Historical analysis `3.0.0`
retains its multi-page scope quotas, including the nine-item quota derived from
the historical 1,024-token allowance, and `2.0.0` its original validators;
neither version's evidence identities are rewritten.

Regression tests cover page-only enums, single-item cardinality, empty/two-item
and mixed/foreign responses, 192/193-character bounds, sparse/dense plans, tail
inclusion, small-page exhaustion, visual-only exclusion, chunk-independent
stopping, missing/extra evidence on reload, and historical artifact compatibility.
Deterministic positional quote selection remains a contingent fallback, not an
automatic response to synthesis or verification failure.

For synthesis version `4.0.0`, `N` is the native-text page count and `E`
is the validated evidence count. The document claim budget and floor are:

`B = min(64, max(8, ceil(3 * N / 5)))`

`K = min(B, E, max(3, ceil(B / 2)))`.

Both direct and hierarchical synthesis use those document-level bounds.
Every accepted synthesis contains at least `K` and at most `B` distinct
claims, every claim cites supplied evidence, and the union of cited evidence IDs
covers all `E`. Related evidence may consolidate into one coherent claim.
Requests are partitioned deterministically in source order with at most eight
items and the context-derived input limit below. Both direct and hierarchical
requests allow at most 4,096 output tokens. Hierarchical candidate reductions
preserve original evidence lineage and document-level capacity through bounded
pairwise composition; the whole document is never funneled through a final
request whose batch size becomes the claim ceiling. The complete synthesis plan
is capped at 256 model requests, and no claim may expand beyond 16 original
evidence items. Rust derives final claim IDs, restores canonical evidence order,
renders authoritative page labels, and persists every source chunk ID exactly
once in source order.

Generation allowances are ceilings, not required answer lengths. With context
`C = 8,192`, stage output allowance `O`, and framing reserve `512`, analysis and
synthesis admit at most `min(16_000, 3 * (C - O - 512))` Unicode characters of
system plus serialized user text: 16,000 for analysis and 10,752 for synthesis.
The three-character proxy is not exact tokenization. Synthesis partitioning and
execution both reserve the larger of the direct and hierarchical system prompts,
so the same bound controls planning and inference. Boundary tests cover exact
limits, one character beyond, arithmetic exhaustion, request output allowances,
and preservation of historical artifact quotas. Increased output room does not
relax any validator, evidence or claim floor, or timeout. Runtime enforcement of
response schemas remains separately necessary; Primary transport alone is not
proof of constrained decoding.

Synthesis also preserves downstream verifiability. Before inference, Rust
proves that the evidence catalog can be assigned to no more than `B`
single-claim verification inputs under `V_request` while reserving the maximum
claim-text size. Candidate pairs are compatible only when their combined
evidence fits that conservative single-claim bound. After synthesis, Rust runs
the actual claim catalog through the complete count, per-request character, and
aggregate verification planner before persistence. Current-version synthesis
artifacts must pass the same planner when loaded, so a count-legal but
context-oversized claim set fails in synthesis rather than surprising the later
verification stage.

Verification version `4.0.0` classifies every synthesized claim against only
its validated exact quotations. Requests stay in canonical claim order and
contain at most 16 claims. With context `C = 8,192`, output allowance
`O = 4,096`, framing reserve `R_V = 512`, and the identifier-aware
three-character token proxy, the complete system-plus-user input limit is
`V_request = min(16,000, 3 * (C - O - R_V)) = 10,752` Unicode characters.
The aggregate limit for one verification pass is
`V_aggregate = V_request * ceil(B / 16)`, capped at 64,000 characters and
currently at most 43,008. Count, per-request character, single-claim, and
aggregate bounds are all validated before runtime health or inference. The
verifier must reject material relationship errors such as swapped table
columns, the wrong actor, reversed or dropped negation, or strengthened
modality; matching words alone are insufficient.

Rust requires one unique `supported`, `unsupported`, or `ambiguous`
verdict for every claim and restores provenance from persisted artifacts. Let
`V` be the supported-claim count and `E_V` the evidence cited by supported
claims. `V = 0` is a structured hard failure. If `0 < V < K` or `E_V`
does not cover all `E`, the first verdict artifact records
`SUMMARY_COVERAGE_SHORTFALL` and the run performs exactly one bounded
re-synthesis from the original validated evidence with the same budgets and one
fresh verification pass. A retry supporting zero claims fails. A positive
second shortfall completes with only supported claims and the durable warning;
there is no third attempt. Unsupported and ambiguous claims remain auditable
and add `SEMANTIC_CLAIMS_WITHHELD`, but only supported claims reach displayed
text and citations.

Schema version 14 stores synthesis and verification attempts in append-only
tables keyed by run and attempt ordinal. Ordinal zero is written atomically with
the primary synthesis; retry synthesis and both verdict sets are appended
without overwriting it. The accepted `VerifiedDocument` names the synthesis
ordinal it filters, and final-summary and workspace validation load that exact
attempt. Update and delete triggers make attempt history immutable; migration
backfills existing primary summary artifacts as ordinal zero. Artifact JSON,
version, document identity, creation time, and SHA-256 row hash remain checked
on retrieval.

Current artifacts use analysis version `4.0.0`, synthesis, verification, and
summary version `4.0.0`, and citation version `3.0.0`. Previously persisted
semantic synthesis, verification, and summary version `3.0.0` artifacts and
citation version `2.0.0` retain their original validation rules; mechanical
version `2.0.0` summary artifacts and citation version `1.0.0` remain readable
separately. Continuation from a pre-upgrade `SYNTHESIZED`, `VERIFIED`, or
completed checkpoint does not apply current-version invariants retroactively.

The supported runtime adapter is Ollama through its loopback OpenAI-compatible
API, defaulting to `http://127.0.0.1:11434/v1/` and
`qwen3-30b-a3b:latest`. Each generation request retains the 900-second
deadline; connection and health checks have separate shorter limits. Deployment
overrides remain `DOC_SUM_MODEL_BASE_URL`, `DOC_SUM_MODEL_NAME`,
`DOC_SUM_MODEL_TIMEOUT_SECONDS`, and optional
`DOC_SUM_MODEL_API_TOKEN_FILE`. The adapter accepts only plain HTTP on exact
IPv4 or IPv6 loopback, disables proxies and redirects, and reads only this
application's bounded token file. Prompts mark document and evidence text as
untrusted data and temperature is zero. The adapter requests
`reasoning_effort: "none"`; whether reasoning is actually disabled depends on
the runtime and model template, not that request field alone.

Each run derives a signed-range generation seed from a domain-separated SHA-256
mapping of its `run_id`; a distinct domain-separated attempt seed makes the
bounded re-synthesis a genuine second generation while preserving reproducible
requests for one run/attempt. Every logical request records stage, ordinal,
locally measured elapsed time, configured output limit, transport attempt, and
provider-reported prompt/completion/total token counts. Missing usage remains
unreported rather than estimated. Diagnostics contain no prompt, source,
quotation, model output, credential, or private path content.

Before transport, the adapter derives a non-mutating decoder projection of the
canonical schema. It omits decoder-unsupported `uniqueItems` and strips numeric
`maxLength` values only when they exceed 192 characters. Bounds from zero
through 192 survive at every nested schema location, including analysis's
192-character claim limit; the old 2,000- and 4,000-character grammar expansions
remain omitted. Object closure, required fields, enums, non-empty strings, and
array bounds survive. The analysis system prompt explicitly states that each
`claim_text` is at most 192 characters and that each `quote_id` may appear at
most once in the entire response. An enum constrains membership, not reuse;
`uniqueItems` would compare whole objects rather than quote IDs and is unsupported
by production Ollama's llama.cpp grammar converter as well as vLLM. Current
single-item page-local analysis enforces uniqueness and page membership
structurally instead. See the [llama.cpp decoder limitations](https://github.com/ggml-org/llama.cpp/blob/master/grammars/README.md#json-schemas--gbnf).

Projection tests cover zero, 191, 192, 193, 2,000, and 4,000, preserve the
canonical input schema, and exercise small and large bounds in nested schemas.
Prompt tests tie the stated numeric limit to the validator constant. Live
acceptance reruns the public NARA schedule and DOL deck through the complete
pipeline and reports claims, evidence, cited native-page fraction, request
count, and total completion tokens for each. Any failed run remains an open
product blocker and must lead the result report.

Rust remains authoritative
for all string sizes, uniqueness, identity, quotation, provenance, and response
limits: the validator, evidence floors, and distinct-page requirements are not
relaxed, and invalid selections are never silently deduplicated. This change
does not alter the endpoint, model, timeout, schema versions, or storage.
The exact Ollama vocabulary-loading failure may retry once through
JSON-object mode; other HTTP failures do not activate that fallback, and each
transport attempt is diagnosed separately.

Historical synthesis `3.0.0` and `2.0.0` artifacts retain their original
identity and coverage rules. Semantic verification `3.0.0` retains its original
single-attempt behavior, while mechanical verification `2.0.0` may produce
paired summary `2.0.0` and citation `1.0.0` with
`SEMANTIC_VERIFICATION_DEFERRED`. Final summary and citation artifacts carry
content-integrity hashes, citations bind the exact summary hash and rendered
text, and retrieval revalidates the accepted verdict against its named
synthesis attempt.

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

Current limits are conservative. Source chunks remain bounded at 100,000
Unicode characters. Current analysis is bounded by its selected-page plan,
one-item response, quote/claim lengths, and context-derived request input.
Synthesis has no unbounded aggregate prompt: every direct, evidence-batch, and
candidate request is capped at eight items and the context-derived
system-plus-user input limit, and a hierarchy is capped at 256 requests.
Verification is bounded by claim count, the
context-derived per-request input limit, and its formula-derived aggregate
limit before inference. A native-text-free document, an individually oversized
item, an unrepresentable evidence catalog, or a plan beyond those limits fails
with a structured domain error; no summary text is invented. Cancellation is
checked before and after every model request.

Live native-text acceptance requires the final supported claim count to be at
least `K` and no greater than `B`, complete validated-evidence coverage, and
citations to at least 60 percent of native-text pages. Visual-only pages are
excluded from that ratio. Deterministic boundary tests cover direct and
hierarchical parity, tail eligibility, sparse page scopes, quote-ID admission,
claim/evidence floors, synthesis-to-verification safe and oversized candidate
pairs, final-catalog admission, request-size edges, zero/positive verification
shortfalls, attempt immutability, exact quotation bytes, provenance, and
fail-closed persistence.

Connect, entitlement, packaging, OCR/vision, parsing, model selection, and
customer-visible presentation are outside the summary-quality behavior. The
delivered UI continues to render cited claim cards; changing it to prose is a
separate product decision. Independent source-fact verification, visual-page
citations, and PDF-viewer navigation also remain deferred. This work does not
raise the timeout, change dependencies, or broaden storage beyond immutable
summary-attempt lineage.

A later, separately contracted adapter evaluation may compare the current
OpenAI-compatible endpoint with Ollama's native `POST /api/chat`. That
evaluation must test explicit `options.num_ctx`, preserve the same Rust
validation and loopback/privacy boundary, and measure output quality, latency,
request metrics, and failure behavior. It must also record GPU inventory, model
residency, configured context, and KV-cache memory under the actual precision
and concurrency. No native-endpoint cutover is part of the current contract.

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
