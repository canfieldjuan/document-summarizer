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
- Backfill cannot detect a wrongly omitted substantive page. Mechanical noise
  and stamp cases must be removed by Rust before inference, rather than left to
  a general model omission decision governed by prose.

Required change surface: deterministic pre-catalog page eligibility,
versioned analysis/omission contracts, selection and paraphrase requests,
analysis planning and reload validation, synthesis ranges,
coverage-error classification and bounded request repair, durable local audit
records, and focused/live acceptance. Do not import the abandoned rewrite's
architecture. Reuse only the narrow non-substantive-omission concept.

Before constructing quote candidates, Rust inspects the complete native-text
content of each visited page with a pure, versioned eligibility filter. It does
not change parser output, normalized source bytes, or the native-text page count.
It returns either retain or a typed deterministic omission with the predicate
and source fingerprint that justified it. Unknown/ambiguous input is retained.
Two narrow mechanically recognized cases produce an empty catalog:

- Scan noise: non-empty text with no Unicode-aware multi-letter word and at least
  80 percent of non-whitespace characters in punctuation, symbol, or control
  classes (including replacement characters). Neither word absence nor character
  distribution alone permits omission. Recognizable substantive numeric content
  such as amounts, percentages, units, or numeric tables vetoes this rule.
  Attached and separated short units, including "5l" and "5 L", are protected.
  The exact captured NARA page 6 contains "5l" and is therefore an ambiguous-
  content retention fixture, not an omission-positive fixture. This does not
  assert that the original page contains a measurement; it preserves content
  when Rust cannot safely exclude that interpretation. No named fixture is
  exempt from the numeric veto. Use unit-free synthetic scan noise for positive
  omission controls; a noisy distribution never overrides substantive content.
- Isolated date/page stamp: at most 32 trimmed Unicode characters whose entire
  content matches a numeric slash-separated date followed by a short page/form
  marker, such as "03/10/03  A-4", with no other content. The date grammar is
  one/two-digit month and day plus two/four-digit year; the marker is one/two
  ASCII letters, a hyphen, and one to three digits, separated from the date by
  whitespace. This is an anchored furniture grammar, not a generic semantic
  "has no assertion" check. A standalone date, deadline label, amount, or any
  additional assertion is retained. Length alone never permits omission.

The filter operates on the whole page, not isolated words, cells, or fragments
inside a substantive page. A mixed page retains its candidate content. Empty
catalogs caused by either explicit filter result yield a recorded omission with
zero model calls. An empty catalog caused by truncation, input limits, missing
blocks, or construction failure is an error, not evidence of non-substantiveness.
Exact predicates, Unicode handling, numeric-content vetoes, and threshold
boundaries must be tested directly; do not tune thresholds to make a live
coverage assertion pass.

For a retained page, analysis has two separate model operations:

1. Selection sees the page's full application-built candidate catalog and returns
   exactly one schema-enumerated decision: a supplied quote ID or, only for a
   legible heading-shaped page admitted by Rust, an explicit bare-heading
   omission decision. The general non-substantive/noise/stamp omission options
   are not exposed to the model. It cannot return claim text, invented IDs,
   quotation bytes, or block identity.
2. Paraphrase sees only the selected exact quotation and fixed task instructions;
   it cannot see other candidates, prior selection messages, other page text, or
   previous claims. It returns only claim_text, still bounded to 192 characters.
   Rust restores the selected quotation and provenance. This eliminates access
   to competing candidates during paraphrase; it does not make hallucination
   impossible. Semantic verification and all existing negative checks remain.

#### Amendment: structural synthesis attribution

Status: contract `f4ce9d2` precedes implementation `369a871`. Implemented, but
both corpus acceptances fail: DOL returns orphan claims despite complete slots;
NARA completes with warnings below supported-coverage acceptance. See the latest
evaluation; do not describe structural attribution as product closure. Fold this
into current behavior with the complete feature, preserving these proof limits.
Supersedes only the
synthesis/reduction response and missing-reference transport below. Verification
keeps its enum-constrained request-local IDs. Durable schemas, identity formulas,
historical validation, and all-evidence-cited validation remain unchanged.

Root cause: the reduction loop already runs only above the document claim budget
and sends a Rust-selected compatible pair for one output claim. Returning c1
alone communicates no legitimate grouping decision and nevertheless fails the
run. Initial synthesis does choose groups, but lists of references permit a
supplied item to disappear. Membership enums do not enforce complete coverage.

Required change surface: summary wire structs, schemas, prompts, request parsing,
request-local transport helpers, fixture runtimes and boundary tests; this
contract and LOCAL_MODEL_EVALUATION.md. Keep durable attribution validation as a
separate application-owned boundary, never a permissive fallback wire format.

- Reduction returns exactly one text-only claim. RawCandidateClaim has no
  candidate_ids field. Rust attributes the output to both supplied candidates,
  expands their original evidence union, and runs the existing uniqueness,
  canonical-order, known-evidence and MAX_EVIDENCE_PER_CLAIM checks. Reject a
  request other than an exact pair/one-claim bound before inference. Reject any
  model-supplied reference field, including foreign/cross-batch IDs; validate
  candidate metadata too. Attribution does not make unsupported prose supported.
- Initial synthesis returns {claims:[{text:...}], assignments:[integer,...]}.
  Assignment position i denotes supplied evidence position i; its value denotes
  the zero-based claim index. Rust owns the ordered request mapping and derives
  citations, never taking source identities from the response. Every supplied
  item has exactly one slot; each accepted claim must have assigned evidence.
- Preserve the existing minimum/maximum claim range, not a new fixed floor equal
  to ceiling. For every feasible count k in minimum..=min(maximum,N), emit a
  complete object alternative under root anyOf: claims has minItems=maxItems=k,
  assignments has minItems=maxItems=N, and each assignment is an integer enum
  0..k-1. Do not mix properties and anyOf at the same schema node. At most eight
  alternatives are needed because a request supplies at most eight items. This
  ties index bounds to the actual claim count, not merely its upper allowance.
  All alternatives reject additional fields. No feasible count fails admission.
- Rust independently rejects malformed JSON, empty claims, short/long/mixed or
  out-of-range assignments, unassigned claims, overfull claims, unknown or
  duplicate request metadata, and model-supplied ID fields. Preserve canonical
  durable ordering, exactness, provenance and complete synthesized-set coverage.
  Grammar guarantees apply only to completed constrained output; fallback,
  truncation and untrusted mocks remain subject to all Rust checks.
- Reference-omission repair is no longer a normal reachable trigger for these
  responses: short/malformed assignment arrays fail validation, never get padded
  or silently repaired. Do not add retries or raise request reserves. Existing
  bounded re-synthesis after verification shortfall is unchanged.

Assumptions/proof boundary: positional attribution guarantees recorded citation
coverage, not that every source fact is represented in the generated text.
Preserve semantic verification unchanged and report this limitation. A claim
with assigned but unused evidence can still omit meaning; no automatic support
or completeness verdict is inferred from assignments. The live test must prove
the alternatives compile on the actual Qwen/Ollama primary transport before a
full corpus run. Failure is a blocker, not permission to silently loosen schema.

Verification plan: prove exact pair attribution with no response IDs; test
foreign/cross-batch fields and corrupt candidate metadata. For every feasible
claim-count alternative test N-1/N/N+1 slots, -1/0/k-1/k indices, empty and orphan
claims, wrong types, mixed valid/invalid slots and canonical durable restoration.
Exercise the real request path and schema projection, including distinct valid
claim counts. Preserve the existing negative durable-validator tests. Measure
actual serialized packing and reserve at 83 evidence items; run local all-target
tests, strict clippy and fmt, then both Qwen corpus acceptances. Report failures
first, claims, evidence, cited-page fraction, initial/reduction batches, requests,
completion tokens and wall time; do not compare a failed partial run as equal
work to a completed run.

Explicit non-scope: changed B/K, retention or page target, semantic support,
unit veto/eligibility, quote selection/paraphrase, context/output/timeouts or
resource ceilings, parser/OCR/vision, model/endpoint, DB or audit expansion,
historical artifact formats, Connect, packaging, CI and unrelated cleanup.

#### Amendment: request-local generation identifiers

Status: contract `b884235` precedes implementation `4b4c7ed` and terminal-failure
identity preservation `463069b`. Implemented for all new synthesis,
candidate-reduction and verification requests, not only the failing reduction.
Fold into current behavior when the complete feature lands. Durable artifact
schemas, identity derivations and historical reload rules remain unchanged:
this changes transport representations, not artifact meanings or validators.

Root cause: `summary.rs:2180`, `:2218` and `:2243` accept arbitrary identifier
strings in synthesis, reduction and verification output schemas. Analysis quote
selection already constrains membership with an enum. The live deck reduction
returned a foreign 58-character ID instead of a 74-character candidate ID.
The model must not transcribe durable hashes to maintain application identity.
Qualification: `model.rs` already removes uniqueItems before transport; the
keyword does not reach the production decoder. Remove it from the synthesis
and reduction schemas too, rather than advertise a nonexistent guarantee.

Required change surface: summary prompt/response representations, all three
schema builders, request-local mapping, synthesis repair feedback, actual
serialization/partition/preflight and verification batch materialization in
`summary.rs` and a scoped helper module if useful; fixtures and boundary tests;
`office_acceptance.rs` metrics if needed, plus this contract and evaluation.

- Every identifier copied by a model must be constrained to the exact supplied
  vocabulary, never an unconstrained string. Synthesis evidence uses e1, e2,
  etc.; reduction candidates c1, c2, etc.; verification claims k1, k2, etc.
  Verification evidence also uses e-prefixed local IDs, shared consistently
  within that request. Assignment is deterministic from the ordered request
  inputs. Reset mappings per request/batch; keep the same map through its one
  existing repair. No process-global map or cross-run mutable state.
- Rust keeps full durable identities privately and maps response ordinals back
  by exact lookup, not permissive numeric parsing. Reject foreign, malformed,
  wrong-kind, cross-batch and duplicate references, including valid/invalid
  mixtures. Never accept a durable ID as a fallback wire identifier. Validate
  the restored response through all existing provenance, count, coverage and
  identity checks before materialization or persistence.
- Update PromptEvidenceItem, PromptSynthesisCandidate, PromptVerificationClaim,
  PromptVerificationEvidence and RawClaim/RawCandidateClaim/RawClaimVerdict wire
  semantics accordingly. Candidate prompts contain evidence_count, not original
  evidence_ids. Rust still expands selected candidates to their real evidence
  union and enforces MAX_EVIDENCE_PER_CLAIM. A count is not a semantic support
  guarantee and must never weaken compatibility or coverage validation.
- All three output schemas enumerate exactly their local request vocabulary.
  Remove uniqueItems from synthesis/reduction reference arrays. Enums enforce
  membership under constrained decoding, not uniqueness or complete coverage.
  Rust checks distinct references per claim; all supplied references must still
  be covered across the synthesized set. Verification still requires exactly
  one verdict per supplied claim and rejects duplicates and missing verdicts.
  Runtime JSON fallback/untrusted mock outputs still meet the same Rust checks.
- Missing-reference repair feedback carries only local ordinals from the same
  mapping; never leak durable IDs or silently translate a foreign response.
  After repair is exhausted, restore missing-reference error IDs to their
  durable identities before returning the terminal pipeline failure.
  Preserve its existing trigger, one-repair bound and resource accounting.
- Measure and admit the actual local-ID wire representation in partitioning,
  singleton preflight, candidate compatibility and verification batching; the
  planner and dispatched request must agree. Request character/count limits,
  output/context/framing/timeout budgets and reference maxima do not increase.

Cost qualification: 83 full evidence IDs occupy 6,059 characters in aggregate,
not in one existing request. Existing requests are already partitioned and may
repeat IDs in repair. Local ordinals remove most identity overhead; candidate
evidence_count also removes long reference lists. Measure actual serialized
sizes, initial synthesis batches, reduction calls, total requests, completion
tokens and wall time before/after. Do not promise fewer batches merely from
aggregate arithmetic; count limits still bind and successful runs do more work
than the prior run that failed before verification.

Assumptions/blockers: enum membership is a decoder guarantee only when structured
transport is honored; Rust remains authoritative on every transport. Reusing e1
in another request is intentional scope locality, not durable identity reuse.
The prior deck failure is evidence for this defect, not proof that repairing
identity will solve semantic support or consolidation. No live outcome assumed.

Verification plan: inspect exact vocabularies for all three emitted schemas,
prove a foreign ordinal is outside the grammar vocabulary and rejected in Rust,
and prove valid shuffled ordinals restore exactly their corresponding durable
identities. Probe empty/single/multiple, wrong-kind, malformed, mixed, duplicate,
cross-batch and missing references/verdicts; missing-reference retry must retain
the original local map. Assert no durable identity occurs in any model-facing
payload/schema/feedback; candidate prompts carry counts only. Pin actual wire
serialization at size edges and 83-item maximum-text/escaping batch envelopes.
Preserve historical artifact reload and all existing negative tests. Run local
all-target tests, strict clippy and fmt, then both full Qwen corpus acceptances,
reporting failures first with packing, coverage and measured request cost.

Explicit non-scope: altered semantic verdicts, uniqueness/provenance/coverage
relaxation, ID guessing or silent dedupe, lower acceptance or retention, changed
B/K, extra retries, model/endpoint/context/output/timeout or resource increases,
eligibility/unit veto, parser/OCR/vision, DB migration or audit expansion, CI,
Connect, packaging and unrelated refactoring.

#### Amendment: retention reserve for verification withholding

Status: contracts `bf42810` and `5dfcd90` precede implementation `8ece146`.
New runs use analysis version 8.0.0; versions through 7.1.0 retain their original
page plans, stopping
targets, omission validation and identities. Fold/delete this amendment section
when the complete feature lands, not while the broader materiality/audit work
is unfinished. This amendment supersedes only the new-run retention target of
the page-analysis contract; it does not alter supported-page acceptance.

Live acceptance is not closed: the new deck run retains 83 items but fails on
a foreign hierarchical candidate ID before verification. NARA passes at 7/11
native pages. See `LOCAL_MODEL_EVALUATION.md` for failures, cost and proof limits.

Root cause and qualifications:

- Before this amendment, `summary.rs:2521-2524` selected
  min(N, max(B, ceil(3N/5))) pages, and `summary/pages.rs:482` stopped when that
  many items are retained. It equals the acceptance target for the deck, not
  for every document: the B floor already gives some short documents margin.
- For N=111, acceptance requires A=ceil(3N/5)=67. The pre-amendment run retained 67,
  then delivered supported claims citing 66, failing the unchanged target.
  Withholding is permitted and essential for correctness, not guaranteed on
  every run. The observations 7/8 and 66/67 do not establish survival rates,
  causal explanations for those rates, or a statistically justified margin.
- Re-synthesis can improve wording and recover support for an existing item;
  it cannot introduce a source page absent from the retained evidence catalog.
  Do not treat re-synthesis as the reserve or assert it can never improve support.

Retention rule and guarantee:

- N counts the same native-text pages as acceptance, before furniture omissions.
  A=ceil(3N/5), B=clamp(A,8,64), M=MAX_EVIDENCE_PER_CLAIM=16.
  Choose an explicit fault-tolerance policy of W=1 withheld claim, not an
  estimated average model loss rate. Requested headroom H=W*M=16 pages.
- Count-based evidence capacity C=B*M. When A>C, reject the mathematically
  unrepresentable coverage target with a structured capacity error before
  analysis inference. Otherwise R=min(N, A+H, C) is the retained-page target.
  Checked integer arithmetic is required; no overflow or default substitution.
  R never decreases the historical target for a count-feasible document.
- Extend the existing evenly spaced selected-page set, rather than replace it
  with a different sample: add deterministic unused pages distributed across
  the document until R pages are selected. Preserve the document tail and all
  originally selected pages. Then retain the existing deterministic unused-page
  backfill for omissions. Stop at R retained items or exhaust eligible pages;
  omission admission/predicates and one evidence item per page are unchanged.
- Let E be the actual retained distinct-page count. Complete pre-verification
  evidence coverage means withholding w claims can remove at most w*M uniquely
  cited pages; shared references can only reduce that loss. Thus E>=A+16
  guarantees at least A cited pages after at most one withheld claim, provided
  all other claims are supported. No duplicate/filler evidence is permitted.
- This is a page-coverage guarantee only, conditional on actual E and the stated
  withholding budget. It is not a claim of semantic faithfulness, a guarantee
  against arbitrary withholding, or a guarantee of the separate claim floor K.
  If synthesis returns exactly K claims, withholding one can still violate K;
  changing that floor or requesting claim-count slack is not included here.
- Source exhaustion, valid omissions, or C may prevent full headroom. Retain
  available substantive evidence without inventing items or relaxing omissions.
  Record N, A, desired headroom, R, actual E, max(0,E-A), whether E>=A+H,
  withheld-claim count and lost-page count in acceptance metrics. Existing
  shortfall warnings and zero-supported hard failure remain unchanged. A
  natural small-document cap is not falsely described as full fault tolerance.

Couplings and cost checked before committing to the margin:

| Case | N | A | B | C | R | Planned headroom R-A |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| One native page | 1 | 1 | 8 | 128 | 1 | 0 |
| NARA native pages | 11 | 7 | 8 | 128 | 11 | 4 |
| Sparse twenty-page document | 20 | 12 | 12 | 192 | 20 | 8 |
| Deck native pages | 111 | 67 | 64 | 1,024 | 83 | 16 |
| Full count reserve edge | 1,680 | 1,008 | 64 | 1,024 | 1,024 | 16 |
| Partial count reserve | 1,681 | 1,009 | 64 | 1,024 | 1,024 | 15 |
| No count reserve | 1,706 | 1,024 | 64 | 1,024 | 1,024 | 0 |
| Impossible count coverage | 1,707 | 1,025 | 64 | 1,024 | reject | none |

These are count feasibility cases, not claims that the largest documents pass
the separate request-size, synthesis-plan or materiality gates.
Known count limit: full sixteen-page reserve ends at N=1,680. N=1,681 through
1,706 can still meet sixty-percent coverage but have only partial or no reserve.
N=1,707 is the first impossible acceptance target: A=1,025 exceeds C=1,024.
Other unchanged resource limits may reject a document earlier.

For the deck, 83<=64*16. A constructive count witness is 19 pairs plus 45
singletons: 64 claims covering 83 distinct items. In the unescaped maximum-text
fixture, a 2,000-character synthesized claim with two 600-character quotations
and full identifiers totals 4,628 verifier input characters, within 10,752.
Ten such references total 10,300 and fit; eleven total 11,009 and fail. Therefore
B*M is only a necessary count ceiling, not proof that arbitrary text fits.
Preserve the existing measured verifiability partition, candidate compatibility,
all-evidence synthesis coverage, actual verification batch planner and all
single-item negative checks. Never force unrelated evidence into a misleading
claim or discard evidence to fit. An unrepresentable catalog remains an error.

This trades the zero-margin page-coverage cliff for greater consolidation load:
with an unchanged 64-claim ceiling, 67 items require at least three item-count
reductions, whereas 83 require nineteen. Complete reference coverage already
failed in the corpus when synthesis dropped "Labor Standards in Agriculture".
The count witness proves only that packing exists, not that the model can find
faithful consolidations while retaining every reference. These reductions need
not be separate calls: initial batches may consolidate multiple items. The live
corpus run, including coverage, request count and wall time, decides whether
this trade works; the arithmetic alone is not acceptance evidence.

Independent serialization arithmetic against the current prompt shapes gives
R=83, L=384, quote length 600, full IDs: twelve synthesis batches (eleven of
seven items, one of six) under S_user=8,117. Sum of batch maxima is 83, so the
existing conservative plan reserves at most 19 reduction requests and
2*(12+19)=62 calls including bounded request repair, versus 26 for E=67.
Even a one-item-per-batch partition would reserve 2*(83+19)=204, below 256,
provided every individual item fits. This does not prove that all future
candidate pairs will be semantically or structurally compatible.

Without omissions, ordinary analysis calls rise from 134 to 166; with one
paraphrase retry per retained page, 201 to 249. Omitted heading selections can
add calls while backfilling; inspect each native page at most once, preserving
the three-calls-per-inspected-page envelope. Verification still admits at most
64 batches per pass and two passes. No timeout, context, token or request
ceiling increases. The production Rust preflight tests on `8ece146` now reproduce
the 62-call maximum-text and 204-call escaping envelopes for 83 items. These are
reservations, not live call counts. The live deck used 167 analysis and 18
synthesis calls before failing; verification did not run.

Required change surface: versioned retention helpers in `summary.rs`, page plan,
stop/backfill and historical reload dispatch in `summary/pages.rs`; acceptance
metrics/fixtures in `office_acceptance.rs`, and evaluation documentation. No DB
migration or new artifact fields are required; derive metrics from versioned
plans and existing evidence/verdict/omission artifacts. Before live implementation
acceptance, exercise the 83-item envelopes through the production Rust planners.

Verification plan:

- Pin the table above, zero-native-text error, checked-arithmetic boundaries,
  unchanged B/K, and the old-target-subset/new-target property across sparse,
  dense, direct and hierarchical fixtures. Preserve historical 7.1.0 and older
  reload without requiring the new larger target or rewriting identities.
- Exercise omission/backfill and source exhaustion with partial/full reserve;
  assert no model calls for deterministic furniture and no repeated page work.
- Construct 83-page evidence with full synthesis coverage and short quotations
  that let the 16-reference claim fit the real request bound. Withhold one claim
  uniquely covering 16 pages: 67 remain and the page gate passes. With 82 pages,
  the same loss leaves 66 and fails. Probe zero loss, overlapping references,
  two disjoint 16-page losses, and small-document caps without claiming an
  unconditional pass. Preserve real support verdicts, K and source provenance.
- Run actual synthesis partition/count reservations and verification-safe
  coverage preflight on the 83-item maximum-text and escaping fixtures; a
  singleton or incompatible catalog must still fail before the affected model
  stage. Test 16 versus 17 evidence references per claim and exact size edges.
- Run local all-target tests, strict clippy, fmt and both full Qwen corpus
  acceptances. Record failures first with claims, evidence, native-page fraction,
  actual retained margin, withheld/lost pages, request counts, tokens, durations
  and complete reopen assertions. A margin calculation alone is not closure.

Explicit non-scope: lower page/evidence acceptance, changed K/B or semantic
verdicts, forced support, corpus-specific rules, broader omission discretion,
parser/OCR/vision, model choice/endpoint, paraphrase/decoder/output/context or
timeout increases, extra re-synthesis passes, CI, packaging, DB/attempt-audit
implementation. The remaining audit work stays separately sequenced.

#### Amendment: verification admission from actual batches

Status: contract `98965a5` precedes implementation `446fe88`. This supersedes the
aggregate character admission rules below, including the single-claim capacity checkpoint.

Root cause: `verification_aggregate_character_limit` estimates calls from
ceil(B/16), while the actual planner splits on serialized size as well as count.
The completed DOL analysis/synthesis attempt on `05d046f` consequently failed
before verification at 49,670 aggregate characters versus 43,008. The aggregate
check actually runs after partitioning, not before it; it prevents inference,
not planning. It is not needed for per-request context safety, but removing it
does expand the admitted total work. Preserve an explicit finite work bound.

Required change surface: `summary.rs` shared verification planner, its admission
helpers and boundary/integration tests; contract and live evaluation. Use the
same planner for synthesis admission, persisted synthesis reload and verification.

- Keep B, maximum 64 claims, 16 claims per request, and V_request=10,752 unchanged.
  Validate catalog cardinality, claim budget and prompt/claim correspondence in
  the shared planner, before batching, so no caller can silently zip a mismatched
  catalog or bypass the claim bound.
- Plan the actual nonempty batches in canonical order using full serialized
  system-plus-user size and count. Every materialized batch must fit both limits.
  A single oversized claim, including one after a valid prefix, fails the whole
  plan before runtime health or inference. Do not split a claim's references,
  truncate text, omit claims or change semantic verdict requirements.
- Replace the aggregate ceiling with explicit MAX_VERIFICATION_BATCHES=64 per
  pass. Check actual planned batch count. This admits the worst legal partition
  of one batch per claim without inventing another estimated calls-per-claim
  ratio. Empty plans and counts beyond the maximum fail before any calls.
- Delete the formula-derived aggregate constant/helpers and rejection, not
  recompute an aggregate from the resulting batch count. Total work remains
  bounded: at most 64 * 10,752 = 688,128 input characters and 64 configured
  output allowances per pass; the existing single re-synthesis permits at most
  two passes, hence 128 logical verification calls. These are worst-case caps,
  not promised live cost. Transport fallback policy and timeout are unchanged.
- No artifact format, identity/version or migration changes: the classification
  contract is unchanged, only resource admission is broadened. Existing valid
  persisted artifacts must still reopen, with source/claim integrity validated.

Explicit non-scope: paraphrase/decoder bounds, prompts, synthesis budget/funnel,
coverage or materiality changes, larger context/output/timeout, model/endpoint,
DB audit implementation, parser, packaging or CI. No new retries.

Verification: prove a size-only split, count-only split, mixed count-and-size
split, actual 64-batch catalog, and 65-claim catalog rejected before calls;
also probe the batch-count guard directly at 0/1/63/64/65. The over-limit catalog
also exceeds the unchanged claim bound and should fail that earlier admission.
Test oversized singleton and mixed valid/oversized prefix, mismatched lengths,
zero/overlarge claim budget, complete reference coverage and zero runtime calls
on refused plans. Replace old aggregate-negative tests with positive admission
and actual inference tests while retaining per-request negatives. Run local
all-target tests, strict clippy/fmt, and both full Qwen corpus acceptances;
record failures first and the real verification request/token costs.

#### Amendment: word-targeted paraphrase generation

Status: contract `0f108e5` precedes implementation `05d046f`. DOL analysis now
completes; the separate verification aggregate gate blocks live delivery (see
`LOCAL_MODEL_EVALUATION.md`). This narrowly changes the generation
target, not evidence admission. It supersedes character-targeted wording in the
single-claim checkpoint below. New analysis version 7.1.0 keeps version 7.0.0
admission and identity rules; historical artifacts remain readable unchanged.

Root cause: the captured DOL draft is 392 Unicode characters and approximately
61 whitespace-delimited words. The character-targeted retry repeated it. It
passed mechanical completion, but never reached factual verification: faithful
output is not established by that trace. Word-targeting is an empirical attempt
to improve compliance, not a guarantee that models cannot count characters or
that 55 words necessarily fit 384 characters.

Required change surface: `summary/pages.rs` initial/retry prompts and tests;
`summary.rs` prompt-version compatibility and integration tests; existing live
metrics and evaluation. Keep the schema ceiling 1,536, Rust maximum 384,
single-retry limit and complete selected quotation unchanged. No packing change.

- Ask for the shortest complete claim in at most 55 words, preserving supported
  actors, actions, modality, negation, exceptions and quantities. Do not expose
  a numeric character target in the generation instructions or retry feedback.
- Rust supplies an approximate draft word count using `split_whitespace().count()`
  (Unicode whitespace; hyphenated strings remain one segment). For an overlong
  draft the retry receives that count and a 55-word target along with the full
  untrusted draft and the same authoritative quotation. Ask to rewrite it to
  55 words or fewer, with the same supported meaning. If already under that
  target, request shorter phrasing rather than padding or repeating the draft.
- Words are a soft generation target only. A complete 56-word claim within 384
  characters is not rejected for word count; a one-word 385-character claim
  still fails length admission. Completeness, semantic support, exactness and
  provenance remain independent requirements. No Rust trimming/truncation.
- Keep measured character lengths in local diagnostics, not as a model counting
  obligation. Count words deterministically only for feedback; no tokenizer or
  language-specific dependency is introduced.

Explicit non-scope: no tolerance band yet, decoder/resource/context/token or
timeout increase, changed coverage/eligibility thresholds, source edits, parser,
DB migration, extra attempts, model/endpoint change or silent omission.

Verification plan: test the captured 61-word draft feedback, whitespace/Unicode
counting, already-under-word-target overlong drafts, initial/retry word wording,
absence of numeric character targets, unchanged 383/384/385 admission and
1,535/1,536/1,537 decoder/draft boundaries, historical reload and exact binding.
Run local all-target tests, strict clippy, formatting and both Qwen corpus
acceptances. Record failures first, all existing metrics and actual retry lengths.

Conditional next step if live character overshoot survives: specify a separate
contract amendment before any tolerance implementation. A 512-character hard
maximum with a durable warning above 384 must be used by actual packing and
versioned reload, not only the validator. Recompute production planner reservations
instead of trusting rounded call estimates; semantic support and complete text
must never become optional. This word-target amendment makes no such change.

#### Proposed amendment: single-claim capacity and draft-aware shortening

Status: approved in `573ad48`, clarified in `869e458`, implemented separately
in `897d2e9`; DOL live acceptance remains blocked, as recorded in
`LOCAL_MODEL_EVALUATION.md`. These rules supersede the 192/768 limits and
draft-free retry of the implemented
completion checkpoint below for new analysis version 7.0.0 only. Preserve
versions 6.0.0 and earlier with their original limits, completion rules and
identity/hash behavior. Fold/delete this proposed text with the complete feature,
not while the broader omission and request-audit work remains unfinished.

Root cause and assumptions:

- The production paraphrase operation returns one claim per 2,048-token call.
  The older 1,024-token, 192-token-envelope, 92-token-item quota remains only for
  historical multi-item analysis (`summary.rs`, `analysis_evidence_quota`).
  It does not justify the current single-claim length limit.
- Captured DOL responses 7 and 8 on `cb53dea` contain complete, punctuated
  agriculture definitions of 333 and 272 Unicode characters, using 76 and 63
  completion tokens. These are measured candidate paraphrases, not verified
  supported claims: the run failed analysis before semantic verification. Do
  not infer that either wording is faithful solely from its length or punctuation.
- Choose a concise capacity from these observations, not the entire token
  allowance: L = 64 * ceil(333 / 64) = 384 Unicode characters. This is 51
  characters of headroom over the longer observed draft, not permission for
  routine padding. The 64-character rounding quantum is an explicit sizing
  choice, not a model guarantee or the retired evidence-item quota. The prompt
  asks for the shortest complete supported claim preserving material qualifiers.
- With the existing three-characters-per-token planning proxy and a conservative
  32-token single-field JSON allowance, L needs about ceil(384/3)+32 = 160
  output tokens, below 2,048. This is a feasibility check, not a tokenizer-based
  worst-case guarantee. The measured 333-character draft used 76 tokens. The
  output-token limit remains a ceiling, not a target to fill; do not derive a
  multi-thousand-character claim allowance from it. Evidence remains one item
  per retained page, and no source quotation is shortened to accommodate L.

Required change surface:

- `summary/pages.rs`: prompt/schema, bounded length feedback, and full rejected
  draft input on the one existing paraphrase retry. `summary.rs`: versioned claim
  admission/reload limits and synthesis/verification packing regression tests.
- `model.rs`: decoder projection preserves maxLength through D = 4*L = 1,536;
  strips larger values. Pin both the 4x relationship and projection boundary.
  This widens resource headroom, not Rust acceptance. Compilation at 1,536 is
  unproven by the previous successful 768 probe: require a real Primary-transport
  schema probe before claiming compatibility; never silently remove the bound,
  change the 4x relationship, or count JSON fallback as proof of grammar support.
- `office_acceptance.rs` and the evaluation report: capture paraphrase repair
  counts and first/retry lengths alongside existing duration/tokens and coverage.
  Keep diagnostics free of source/draft text; raw response traces remain local,
  explicitly opted in, and are not a substitute for the deferred durable audit.

Synthesis packing is recomputed, not expanded:

S_total = min(16,000, 3*(8,192 - 4,096 - 512)) = 10,752 characters.
The current larger synthesis system prompt is 1,099 characters, and bounded
missing-reference feedback reserves 1,536. Therefore S_user = 8,117 characters
before repair. Keep eight as an item-count ceiling, not a promise that eight
fit. The request planner measures actual serialized JSON, including escaped
strings, field names, IDs and envelope, and splits when either count or size
would overflow. Count the actual system and full feedback on the repaired call.

Concrete compact-JSON fixtures (`minimum_claims:1`, `maximum_claims:64`, each
evidence ID 73 ASCII characters, exact quote 600 unescaped characters):

| Analysis text per item | Items | Serialized user characters | Fits S_user |
| --- | ---: | ---: | --- |
| Historical 192 | 8 | 7,389 | yes |
| Proposed 384 | 7 | 7,816 | yes |
| Proposed 384 | 8 | 8,925 | no: partition |

These are sizing fixtures, not global worst-case promises. Six-character JSON
escapes change the 384/600 case to 6,082 characters for one item and 12,111 for
two: only one fits. Tests use actual serialization and valid distinct IDs.
Candidate reductions still allow 2,000-character synthesized text and up to 16
original evidence references per claim; they use the same measured input gate,
not the analysis length as a proxy. Recompute plan/call reservations for the
resulting partitions, preserve B/K and all original lineage, and never restore
a shrinking claim funnel or drop evidence to fit. The existing 256-request
synthesis ceiling, feedback reserve and output allowance do not increase.

Pre-implementation cost clarification: 10,752 is already the effective total
input bound, not a new reduction from 16,000. Both ordinary synthesis and its
repair enforce the context-derived bound today. At B=64, a 59-item maximum-size
fixture changes from eight batches at L=192 to nine at L=384. Maximum candidate
count stays 59, so no reduction request is required; preflight reserves 16 versus
18 calls including one repair per request. Also exercise E=67 (the 111-native-
page plan's target): nine versus ten batches, maximum three reduction requests,
24 versus 26 reserved calls. These are size-envelope/planning comparisons, not
measurements of the earlier transient evidence texts or promised live call
counts. Before live acceptance, run these fixtures through the production
partitioner and sum-of-batch-maxima preflight under the unchanged 256 ceiling.

Verification sizing follows its real input fields:

V_request = min(16,000, 3*(8,192 - 4,096 - 512)) = 10,752 characters, counting
the 1,089-character verifier system prompt and complete serialized user JSON.
`PromptVerificationEvidence` carries evidence_id and exact_quote, not analysis
claim_text. Verification receives synthesized claim.text (still at most 2,000),
so raising L does not directly raise a verification field limit. Preserve the
16-claim ceiling plus greedy actual-character packing; neither is a fixed batch
size. With one 600-character quote, a 73-character evidence ID and 70-character
claim ID per claim, three 2,000-character claims total 9,555 model-facing
characters and fit; four total 12,373 and must split. Eight 384-character
synthesized claims total 10,717 and fit only this one-quote, unescaped fixture.
Multiple references, longer text, or escaping require earlier splitting. One
2,000-character claim with sixteen such quotes totals 14,554 and must fail
admission, never lose a reference. Keep synthesis's verifiability preflight.

The original count-derived aggregate rule of this checkpoint is superseded by
the actual-batch admission amendment above. Actual serialized requests must each
fit V_request and the resulting nonempty plan must contain at most 64 batches.
Larger text may change partition choices but cannot relax any individual request
limit, the document claim budget, or evidence-reference coverage.

Draft-aware shortening:

- The initial paraphrase sees only the selected quotation and fixed task text.
  On a length violation, the single retry additionally receives the complete
  rejected claim_text as a separate `rejected_draft` JSON field, plus measured
  character count, target L and typed violation feedback. Ask explicitly to
  shorten/rewrite that draft to L without losing supported qualifiers, while
  correcting anything not supported by the quotation. The draft is untrusted
  model output, never source evidence or instructions; the selected quotation
  remains the sole factual authority. Do not introduce other candidate quotes.
- For completeness/canonicality-only failures, keep typed feedback and the
  selected quote; no draft is required. A response failing length and other
  predicates takes the length-rewrite path and must fix all reported failures.
  Selection is never rerun. The distinct attempt seed, one-retry ceiling,
  cancellation checks, malformed/foreign/transport failure behavior and all
  output validators remain. Success uses only the newly validated response.
- Bound the retry draft at D = 1,536 Unicode characters. Admit the full serialized
  retry, not a truncated draft: system + selected quote + draft + typed feedback
  must fit A_request = min(16,000, 3*(8,192 - 2,048 - 512)) = 16,000. An otherwise
  well-shaped response above D, possible with non-enforcing transport, fails
  closed with a structured repair-input-too-large failure and no additional
  model call. The same applies if escaping makes the complete retry too large.
  Never truncate either source or draft, omit the required draft silently, or
  regenerate without it as an extra fallback. Test exact/max-plus-one draft and
  actual-request boundaries, including escaped/untrusted draft content.

Explicit non-scope: no model or endpoint change, output-token/context/timeout
increase, changed source quotes, Rust shortening/trimming, weakened completeness
or semantic verdicts, numeric-unit veto or furniture thresholds, corpus-specific
exceptions, lower native-page or supported-evidence targets, B/K changes,
dependencies, parser/OCR/vision, rendering, Connect, packaging, or DB migration.
Partial-analysis and per-request immutable lineage remains the separately
sequenced DB work; this amendment does not claim that work is complete.

Verification plan and acceptance:

- Pin L=384, D=1,536 and 4x headroom; projection tests at 1,535/1,536/1,537 plus
  historical 768, 2,000 and 4,000. Length tests at 383/384/385, Unicode character
  rather than byte counts, and complete versus mid-word boundary endings. Keep
  the exact captured 192-character cutoff regression and all completeness tests.
- Recompute only version 7 admission/reload rules; prove historical version 6
  rejects 193+ characters and retains its old evidence identities, while valid
  new-version evidence through 384 survives reopen with exact source provenance.
- Capture retry requests to prove exact draft round-trip, same selected quote,
  typed feedback, distinct seed, one additional call, and no injection of draft
  into authoritative quote/provenance fields. Test successful shortening, still
  overlong/incomplete retry, mixed violations, malformed/foreign input, oversized
  draft/request refusal, cancellation and no persisted invalid evidence.
- Test the exact serialized synthesis and verification fixtures above through
  production planners, with max-length mixed items, escaping, repeated quote
  references, count/character boundaries, direct/hierarchical B/K equivalence,
  full synthesis coverage, actual batch-count admission and existing negative tests.
- Run local all-target tests, strict all-target/all-feature clippy and fmt.
  Then run both full Qwen corpus acceptances and report failures first: claims,
  retained/cited evidence, raw cited-native-page fraction, request/repair counts,
  response lengths, duration and completion tokens. NARA must retain its durable
  warning semantics; neither native-page nor supported-evidence 60 percent
  threshold moves. A 1,536 grammar probe, successful rewrite, or passing count
  gate alone is not a claim of full corpus success or independently proven
  fidelity. DOL remains blocked until the complete live run passes.

#### Implemented checkpoint: paraphrase completion and verification-aware acceptance

Historical specification for analysis 6.0.0 (`90a8b91`, then `cb53dea`). The
proposed single-claim capacity amendment above supersedes its numerical length
limits and draft-free retry for new artifacts only; completion and coverage
invariants below remain unchanged.

Root cause before this checkpoint: the paraphrase decoder enforced the same
192-character ceiling as Rust. Constrained generation can close a string at that boundary
without finishing its clause; it does not perform semantic shortening. The
captured DOL response ends at 192 characters with trailing whitespace and fails
canonical validation, while an equally truncated unpunctuated word without that
space passes the old predicate. Separately, live acceptance incorrectly applies
synthesis's all-evidence rule to supported claims after verification, although
retained ambiguous evidence is allowed to be withheld with a durable warning.

Required change surface: model decoder projection, page paraphrase schema,
prompt and bounded retry, versioned analysis admission/reload validation, and
office acceptance's persisted synthesis/verification checks and metrics. No
eligibility filter, unit veto, parser, source quote, model, token allowance,
context, timeout, evidence target, or page-coverage threshold changes.

Paraphrase uses a decoder maxLength of 768 characters (four times Rust's 192),
and the adapter preserves numeric maxLength through 768, stripping larger
values as before. This ceiling is resource headroom, not the accepted length:
no valid claim can reach it. It cannot promise that an invalid model response
never reaches a finite ceiling. The prompt still demands at most 192 characters,
explicitly asking for a complete sentence instead of a cut-off clause. Rust
accepts only non-empty, already-trimmed text at most 192 Unicode characters.
Additionally, after optional closing quotes/brackets, it must end with terminal
punctuation (. ! ? or their full-width counterparts), following an
alphanumeric character with optional intervening closing quotes/brackets.
This admits terminal punctuation either inside or outside closing quotation
marks or parentheses. Bare words, dangling commas/colons/hyphens, ellipses,
and unpunctuated mid-word boundary cuts fail. This is a mechanical completeness
predicate, not a dictionary or proof of grammatical/semantic completeness;
semantic verification remains authoritative. Never trim, truncate, append a
period, or otherwise rewrite a model response to make it pass.

An otherwise well-shaped paraphrase failing length, canonicality, or this
completion predicate gets exactly one additional paraphrase call, with a
distinct deterministic attempt seed and bounded feedback naming the failed
checks. Both calls receive only the same selected quotation, never alternative
quotes, previous generated text, or other page content. Selection is not rerun.
A second invalid paraphrase fails closed; malformed JSON, extra fields,
selection/provenance errors and transport failures do not enter this retry.
The retry uses existing input/output limits, cancellation checkpoints and
per-request diagnostics. The analysis loop is bounded by N selections and 2*N
paraphrases; omitted pages consume no paraphrase calls. New analysis version
6.0.0 enforces completion at ingestion and reload. Version 5.0.0 and earlier
keep their historical validators and identity rules. Durable partial-analysis
and repair-attempt audit remains the separately sequenced DB work, not silently
claimed complete by this amendment.

Verification plan: projection tests at 0, 191, 192, 193, 767, 768, 769, 2,000
and 4,000; intact schemas/enums and non-mutating projection. Completion tests
on both sides of 192, including the captured whitespace suffix, a 192-character
mid-word cut without whitespace, valid complete sentences at the boundary,
Unicode, closing quotes, mixed-valid/invalid items, ellipses and dangling
punctuation. Prove rejection before persistence and on v6 reload, historical
v5 acceptance unchanged, one feedback retry repairs an invalid answer, repeated
failure stops after that retry, and malformed/foreign outputs do not retry.
Local full tests, strict clippy, fmt and both live Qwen corpus runs must report
failures first, Primary/fallback transport, request count, tokens and coverage.

A model-omitted bare heading makes only the selection call; a retained page
makes both calls. Every omission records NonSubstantivePageFurniture plus a
bounded origin (deterministic scan noise, deterministic date/page stamp, or
model bare heading), filter version, page/chunk identity, and complete source
binding. Model omissions additionally bind the examined candidate catalog.
No arbitrary free-form omission reason is accepted. Residual model judgment is
restricted to a legible, non-assertive bare heading; it is not a fallback for
text the mechanical filter was uncertain about. Shortness, difficult content,
redundancy, uncertainty, or failed verification alone are not omission reasons.
A short obligation, exception,
deadline, table value, or substantive heading must not be omitted as furniture.
Never treat illegibility as proof that the original document contains no facts.
Rust permits deterministic omission only after inspecting all original
native-text content on that page. A model omission requires a complete,
unfiltered candidate catalog covering that content. A truncated or
input-rejected catalog cannot justify an omission; missing candidate text must
not become missing evidence. Reload recomputes deterministic filter outcomes
from the bound source and rejects a forged reason, origin, or version.

Omissions are not EvidenceItems and never become summary claims or citations.
Only these recorded omissions are outside the pre-verification synthesis
all-evidence-cited requirement;
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
have outcomes. Thus at most N selections and 2*N paraphrases run (including the
single bounded paraphrase repair); omitted pages
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
The synthesized artifact, before verification, must satisfy K..B and cite every
retained evidence ID. Acceptance loads the persisted synthesized attempts and
checks complete evidence coverage there, including the selected attempt.
Supported claims may cite fewer retained items: report supported-evidence
coverage as distinct cited retained IDs divided by E, and gate live acceptance
at 60 percent, separately from the unchanged 60 percent raw native-page target.
The supported claim-count floor remains K. This acceptance threshold does not
replace the runtime's full-coverage target: any positive supported shortfall
still triggers the existing bounded re-synthesis and, if unresolved, completes
with a durable coverage warning. Zero supported claims still fails closed.
Tests must prove the warning survives database reopen, complete synthesized
coverage with partially supported evidence can pass acceptance, missing
synthesized evidence cannot, and either supported-evidence or native-page
coverage below 60 percent fails. Use integer comparisons and both boundary
sides; do not shrink E or N or special-case the corpus.
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

Approved dependency exception: promote the already locked unicode-properties
0.1.4 to a direct dependency, pinned to that version with general-category
support, solely for the filter's Unicode predicates. No dependency upgrades or
other new dependencies are authorized. This does not change the 80 percent or
32-character thresholds. Verify category handling and the unchanged lockfile
package versions alongside the direct filter boundary tests.

Verification required before claiming completion:

- Selection/paraphrase separation: a decoy candidate contains a fact absent from
  the selected quote, and captured paraphrase input demonstrably excludes it.
  Invalid selection, mixed outcome, extra field, and 192/193-character tests.
- Deterministic filter fixtures retain the exact captured NARA page 6, with a
  non-empty quote catalog and no model omission option. Unit-free synthetic scan
  noise and the exact captured NARA date-stamp page yield empty catalogs and zero
  model calls. The 80 percent ratio and 32-character stamp bound remain unchanged.
  Test the filter itself with short obligations, exceptions, standalone dates, labeled
  deadlines, amounts, percentages, units, numeric tables, Unicode words, and
  mixed noise/substantive content, including substantive text at the page tail.
  These controls must retain content without relying on a model to rescue it.
  Probe both sides of the character-ratio and length thresholds, single versus
  multi-letter words, empty input, full versus partial stamp matches, and an
  extra assertion added to otherwise matching furniture. Construction errors
  must never become omissions. Recorded outcomes survive reopen and tamper tests.
- Bare-heading model omission is tested separately: schema admission rejects
  noise/stamp/general omission reasons and does not expose omission on body-text
  pages. Focused live selection probes cover a true bare heading and heading-like
  substantive obligations, exceptions and conclusions. Scripted responses alone
  cannot establish correct model omission decisions.
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
actual batch-count verification planner before persistence. Current-version
synthesis artifacts must pass the same planner when loaded, so a count-legal but
context-oversized claim set fails in synthesis rather than surprising the later
verification stage.

Verification version `4.0.0` classifies every synthesized claim against only
its validated exact quotations. Requests stay in canonical claim order and
contain at most 16 claims. With context `C = 8,192`, output allowance
`O = 4,096`, framing reserve `R_V = 512`, and the identifier-aware
three-character token proxy, the complete system-plus-user input limit is
`V_request = min(16,000, 3 * (C - O - R_V)) = 10,752` Unicode characters.
There is no count-derived aggregate character ceiling. The actual plan must
contain at most 64 nonempty batches, each independently fitting V_request and
the 16-claim limit. The document claim budget, prompt/catalog correspondence,
single-claim fit and actual batch count are validated before runtime health or
inference. Worst-case work is bounded by 64 requests per pass, or 128 over the
existing two-pass ceiling; see the actual-batch amendment for its explicit cost.
The verifier must reject material relationship errors such as swapped table
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
`maxLength` values only when they exceed 192 characters (historical adapter
behavior, superseded by the proposed paraphrase-completion amendment above).
Bounds from zero
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
context-derived per-request input limit, and an explicit 64-batch maximum
per verification pass before inference. A native-text-free document, an
individually oversized item, an unrepresentable evidence catalog, or a plan beyond those limits fails
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
