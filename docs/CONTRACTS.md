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
  artifact persistence contain no model-family or SDK types. The current
  adapter uses native Ollama endpoints on exact-loopback HTTP and can be
  replaced without changing analysis, synthesis, verification, or Connect
  contracts.

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

### Direct verified paraphrases and explicit omissions

Status: the direct-summary and tolerant-paraphrase behavior is implemented by
`2ed9c07` and `8b0d039`, after their separate documentation-only contracts
`9d81f89` and `5c2c3e6`. The material-marker omission admission and analysis-v12
punctuation-boundary correction are implemented by `77742f4` and `12dd322` after
their documentation-only contracts `e5bc59b`, `4344ec0`, `cc10ba6`, and
`4e35c03`. Live corpus proof is reported separately in
LOCAL_MODEL_EVALUATION.md. This section replaces the retired proposal and its
accumulated amendments. Historical artifacts retain their original validation
rules. No optional shorter-summary feature is introduced.

Root cause: useful quote-bound paraphrases are regenerated and forcibly merged
to meet a page-derived claim target; generated grouping is a new failure surface.
The paraphrase response cannot record non-substantive complete-page input, and
raw-page coverage counts even explicitly omitted furniture. A seed-only retry
does not provide new information at the configured zero temperature.
Qualification from the trace: NARA response 5 already says "Permanent. Cut off
annually and transfer to WNRC. Destroy when 7 years old." The later merge spreads
that source/interpretation problem; it did not originate every factual defect.
Neither readable paraphrases nor model verdicts prove perfect faithfulness.

Required change surface: analysis omission enum and page paraphrase schema/parser,
versioned omission admission/reload, deterministic material-marker recognition,
direct synthesis materialization, verification admission and single-pass lifecycle,
coverage reporting/acceptance, fixture and negative tests, contract and evaluation
including complete delivered text.

- New runs use analysis version 12 and direct synthesis/verification/summary
  version 5. Version 11 remains readable with its original value-suffix
  completeness and phone-token boundary rules. Version 10 analysis remains
  readable with its complete-page-only model omission admission. Version 9
  remains readable with its 384-character validation and original omission
  admission; version 8 analysis and version 4 downstream artifacts remain
  readable under their original limits. Existing citation format remains
  unchanged. The technical omission is rejected in historical analysis where it
  was never valid.
- Carry every retained paraphrase unchanged into exactly one claim citing its
  single original evidence item, in source order. Materialize durable claim IDs
  and rendered page labels in Rust. No synthesis, assignment, pair reduction or
  model shortening call lies on the new default path. Thus one orphan batch
  cannot block the document: the one-to-one set is the default, not a rescue
  based on guessed assignments. No optional consolidation or adjacency pairing
  remains active for new runs. Historical negative tests may retain test-only
  generation helpers; these are not an alternate production path.
- Use 512 as an explicit pathological claim ceiling, not an output target or a
  floor. Do not derive a minimum claim count from half that ceiling. Require a
  nonempty supported result, exact one-to-one pre-verification coverage, and the
  existing 60 percent supported-evidence/eligible-page acceptance measures.
  Preflight actual verification batches against the unchanged per-request input,
  output/context/framing limits and 64-batch ceiling. The 512 ceiling is not a
  promise that every maximum-size catalog fits; reject impossible input before
  inference, never drop evidence or merge to force admission.
- Preserve the raw-size retention sample and headroom for ordinary documents;
  new retention R=min(N,ceil(3N/5)+16,512). If raw acceptance alone exceeds 512,
  reject as unsupported capacity before analysis instead of pretending full
  coverage is feasible. Historical plans are not recomputed under this formula.
- Paraphrase has disjoint typed outcomes: claim (claim_text) and
  no_substantive_content (no claim text). Expose the latter only when the selected
  exact quote covers the complete page text, not a truncated, sampled or partial
  catalog, and the complete text contains no deterministic material marker.
  Compare complete text with whitespace normalization only; no fuzzy deletion of
  words. A selected fragment cannot establish that unseen content is empty.
  Deterministic filters and their numeric/unit veto remain unchanged. Model
  omission is a recorded judgment, not proof that an illegible original never
  contained facts. Never omit substantive obligations, exceptions, values,
  uncertain or difficult assertions. Preserve a conservative keep option.
- Material-marker admission fails toward retaining content. It vetoes model
  omission when complete-page text contains any of: (a) an email-shaped token
  with a nonempty ASCII local part, `@`, a dotted ASCII domain and an alphanumeric
  final domain label; (b) a phone-shaped span of at most three whitespace tokens,
  seven to 15 ASCII digits and at least one `+`, `-`, `(` or `)` separator; (c) a
  numeric date with three one-to-four-digit fields separated consistently by `/`
  or `-`, or a case-insensitive English month name followed by a one/two-digit day
  and two/four-digit year; or (d) a five-to-64-character reference token beginning
  with an ASCII letter, containing at least two ASCII digits and `/` or `-`, with
  only alphanumerics, `_`, `/`, `-`, `?` or `.` inside. Leading/trailing ordinary
  sentence punctuation, including a terminal `?` on a phone span, is ignored for
  marker recognition. The compact `703-696-4959?` form also matches the permissive
  numeric-date veto; the corrected phone boundary is pinned with the non-date-shaped
  `(703) 696-4959?` form. These are omission vetoes, not validators of whether an
  address, date, phone or reference is real. Ambiguity retains content.
  The captured short NARA form containing a job number, phone, email and dates is
  a positive veto fixture. Short scan-noise pages containing none of these remain
  eligible for the model omission outcome. The exact standalone date/page-stamp
  grammar still takes the deterministic omission path before any model request.
- Treat 384 characters as the generation/repair target, not a correctness
  boundary. A complete, trimmed paraphrase through the 1,536-character decoder
  ceiling remains evidence and gets `LONG_CLAIM`; prefer a successful shorter
  repair but never truncate. If the single repair cannot produce a mechanically
  complete result and no usable long draft exists, record
  `ParaphraseUnrepairable` with the source/catalog bindings, warn with
  `PARAPHRASE_UNREPAIRABLE`, continue and backfill. This technical loss remains
  in both page-coverage denominators. Initial malformed, foreign or transport
  failures remain failures; cancellation is never converted to an omission.
  A repair transport/provider failure also propagates as a runtime failure; only
  a successfully received but mechanically unusable repair may become the
  technical omission. Once the initial response contains a claim, the repair
  response shape is claim-only: it cannot revise that page into
  `no_substantive_content`. A model materiality omission is accepted only as the
  initial complete-page outcome.
- Mechanical completeness accepts terminal sentence punctuation after an
  alphanumeric character. Analysis-v12 also accepts `%`, `‰`, `‱`, `°`, or a
  Unicode currency symbol when the preceding non-whitespace character is
  numeric, including inside the existing closing quote/bracket forms. A
  standalone/arbitrary symbol, ellipsis, missing terminal punctuation, cut-off
  word or trailing whitespace remains invalid. `$500.` was already valid because
  the period follows `0`; this correction does not weaken that boundary.
- Record ModelNoSubstantiveContent/NoSubstantiveContent using the existing page,
  chunk, source fingerprint, catalog fingerprint and filter-version audit fields.
  Analysis-v12 reload verifies whole-page admission, absence of current material
  markers and exact source/catalog binding. Analysis-v11 reload retains the prior
  phone boundary; version-10 reload retains its original whole-page-only admission.
  Reject unknown, duplicate, forged, mixed retained/omitted, or historically
  invalid outcomes. Backfill as before; omissions never count as retained evidence.
- Acceptance requires at least 50 percent raw native-text-page coverage and at
  least 60 percent omission-adjusted coverage, using integer comparisons and
  nonzero denominators. Only validated material omissions leave the adjusted
  denominator; technical omissions, uninspected, unsupported or ambiguous pages
  remain in it. Show raw and adjusted counts/fractions plus omission pages,
  origins and reasons together. A zero adjusted denominator is not success: no
  retained evidence fails explicitly. Verification losses cannot turn into
  omissions after the fact.
- Treat names, organizations, identifiers, dates, reference numbers and
  cross-references as material on form-shaped pages in both selection and
  complete-page omission prompts. Emit `OCR_TEXT_LAYER_STRUCTURE_RISK` when a
  deterministic scan-noise page exists or native text contains U+FFFD. The
  warning says claims reflect the extracted layer and layout/table relationships
  may be lost; it is not a general OCR detector. Existing warning propagation
  renders it in the UI. NARA remains a robustness fixture, not OCR-table accuracy
  evidence, because its known row-fused claim survives semantic verification.
- Verify the unchanged one-to-one claim set once. Withholding remains authoritative;
  a nonempty result persists with durable warnings on loss, zero supported claims
  still fails closed. Delete seed-only re-synthesis; persisted attempt lineage
  records only the actual attempt. Do not claim run-derived seeds alone make a
  greedy retry meaningfully different. Keep the bounded paraphrase repair because
  it changes the prompt with typed violations and the rejected draft where needed.

Verification requirements: direct synthesis makes zero model calls and preserves text,
IDs/order/exactness at 1, 83, 512 and rejected 513 items; verify actual serialized
batch limits. Whole-page omission positives and partial/mixed/tail negatives run against
schema admission and parsing. Live short-obligation/amount/exception controls
check the model's judgment separately: full-page admission cannot mechanically
prove that a model omission is correct. Probe partial quote denial, unknown outcomes, duplicate/mixed
page results, fingerprint tampering, historical reload and zero denominator.
Prove a verification loss never invokes synthesis or a second verifier pass;
retain existing provenance, verdict and persistence failure tests. Run local
all-target tests, strict clippy, fmt and both Qwen corpus acceptances. Print the
complete delivered text and report raw/adjusted coverage, claims, evidence,
omissions, requests, completion tokens and wall time, failures first. Judge
readability and obvious source-fidelity concerns separately from numeric gates.

Explicit non-scope: relaxed support/exactness/provenance or source parsing,
retuning deterministic noise predicates, automatic repair of source meaning,
model/endpoint/temperature/context/output/timeout changes, optional UI summary
styles, DB migration or broader unfinished attempt-audit work, Connect, OCR,
vision, packaging and CI. Any residual factual or omission failure is reported,
not repaired by relaxing coverage or retrying until a favorable run appears.

### Boundary-safe analysis quotation catalogs

Status: implemented. The locked all-target/all-feature Rust suite, strict Clippy
with warnings denied and formatting gate pass locally. The opt-in NARA, DOL and
captured 134-page live reruns are reported separately and are not implied by
these deterministic gates.

Root cause: analysis version 12 bounds a quotation by scanning the latter half
of a 600-character window with one mutable preferred boundary. A later
whitespace character overwrites an earlier terminal-punctuation boundary, so a
prefix of the following sentence becomes an ordinary exact-quote candidate and
the next candidate begins with its suffix. Exact-substring, provenance and
mechanical claim-completeness checks all accept that construction, while the
verifier sees only the clipped quotation and cannot recover source text omitted
before inference.

Required behavior:

- New runs use analysis version 13.0.0. Preserve the exact version-12 quote
  splitter and route version-12 reload through it; do not reinterpret stored
  version-12 quote signatures, evidence identities, omissions or warnings.
  Synthesis, verification, summary and citation versions remain unchanged.
- A complete normalized source block whose trimmed text is at most 600 Unicode
  characters is one ordinary quote candidate even when it has no terminal
  punctuation. The catalog has removed no source context in that case; this
  preserves bounded headings, labels and slide lists without claiming that
  they are grammatical sentences.
- Split only a source block whose trimmed text exceeds 600 characters. Each
  resulting ordinary quote candidate consists only of one or more complete
  source sentence units, in source order, and is at most 600 Unicode characters
  after trimming. Greedily pack whole units until the next unit would exceed
  the limit. Never expose a prefix or suffix of one unit as an ordinary
  candidate. After an unsafe unit, resume at the next safe sentence boundary so
  a substantive tail remains eligible.
- Derive sentence-boundary proposals with Unicode sentence segmentation, then
  conservatively coalesce proposals at decimals, initials, acronyms,
  abbreviations and ellipses. A boundary that cannot be classified safely is
  not a split. False negatives may reduce the candidate catalog and warn; false
  positives must not admit partial source assertions.
- Within an over-budget block, a source sentence unit longer than 600
  characters or a nonempty source tail without a safe terminal boundary is
  unavailable to ordinary quote selection. A no-terminal whole block is
  therefore admitted through 600 characters and omitted at 601. Record a
  durable `ANALYSIS_QUOTE_BOUNDARY_OMITTED` warning whenever a page loses any
  source unit for this reason. If that page has another safe candidate,
  selection proceeds over only the safe catalog. If it has none, make no model
  call and record a deterministic `QuoteBoundaryUnusable` technical page
  omission with the existing page/chunk/source-fingerprint audit binding.
  This omission counts in both raw and omission-adjusted coverage denominators,
  does not count as retained evidence and remains eligible for ordinary
  backfill/exhaustion behavior.
- Version-13 validation reconstructs the boundary-safe catalog and its
  warning/omission state from normalized source. It rejects a candidate that is
  neither an exact complete bounded block nor an exact bounded substring ending
  at a safe source-sentence boundary, duplicate or reordered candidates, forged
  warnings/omissions, and any page represented as both retained and omitted.
  Candidate and evidence identities bind analysis version 13.0.0 and the exact
  admitted bytes.
- Quote-boundary loss is deterministic source admission, not model judgment or
  paraphrase failure. Do not label it `ParaphraseUnrepairable`, permit the model
  to authorize it, remove it from the adjusted denominator or retry it with a
  different seed.

Verification requirements: add failing-before regressions for the four captured
mid-sentence endings and assert that the following candidate begins at the full
next sentence. Probe valid complete sentences at 599, 600 and 601 characters;
punctuation followed by later whitespace; a single over-budget sentence;
no-terminal text at 599, 600 and 601; a bounded DOL-style heading/list block; a
safe sentence before and after an unsafe unit; decimals,
initials, acronyms, common abbreviations, ellipses, closing quotes/brackets and
Unicode sentence punctuation. Prove exact source bytes, tail coverage, catalog
order/uniqueness, request limits and deterministic identities. Prove a stored
version-12 artifact still reloads with its original catalog while an equivalent
version-13 artifact rejects every partial candidate. Run all-target tests,
strict Clippy, formatting, NARA and DOL corpus acceptance, then rerun the
captured 134-page PDF and inspect the four known pages plus the complete
delivered artifact. Report coverage, warnings, claims, requests, completion
tokens and wall time; hosted CI is not implied.

Explicit non-scope: model family/size/runtime, context/output/temperature/
timeout, page sampling and retention formulas, claim ceilings, verifier support
rules, parser/OCR/vision, synthesis consolidation, database schema, Connect,
packaging, citation rendering and the detailed-claims user experience. Do not
special-case the four pages, truncate source or generated claims, weaken exact
provenance or accept unsupported/ambiguous claims.

### Delivery-scoped Connect summary byte admission

Connect v1's 1 MiB UTF-8 summary-text ceiling is an optional delivery policy,
not a standalone-summary limit. One UI-neutral pipeline constant is authoritative
for both the Connect policy and wire projection. Standalone service calls supply
no delivery policy and may persist and reopen a larger valid summary unchanged.

For a current Connect run, analysis accumulates the exact UTF-8 bytes of whole
rendered claim lines in source order, including blank-line separators and
application-derived citation labels. The first claim that would exceed the
ceiling is not admitted; analysis records one durable
`SUMMARY_TRUNCATED_FOR_DELIVERY` warning and schedules no later page model calls.
Synthesis and verification continue over the admitted nonempty prefix, which
must still satisfy the existing raw and omission-adjusted coverage gates. A
delivery limit is not an omission and removes no page from either denominator.
Before accepting the early stop, Rust checks that the retained prefix itself
already clears both floors; otherwise analysis fails immediately with the
delivery-capacity error. The delivery warning replaces the ordinary exhausted-
plan shortfall warning because later pages were deliberately not inspected.
After verification, Connect recomputes both floors from the actually supported
claim citations and fails closed before persistence if either has fallen below
its threshold.
The first non-fitting claim may already have consumed its selection/paraphrase
calls because its exact bytes are unknowable earlier. Final completion checks the
actual supported rendering against the same ceiling before persistence.

Wire projection remains defensive for historical or resumed artifacts. It
derives claim boundaries from the persisted citation artifact and, when complete
text or serialized JSON does not fit, persists the largest nonempty whole-claim
prefix satisfying both the 1 MiB text and 2 MiB JSON limits. It adds the same
warning idempotently without mutating the pipeline artifacts. If even one whole
claim plus required metadata and warnings cannot fit, delivery fails closed.
Before completion, the provider recomputes both page-coverage floors from the
exact prefix selected by wire projection; JSON-escape expansion cannot silently
shrink a previously valid result below either floor. An under-covered wire
prefix fails instead of being stored as a completed Connect job.
No path splits a UTF-8 scalar, claim, citation label, or JSON escape, and no
truncation can rescue an otherwise invalid or unsupported claim.

New direct synthesis retains its 512-claim pathological ceiling. Historical
hierarchical generation and reload retain their 64-claim compatibility ceiling,
named `LEGACY_MAX_SUMMARY_CLAIMS`; neither value is recomputed by delivery policy.
The boundary suite pins exact-limit and one-byte-over supplementary-plane text,
separators and citation labels, stopped model scheduling, warning idempotence,
whole-claim JSON fallback, standalone isolation, both claim ceilings, and a
persisted completed Connect result.

### Analysis, deterministic filters and model boundaries

`ModelRuntime` remains the only inference boundary. New analysis uses a
page-local selection call followed by a quote-only paraphrase call. Selection
receives an application-built candidate list and an enum of short local IDs;
it cannot supply quotation bytes or durable identities. Paraphrase receives
only the selected quotation, never other candidates. Rust restores the exact
substring, authoritative block, source span and deterministic evidence ID.
Neither selection nor paraphrase proves semantic support; verification remains
required. The candidate builder covers the page's blocks and tail within the
existing bounded catalog and fails on construction/input errors.

The current claim-text limit is 384 Unicode characters, based on the observed
faithful 333-character regulatory draft rounded up with modest headroom, not the
retired multi-item output-packing quota. Generation targets 55 words; Rust's
character limit remains authoritative. The decoder ceiling is four times larger,
1,536 characters, to avoid forcing a string closed at the acceptance boundary.
The output allowance remains 2,048 tokens. Rust never truncates a draft.

Completeness and length are separate predicates. Require terminal sentence
punctuation, optionally followed by closing quotation/bracket marks. The content
immediately before it must end in an alphanumeric character or the bounded
numeric value-suffix form described above. Reject ellipsis, dangling punctuation,
trailing whitespace and the captured mid-word cutoff. This is a mechanical
completeness check, not proof of grammatical or semantic completeness. One
bounded paraphrase repair changes the prompt with
typed violations; for length failures it includes the rejected draft, measured
length/word count and the 55-word shortening target. A second failure stops.
Malformed JSON, foreign selections and transport errors do not gain arbitrary
retries. Each call retains input limits, cancellation checks and diagnostics.

Filter version 1.0.0 is unchanged. Inspect the complete original normalized page
before admitting a deterministic omission. Empty or uncertain input is retained.
The scan-noise predicate requires BOTH no Unicode-aware multi-letter word and
at least 80 percent punctuation/symbol/control characters among non-whitespace
characters. Letters separated only by combining marks count as a word. Unknown,
private-use or format characters veto noise classification. Recognizable numeric
content vetoes the noise rule, including numeric tables, standalone numbers,
dates/values, currency or percent quantities, and recognized single-letter units.
An operator followed by a numeric value, including a single-digit comparison
such as `< 5`, is recognizable numeric content even when it is the only value
on a punctuation-heavy page. Operator/value recognition must not depend on two
numeric tokens or a multi-digit run.
The captured NARA page containing `5l` is retained by the unit veto; the
deterministic filter must not be tuned to remove it.

The separate date/page-stamp predicate is anchored to the entire trimmed page,
at most 32 Unicode characters, with exactly two whitespace-separated fields.
The date has one/two ASCII month digits, one/two day digits and two/four year
digits separated by slashes. The marker has one/two ASCII letters, a hyphen and
one to three ASCII digits. Standalone dates, added assertions and partial matches
do not qualify. The captured `03/10/03  A-4` is a positive fixture. This is
a narrow grammar, not a general assertion detector or calendar validator.

Bare-heading omission remains an optional selection outcome only on Rust-admitted
heading-shaped complete pages: short letter/whitespace-only title-shaped text,
without the versioned assertion/obligation/exception markers. This is admission
for model judgment, not proof of non-substantiveness. The new paraphrase omission
is separate and requires the complete-page quote binding described above.
A model's mistaken materiality judgment remains a residual risk even when that
binding is valid. A short obligation, exception, amount, table value or uncertain
assertion must be retained; live controls complement, not replace, filter tests.

Analysis starts from the versioned evenly spaced plan, including the tail, then
backfills unvisited native-text pages in deterministic source order until R
retained pages or exhaustion. Each inspected page has exactly one evidence item
or recorded omission. Omissions do not count toward R. At most N selections and
2*N paraphrases can run, including the single repair; deterministic omissions
need no model call, heading omissions need no paraphrase. Inspected order,
stopping condition, unique outcomes, source and catalog fingerprints are
revalidated on reload. An exhausted target produces a durable warning, not
fabricated evidence. Zero retained evidence fails explicitly after analysis is
persisted. Partial analysis that fails before a valid artifact is committed is
not promised durable per-request decision storage; that broader audit remains
deferred.

### Verification, persistence and historical compatibility

New synthesis is deterministic materialization, not generation. It preserves
every retained paraphrase, its single evidence binding and source order, with
claim identifiers derived under version 5. Its validator reconstructs the
expected set and rejects omission, reordering, rewritten text or rebound sources.
No optional consolidation, assignment response or adjacency reduction is on
the production path. Historical generation helpers are compiled only in tests
to retain their negative regression fixtures.

The complete verification plan must fit before any verifier inference.
Requests use request-local `k1` claim IDs and local evidence metadata; the
response claim ID is an enum of exactly the current batch's IDs. Rust restores
durable identities and requires one unique valid verdict per supplied claim,
rejecting missing, duplicated, foreign or mixed-invalid results. No model-copied
identifier may be an unconstrained string. Enums enforce membership, not
uniqueness or semantic support.

Each batch is bounded by 16 claims and the actual serialized system-plus-user
size. With assumed context C=8,192, output O=4,096, framing reserve 512 and the
three-character token proxy, input is at most
`min(16_000, 3*(C-O-512)) = 10,752` Unicode characters. The actual plan has at
most 64 nonempty batches. There is no count-derived aggregate-character estimate.
A claim too large to fit alone or a plan beyond the batch limit fails before
inference; nothing is silently dropped. The proxy is conservative planning,
not exact tokenization or a claim that the adapter configures model context.

The verifier receives only claims and their exact quotations, not outside
facts. Its prompt checks actor/action/object relationships, swapped table
columns, wrong actors, negation, modality, exceptions, purpose and quantities.
Only supported claims reach final text and citations. Unsupported and ambiguous
verdicts remain auditable. Nonempty withholding produces durable warnings
without another synthesis/verifier pass. Zero supported claims still fails
closed. A supported verdict is the model's judgment, not human fact-checking.

Schema version 14 remains unchanged. Synthesis/verification attempt tables are
append-only, keyed by run and ordinal, with immutable SQL triggers. New runs
write the actual direct synthesis and single verification as ordinal zero.
Historical ordinal-one attempts remain readable; new version-5 verification
cannot claim ordinal one. The accepted verification names the exact synthesis
attempt it filters. Expected-state/version checks, cancellation races,
transactional transitions, immutable events, source identity, row hashes and
independent-reopen validation remain authoritative.

New artifacts use analysis 12.0.0 and synthesis/verification/summary 5.0.0,
with citation format 3.0.0 unchanged. Historical analysis 11.0.0 retains its
prior terminal-value and phone-token boundaries. Analysis 10.0.0 retains its
complete-page-only model omission admission. Analysis 9.0.0 retains the
384-character ceiling and its original omission admission; 8.0.0 retains its
earlier retention formula; 7.1.0/7.0.0 retain the 384-character validation era;
6.0.0 retains 192-character completeness checks; earlier artifacts retain their
versioned older rules. The technical omission is rejected in all historical
versions where it was never valid. Historical synthesis/verification/summary
4.0.0 retains the page-derived B/K and generation-attempt lineage rules when
loaded; 3.0.0 and mechanical 2.0.0 remain readable with their original citation
formats. Resuming an already synthesized or verified checkpoint does not
relabel its artifact or invent a new prior attempt. A new direct synthesis from
validated older analysis is version 5, not a rewrite of the older evidence
identity.

### Runtime, reporting and explicit limits

The supported runtime is native loopback Ollama, default
`http://127.0.0.1:11434/`, with an exact-digest qualified Qwen preset selected
through persisted application settings. The timeout remains 900 seconds, with
separate short connection/health limits. The adapter rejects non-loopback HTTP
hosts, credentials in the URL, proxies and redirects; optional credentials come
from its bounded token file. Deployment endpoint, timeout and token-file
overrides remain available; the desktop model choice is not an environment
variable. All source text is marked untrusted in prompts. Temperature remains
zero and native requests set `think: false`; template/runtime behavior must be
measured.

Run-derived signed-range seeds remain in requests for reproducibility. A changed
seed alone is not represented as a meaningful greedy retry. Actual paraphrase
repair changes its input. Each logical request records stage/ordinal, output
allowance, elapsed time, transport and provider prompt/completion/total tokens.
Missing usage is not estimated. Ordinary diagnostics contain no source text,
prompt, model response, credential or private path.

Decoder projection remains non-mutating. Unsupported `uniqueItems` is removed;
numeric `maxLength` survives up to 1,536 and is stripped above it. Object
closure, required fields, enums and supported cardinality bounds survive.
Rust remains authoritative for exactness, identity, uniqueness, completeness
and accepted lengths. Only the exact vocabulary-loading failure may retry in
JSON-object mode; other failures do not trigger that fallback. Transport
success is not proof of semantic correctness.

Analysis system-plus-user input retains the defense-in-depth character bound
`min(16_000, 3*(C-2048-512))`, where `C` is the admitted analysis profile
context; the pinned-tokenizer admission over the full native payload is
authoritative.
Source chunks retain their 100,000-character admission guard. The claim ceiling
does not remove page/catalog/input/output/verifier constraints. New direct
synthesis has no model-request budget to exhaust; the old 256-request
synthesis budget applies only to historical generation fixtures, not a
new run-wide guarantee. Requests are cancellable at the existing boundaries.

Live acceptance proves complete pre-verification evidence coverage and at least
60 percent supported-evidence and omission-adjusted native-page coverage, with
a nonempty result and no more than 512 claims. Print raw and adjusted fractions,
recorded omission origins/pages, retained/supported evidence, request/stage
counts, tokens and wall time together. Neither uninspected pages nor withheld
claims reduce the denominator. A zero denominator is not a passing fraction.
Print complete delivered text only through the existing explicit source-text
opt-in; default reports continue to hide source and summary text.

Local gates include exactness/provenance/fail-closed negatives, source-ordered
direct materialization, capacity edges, actual verification batches, omission
admission/tampering, historical reload, and single-pass lineage. Marker boundaries
cover every marker class, mixed valid/invalid text, a 462-character form that must
not receive the omission schema, short scan noise that must receive it, and the
standalone date/page-stamp path that must make no model call. Live NARA and DOL
results must lead with failures and distinguish gate success from readability and
source fidelity. Do not rerun until a favorable sample or change omission
thresholds to pass.

Merge admission requires a review whose commit OID equals the current pull-request
head, followed by a separate fresh unresolved-thread and review-state poll. A
thread count from a previous-head review is not evidence about the final head.
PR #30 violated this ordering when its final-head review arrived after merge; this
correction must not repeat that process failure.

#### Native transport and discovery

Production generation uses native `POST /api/chat`; discovery uses `/api/tags`
and `POST /api/show`. Requests retain temperature zero, the run-derived seed,
the stage output allowance and the projected JSON schema. Native options carry
the qualified `num_ctx` and `num_predict`; the schema is sent in `format`.
Responses record provider prompt and completion counts when supplied. The same
loopback-only URL, no-proxy, no-redirect, bounded credential-file, timeout,
diagnostic-redaction and exact schema-fallback boundaries remain authoritative.
Each qualified chat request asks Ollama to retain its runner for a bounded
execution-provenance check. Qualified generation uses native NDJSON streaming.
After receiving the first non-final frame and before reading the final frame,
the adapter reads authenticated `/api/ps`, caps its record count before
filtering, and requires exactly one active record for the response model name
with the digest admitted for that run. This overlaps the proof with the still
active request instead of correlating output with a later global runner state.
Only after that proof may the remaining frames be read and their message
content assembled. A final-only response, changed model name, oversized,
missing, ambiguous or mismatched runner catalog, malformed frame, mid-stream
error, missing final frame or data after the final frame rejects the entire
output. No partial content reaches a stage parser or artifact.

The running-model record cap is the same explicit 256-record ceiling used for
installed-model discovery and applies on every provenance query. The bounded
response-byte limit remains independent. A post-response `/api/tags` or
`/api/ps` lookup is insufficient because mutable global state can change after
the completed request. Ollama 0.24.0 does not accept either a raw digest or a
`sha256:` digest as the chat model address, so request-time streaming proof is
the supported binding for this adapter rather than nominal content addressing.
Unqualified qualification probes may consume the same bounded stream without a
digest proof; they never become product runtimes or persist qualified identity.
Execution-provenance rejection is recoverable: the unproven response is
discarded, the immutable run snapshot remains authoritative, and retry admission
must recheck that exact profile before creating work. A permanently unsupported
or unqualified profile remains a terminal configuration error.
The same temporary exact-digest drift detected by a stage-boundary health check
is recoverable; restoring the snapshotted digest may admit a retry, while the
current stage remains rejected.

Qualified direct GGUF generation uses an app-managed llama.cpp runtime, not LM
Studio, Ollama import or a user-entered endpoint. The runtime executable is
resolved from the deployment override or `PATH`. Because it is dynamically
linked, the qualified runtime identity includes the executable and every
profile-named application-local llama.cpp/ggml dependency. The app opens and
hashes each resolved regular file, copies every verified executable and library
into an anonymous sealed file that cannot be written, grown or shrunk, exposes
required library names through a private directory of inherited
`/proc/self/fd` descriptors, and launches the sealed descriptor-bound
executable without a shell. Qualification never executes or loads bytes from a
mutable source descriptor after its digest check. The parent keeps `CLOEXEC` set;
only the forked llama child clears it immediately before `exec`, so another
concurrent child cannot inherit the model or runtime bundle. Path replacement
after admission cannot select different model, executable or application-local
library bytes; host C/C++, CUDA driver and system runtime libraries remain the
operating-system trust boundary. Direct GGUF execution is Linux-only in this
release.

The registered GGUF itself remains on its source filesystem rather than being
duplicated into RAM or app storage. A dedicated supervisor thread, created once
and retained for the application process lifetime, is the sole executor of the
complete direct-runtime startup critical section: model open, read-lease
acquisition, registered-identity checks, runtime-bundle preparation and child
spawn. Callers from Tauri blocking pools or other transient threads submit that
work and wait for its typed result; they never become the kernel parent task of
the child. An unavailable or terminated supervisor fails closed before a child
can be admitted.

Before fork, the supervisor must acquire a Linux read lease on the retained
read-only descriptor; acquisition fails if a writer already has the file open.
Before taking parent ownership of that lease, the supervisor installs the
lease-break handler and explicitly unblocks `SIGIO`. Parent ownership is
targeted to that exact Linux supervisor thread rather than to an arbitrary
thread in the process, so another thread cannot defer the handoff notification.
Complete registered file identity is checked only after that lease is held.
Runtime-bundle preparation may then take time, so immediately before spawn the
same supervisor thread requires a clear break flag, an intact kernel read lease,
and the unchanged complete GGUF identity again. A signaled, expired or drifted
lease fails without executing the loader. A modify-and-close between an
unprotected identity check and lease acquisition therefore cannot reach the
loader. The child becomes the lease-break signal owner with
the default terminating disposition before `exec`, and explicitly unblocks the
lease-break signal in the child so a signal mask inherited from the spawning
thread cannot defer termination. Separately, before installing
`PR_SET_PDEATHSIG`, the child resets `SIGTERM` to its default disposition and
removes `SIGTERM` from its inherited signal mask. An ignored, handled or blocked
`SIGTERM` in the supervisor process therefore cannot make the one-shot
parent-death notification inert. Only after that normalization may the child
install `SIGTERM` as its parent-death signal and verify the captured parent PID;
any normalization, installation or parent check failure aborts before `exec`.
A later write-open therefore blocks in the kernel and terminates the loader
before the writer can reach the live inode. The
supervisor checks the delivered break flag again after spawning and
kills/reaps the child instead of accepting a lease whose thread-directed parent
notification raced with the handoff. Once ownership reaches the child, a break
terminates it; cache reuse also rejects and reaps a child that has exited. The
parent releases the lease only after the child is terminated or ownership ends.
A filesystem without working read-lease enforcement is not admitted for direct
GGUF execution. Post-load metadata checks remain defense in depth, not the
mechanism that prevents mutable bytes from reaching the loader.

The child binds a Unix-domain socket inside its owner-private runtime directory
with an unpredictable per-process bearer token and exact digest alias. The
application retains that directory for the child's lifetime, and its HTTP
client connects only through the socket; there is no released TCP port for a
different local process to claim before the first credential-bearing request.
The token is written to an owner-private file inside the same directory and
supplied through the qualified server's API-key-file option; neither the token
nor source text appears in child argv. It uses the qualified context, one parallel
slot, reasoning disabled, web UI disabled, offline mode, no warmup and GPU
layers enabled. Proxies and redirects remain disabled. Startup has one bounded
deadline and verifies health, digest alias, active context and trained context
before inference; it also rechecks the retained model descriptor after load.

The private runtime directory must not be created through ambient `TMPDIR` or
the process-global temporary-directory resolver. Runtime construction receives
the explicit model-settings directory. Product startup creates or tightens its
app-data/model-settings directory to effective-user-owned mode `0700` before
database, settings or runtime use; a foreign owner or non-directory fails
startup. Runtime construction canonicalizes that directory and rejects any
ancestor that is not a directory. Every canonical ancestor must be owned by
root or the effective user regardless of its current write bits: an unrelated
owner can add owner-write permission later and rename or replace the admitted
subtree. For root/effective-user-owned ancestors, group/other write permission
is accepted only when the sticky bit is set. Sticky storage controlled by
another unprivileged account is not trusted.
Beneath that admitted location the application creates one dedicated
runtime root with mode `0700`; an existing root is accepted only when it is a
real directory owned by the effective user with exactly mode `0700`. A symlink,
foreign owner, broader mode, missing/untrusted ancestor or metadata failure is
a typed pre-spawn runtime failure. Per-child runtime directories are created
only inside that verified root. This makes another account unable to rename or
replace the leaf between admission and socket bind; checking only the leaf mode
under an ambient attacker-writable parent is not sufficient. Boundary proof
must accept root-owned and effective-user-owned private/sticky ancestry, reject
a foreign-owned ancestor even when it is currently mode `0555`, reject
non-sticky writable ancestry on both sides, and prove the staged child remains
beneath the admitted root.

Because the qualified server has one inference slot, the application admits at
most one direct-model request at a time for a shared child. Callers wait on an
application-owned mutex before tokenization or completion begins. Slot
acquisition observes the run's cancellation control and has one absolute wait
deadline no longer than the existing model-request timeout; it does not wait a
fresh timeout for each caller ahead of it. Cancellation leaves the queue without
sending a model request, and queue-timeout returns recoverable busy without
invalidating the shared child. Request duration and HTTP timeout begin only
after that caller owns the slot. A queued caller therefore cannot spend its
network timeout behind another generation or kill the shared child when only
the queue wait was long.
Before spawning, the process PID is captured. Immediately after installing the
parent-death signal, the child compares its actual parent with that captured PID
and aborts before `exec` if the process died during the fork-to-handoff window;
setting a parent-death signal after reparenting is not accepted as cleanup
protection. Linux ties `PR_SET_PDEATHSIG` to the task that called `fork`, so the
durable supervisor must perform the spawn itself and must not retire while the
application process remains alive. A transient caller returning or its blocking
pool thread retiring therefore cannot terminate a healthy cached child; process
exit still terminates the supervisor and delivers the configured child signal.
Boundary proof must cover inherited blocked and non-default `SIGTERM` states in
addition to the ordinary state: the pre-exec normalization restores the default
disposition and unblocks the signal before parent-death installation, while an
unchanged unrelated blocked signal remains blocked. A probe whose parent exits
after handoff must not leave the child alive under any admitted inherited
`SIGTERM` state.
Stage-boundary health repeats the registered model path identity, retained
descriptor identity and loaded alias/context checks. Cache reuse first repeats
the registered-path and retained-descriptor checks; deleting, unlinking or
replacing a registered path can therefore never revive a cached ghost runtime.
Child output is discarded without persisting prompts or source text. One child
is shared for the exact model/context/runtime-bundle identity; a different
direct identity cannot coexist while the cached child is actively referenced.
When a new direct identity is admitted, map-only idle children are terminated,
reaped and evicted before startup; actively referenced children instead produce
the recoverable busy result. Cached reuse requires both unchanged registered
identity and a live owned child. A child terminated by a lease-break signal or
any other exit is reaped and the dead entry is evicted rather than returned as
a usable runtime. Selection change prunes only map-owned idle entries; an
externally referenced runtime remains discoverable under its exact cache key,
so selecting that profile again shares the existing child rather than starting
a second process. Ollama construction removes idle direct entries but returns
recoverable busy while any direct entry is externally referenced. App exit is
the only selection-independent path that deliberately clears every cache entry.
Any model identity failure atomically makes the cache entry unavailable and
terminates and reaps the owned child even if another reference still exists.
Every child is terminated and reaped on ownership loss, startup failure or
identity failure.

Direct generation uses llama.cpp `POST /completion` with a pinned minimal Qwen
ChatML token sequence containing only the existing system and user messages plus
the assistant prefix. Framing is tokenized as Qwen control syntax, while system
and user content are tokenized with special-token parsing disabled; delimiter-
shaped source text therefore remains untrusted message data and cannot create a
new role. The GGUF embedded template and OpenAI-compatible chat route are not
admitted. The projected schema is sent as `json_schema`; there is no schema
fallback. The same server's `/tokenize` endpoint counts the exact prompt before
inference. The counted input, output allowance and framing reserve must fit the
qualified context. Prompt or output truncation, an output-limit stop, an absent
or unknown stop reason, an empty response, malformed or oversized JSON, absent
token accounting, or token accounting that differs from the admitted prompt
and output ceiling fails closed. A completed response is admitted only when the
qualified server reports an end-of-sequence or configured stopping-word stop;
syntactically valid JSON produced exactly at `n_predict` is not evidence that
generation completed. The private child, bearer token and startup-verified
digest alias establish runtime identity; a mutable per-completion path string
does not.

Before direct startup, the bounded loopback Ollama API is checked for resident
models whose digest exactly matches an Ollama profile admitted by this
application. A matching resident digest returns a recoverable busy result; the
operator can retry after the application's bounded Ollama keep-alive expires.
Only an exact loopback connection refusal proves that no Ollama listener is
present and permits direct startup without a residence response. A timeout,
connection reset, malformed response, rejected status, or any other ambiguous
probe failure is recoverable and fails closed before the direct child starts.
Every record in a successful non-empty residence response must carry a canonical
64-character lowercase hexadecimal digest. A missing or malformed digest is an
unverifiable resident and fails recoverably before direct startup, even when its
name or other fields appear unrelated. A valid foreign digest remains untouched
and does not become an admitted model. None of these ambiguous states is treated
as evidence that GPU residency is empty.
The application does not request unload through a mutable model alias because
the local Ollama API cannot atomically bind that name-addressed operation to the
digest observed by `/api/ps`. Same-name/different-digest and unrelated models
are never targeted or treated as an admitted Ollama dependency. An unreachable
Ollama service with no loopback listener is not a direct-runtime dependency.

Discovery admits only Qwen-family architectures that the application explicitly
supports. An installed descriptor records the exact Ollama name and digest,
byte size, architecture, parameter-size and quantization metadata when reported,
the model maximum context read from architecture-scoped `model_info`, and its
qualification state. Names, filenames and display metadata are not capability
proof. Missing, malformed, zero or implausible context metadata leaves a model
visible but unqualified; it never silently falls back to 8,192. Non-Qwen models
remain visible only as unsupported installed entries and cannot be selected.
The bounded `/api/tags` body may contain at most 256 model records. Discovery
rejects a larger catalog before any `/api/show` request, and all metadata probes
share one five-second aggregate deadline that begins before the tags request.
Response-body bounds are not retained-memory bounds: every externally supplied
string copied into an installed descriptor is therefore limited to 512 UTF-8
bytes, and the sum of all retained descriptor strings is limited to 256 KiB.
The per-field guard applies to names, digests, architecture/family,
parameter-size and quantization metadata before the value can be returned or
used to construct another request. The aggregate guard is checked while the
descriptor vector is built, before it can be serialized to the webview. An
oversized field, arithmetic overflow, or aggregate maximum plus one fails the
catalog as a whole with no truncated or partially trusted metadata; exact
maxima remain valid.
Per-request timeouts remain defense in depth; they do not multiply into an
unbounded startup delay. Reaching any discovery bound fails the catalog as a
whole rather than returning a silently truncated model list.

#### Qualified profiles, context and token planning

A selectable model profile is keyed by immutable model digest, Qwen tokenizer
family and profile version. It carries a measured safe context no greater than
the discovered model maximum, supported stages and corpus qualification result.
There is no free-form context slider. A profile whose digest no longer matches
the installed model becomes unavailable until it is qualified again.

Every request uses the selected stage profile's context. The runtime exposes
that value to the planner; analysis and verification therefore need not share a
context. Input admission counts the actual serialized native system/user/schema
payload with an application-pinned tokenizer for that Qwen tokenizer family,
then reserves the output allowance and an explicit framing margin. The pinned
asset may be reconstructed from Ollama's verbose GGUF token and merge tables
only when their canonical fingerprint and pre-tokenizer kind exactly match the
profile; missing or changed metadata fails before inference. Qwen 3 and Qwen
3.5/3.8 tokenizer fingerprints, pre-tokenizer implementations and versions are
distinct. Character limits remain defense-in-depth payload caps, not token
estimates. A request that cannot fit the qualified context fails before
inference with measured token counts; the adapter does not ask Ollama to
truncate or expand context implicitly.

Token admission counts the exact serialized native-chat payload that is sent.
The pinned counter must not apply NFC, NFD or any other Unicode normalization
unless the transmitted payload is changed by the identical operation first;
decomposed source text therefore retains its actual byte-level token cost.
Removing the prior counter-only NFC step advances both Qwen tokenizer profile
versions rather than changing the meaning of a persisted version in place.
Regression evidence compares canonically equivalent composed and decomposed
input through the production pre-tokenizer and proves that the extra transmitted
code point is not normalized away before the context decision.

The supported candidate matrix is Qwen 3.5 4B, Qwen 3.5 9B, base Qwen 3.8 27B,
Jack Qwen 3.8 27B Coder and the existing Qwen 3 30B-A3B baseline. The locally
available Jack GGUF is a distinct candidate, not an alias for base Qwen 3.8 and
not presumed equivalent from its filename or footprint. Models below 4B are not
exposed as product presets in this slice. A weak candidate must not reduce the
baseline build's limits, prompts, validators, coverage targets or verifier
standard.

The current product registry contains the exact Qwen 3 30B-A3B Ollama digest as
the default and strongest verifier, plus the exact Jack Qwen 3.8 27B Coder GGUF
`e7fecb29086afb4f6ca054b0f1469f2704a24e56db27c5980827f5f32d26f041`
as an opt-in full profile at 8,192 tokens. Jack is admitted only with the exact
qualified llama.cpp runtime manifest recorded in the qualification evidence.
It does not create a cross-runtime hybrid because both runners exceed the
qualified GPU-memory envelope. The 4B and 9B candidates remain visible but
unqualified; base Qwen 3.8 remains unqualified because the installed GGUF did
not load through the tested Ollama path.

#### User settings and stage routing

The desktop exposes installed Qwen descriptors and qualified presets, including
model size, family, maximum context, qualified effective context and disabled
reason. The user selects a persisted preset in the application rather than by
editing an environment variable. Settings are written atomically in the private
application-data directory; malformed, unknown, unsupported or stale-digest
values fail closed to an explicit unavailable state, never to an arbitrary
installed model. A settings change affects new runs only.

Runtime-profile leases cover desktop and Connect work. Multiple jobs may share
one exact analysis/verification profile, but a differently keyed runtime cannot
start or release a resident runner while the old profile has a live owner. Such
an attempted handoff returns a recoverable busy status; it never unloads the
runtime beneath an active job. Same-preset selection is a no-op rather than a
cache eviction. Constructing an Ollama runtime clears an idle app-managed direct
child; an active direct owner prevents construction at the preceding lease
boundary. Cache retention independently preserves the discoverable child for
same-profile work while a selection changes away and back.

Multiple installed Ollama names for the same qualified immutable digest are
aliases, not distinct product presets. The installed-model catalog keeps every
name visible, while preset construction deduplicates by qualified digest before
full or hybrid routing and chooses the lexicographically first enabled alias as
the deterministic name for a new run. Therefore every rendered preset ID is
unique and still identifies one immutable capability profile; persisted preset
selection cannot resolve to a different duplicate row. Historical runs remain
bound to the exact model name and digest already stored in their snapshot.
Snapshot recovery validates `preset_id`, analysis stage and verification stage
as one product-admitted preset before loading settings, taking a profile lease,
or constructing either runtime. Individually qualified stages cannot be mixed
into a cross-runtime combination the preset registry never offered.

The user may register one explicit existing `.gguf` through the desktop picker.
The app never scans model directories, copies the file or mutates the model
store. Registration opens the final component without following symlinks and
with nonblocking semantics before requiring a regular file, so FIFO, device or
socket substitution cannot stall registration, catalog admission or cache
reuse. It hashes the open descriptor, and stores canonical location, basename,
byte size, SHA-256 and Linux device/inode/size/nanosecond-mtime/nanosecond-ctime
identity in private atomic settings. Identity is checked before and after the
one registration hash. Later catalog and runtime admission require the complete
tuple; replacement or mutation requires explicit re-registration. The selected
open model descriptor is inherited by the child, closing path replacement
between check and load. Registration count, path and label sizes are bounded;
duplicate content digests collapse to one canonical registration.

Settings version two stores GGUF registrations and runtime kind. Version-one
settings load with the same selected Ollama preset and an empty registration
list, then upgrade atomically on the next write. The catalog combines Ollama and
registered-GGUF descriptors; one unavailable discovery source does not hide the
other. Routine presentation exposes runtime kind, basename, size, family,
trained maximum, qualified context and disabled reason, never the absolute
private path. Switching to Ollama clears an idle app-managed direct child;
switching to direct GGUF does not mutate an Ollama runner and remains busy while
an admitted Ollama digest is resident.

A full preset routes analysis and verification to the same model and is offered
only after that exact digest passes the full qualification contract. A hybrid
preset routes selection/paraphrase to the chosen qualified smaller model and
verification to the strongest installed qualified verifier. Routing is based on
the typed request stage, not prompt inspection. Direct synthesis remains local
and deterministic. A hybrid preset cannot route verification to a model with a
lower verifier qualification tier merely because it is smaller or currently
selected. If the required verifier is absent or changed, starting the run fails
before document work rather than silently weakening verification.

#### Immutable run identity and historical behavior

A new run snapshots the preset/profile versions, actual Ollama names and digests,
per-stage qualified contexts and tokenizer versions before the first model call.
Every generated artifact continues to record the actual runtime/model identity
that produced it. Continuation uses the snapshot and refuses a missing or changed
model; it never switches because the global setting changed. A retry run inherits
the source run's snapshot unless an explicitly user-started new run selects the
current preset. Historical runs without a snapshot remain readable under their
stored legacy runtime/model identifiers and compiled historical budgeting rules;
they are not relabeled as native or qualified.

Version-two stage snapshots add a typed runtime kind. A direct snapshot carries
profile ID, immutable GGUF digest, qualified context and tokenizer/runtime
versions, but no mutable path. Reconstruction resolves the current registration
by exact digest and repeats file-identity and complete runtime-manifest admission
before state changes. Version-one snapshots remain Ollama-only and reload under
their historical semantics. An Ollama-only snapshot has no dependency on the
mutable GGUF settings file: missing, malformed or future-version GGUF settings
cannot block its reconstruction. Settings are loaded only when at least one
direct stage needs a registration. A missing direct registration or changed
file never falls back to Ollama or another GGUF.

Connect runtime selection happens before admission, and the selected profile
snapshot is inserted in the same SQLite transaction as ingestion and the
accepted job-to-run mapping. Therefore an accepted Connect job cannot expose a
stable ingested checkpoint without its immutable runtime identity, including if
the process exits before worker scheduling. A production Connect runtime without
a profile snapshot is rejected before the acceptance transaction.
If runtime construction or snapshot admission fails after artifact preparation,
the provider rechecks the job identifier before returning that error. An
identical request that committed concurrently returns the stored idempotent job;
a conflicting request returns the existing job-ID conflict. Cleanup removes only
the losing request's separately allocated import and never the accepted job's
owned source.

Desktop new-run admission likewise requires a runtime profile snapshot and
inserts it in the same SQLite transaction that first persists the document and
run. Starting parsing may follow in its existing transition, but a crash at any
point after admission cannot leave the new run without the selected immutable
profile. Test-only legacy setup may explicitly construct snapshotless historical
checkpoints; the product start path may not.

A failed historical run without an immutable model profile is not retryable.
History does not advertise Retry for that run, desktop retry admission rejects
it before runtime construction or health, and the immediate retry-lineage
transaction repeats the profile-presence check before creating a child. A retry
may never substitute the current global preset for an absent inherited profile;
the user must explicitly start a new run to select the current preset. Current
product runs already persist their profile at admission, so this restriction is
limited to snapshotless historical or test-created state.

The selected global runtime status gates new-document admission only. A history
item that advertises retry or runtime-required continuation remains actionable
when the current global preset is unavailable, because the recovery command
reconstructs and health-checks that run's immutable snapshot independently.
Snapshot preflight still occurs before retry lineage, state or events mutate;
an unavailable historical runtime returns its typed error without consuming the
checkpoint. The frontend must not replace that per-run backend authority with
the readiness of an unrelated current preset.

Model disappearance is a recoverable availability failure at every metadata
lookup. In particular, if `/api/tags` reports the snapshotted name and a later
`/api/show` returns not-found during stage health or tokenizer loading, the
adapter returns retryable `MODEL_NOT_AVAILABLE`, rejects the current work and
accepts no generated bytes. Authentication, malformed metadata, unsupported
architecture/tokenizer and invalid context remain their existing terminal or
typed boundaries; a not-found response does not relax profile matching.

#### Qualification and acceptance evidence

Unit and integration evidence must cover native request projection, schema and
usage parsing; exact stage routing; model-maximum versus qualified-context
boundaries; malformed and stale discovery; loopback and redirect rejection;
atomic setting recovery; immutable continuation; and both sides of every token
admission boundary. Tests prove that a small analysis model cannot become the
verifier through a falsy/default path and that downstream code uses the admitted
profile and counted payload rather than raw settings or character proxies.
Catalog tests supply the same qualified digest under two names in reverse order,
retain both installed descriptors, and require one uniquely identified preset
using the deterministic canonical alias.
Discovery resource tests cover each retained external string at 512 UTF-8 bytes
and 513 bytes, plus descriptor aggregates at 256 KiB and 256 KiB plus one. They
prove the checked descriptor-building path, rather than a detached validator,
owns the values ultimately returned to settings and the webview.
Recovery tests also cover a healthy immutable run snapshot while the selected
global preset is unavailable, plus model removal between tags and metadata
lookup; both paths must preserve the checkpoint and remain retryable.
They also prove that a snapshotless failed historical run is not advertised as
retryable, invokes no runtime factory when retry is attempted, and cannot create
retry lineage through the transactional store boundary.
Streaming tests hold the final chat frame until the provenance request is
observed, prove that output is assembled only after a matching in-flight runner
record, and cover missing, duplicate, mismatched, exactly-256 and 257-record
runner catalogs. They also reject final-only, malformed, mid-stream-error,
missing-final and post-final frames without exposing partial generated text.

Each candidate is run through the same live NARA robustness fixture and DOL
product fixture without changing prompts, validators, timeouts or acceptance
thresholds between models. The record includes exact model name and digest,
preset and stage routing, discovered maximum and qualified contexts, tokenizer
version, claims, evidence, raw and adjusted cited-page fractions, omissions and
withheld claims, per-stage request and token counts, wall time, schema fallback
and failure. Failures lead the report. NARA proves delivery and warning behavior
on an OCR-layer stress document, not table accuracy. DOL must deliver at the
existing product thresholds before a model is eligible for a full preset.
Hybrid qualification additionally requires the selected small model to complete
analysis and the qualified verifier to preserve the same verification contract.

The existing Qwen 3 30B-A3B result is the non-regression baseline. A smaller
candidate may improve footprint or analysis latency, but it does not become a
full preset solely because it loads, returns valid JSON or produces more claims.
Jack and base Qwen 3.8 are reported separately. Qualification is evidence for an
exact digest and profile, not a blanket claim about every quantization or model
carrying the same family name.

Direct Jack qualification used the unchanged prompts, validators, 900-second
timeout, 8,192-token context and corpus gates. With the pinned minimal template,
NARA delivered 8 supported claims from 8 evidence items across 8 of 11
native-text pages in 21 requests and 667 completion tokens; DOL delivered 83
supported claims from 83 evidence items across 83 of 111 pages in 180 requests
and 6,948 completion tokens. Neither run used schema fallback. A controlled
trivial structured-output probe used 3,761 prompt tokens through the embedded
template and 22 through the pinned minimal sequence, so the embedded template
is not qualified.

The qualified runtime manifest is `llama-server`
`0ca399edd758decd825a71823b04ba7ddbc8b2e10d2309d8bf623ee3c2283099`;
`libllama-server-impl.so`
`bd3e91a31fb3c61152043083f1e5008f9b7aef7bf5c168d6b3eca0019f634008`;
`libllama-common.so.0`
`9cf26696816c83fb6e148b3142c1a59bb374de9dc6a92a2966d9abe99939d2a1`;
`libmtmd.so.0`
`ec117a22c9acd24e9eeea4418c93d3d13512bcb607fee97a9674f82b93cb35c7`;
`libllama.so.0`
`c600923b1e548798b80d58b029505dbea4ccd4d2844de38416936e184906f645`;
`libggml.so.0`
`b80a4252c981712564828488b1962e80feb49675ca23da4a8479b2ed7361f86f`;
`libggml-base.so.0`
`c08e63a459d5d0ae4e982d1181fac3c4a0fcb40fcc49d7489ad9e16e4d39ca63`;
`libggml-cpu.so.0`
`d0b746ea2d7e8188236023d4c3a1ba8900a88600e8fdd0f1e6b5043ddf76fcdd`;
and `libggml-cuda.so.0`
`4095bde67d003066a6a622573d165595ad8648470d55317cc94ed4d47acf353f`.
Qualification applies only to these exact model and runtime bytes.

Direct-runtime tests cover settings migration and bounds, duplicate digest
registration, regular/symlink/replaced files, cached registration deletion and
replacement, nonblocking special-file rejection, exact and drifted runtime
manifest members, sealed immutable runtime bytes, GGUF post-lease identity plus
read-lease admission and release boundaries, parent/child lease-signal masks and
thread-directed lease ownership, pre-spawn lease/identity revalidation,
durable-supervisor versus transient-caller parent-death handling,
descriptor-backed library names,
lease-broken/dead-child detection and eviction,
idle-versus-active cache replacement, Ollama-only snapshot recovery with
malformed GGUF settings, shell-free loopback launch, runtime-root
owner/mode/ancestor admission, ambient-temp isolation, private Unix-socket
authentication without TCP handoff, exact alias/context checks,
prompt-token admission on both
sides, control-token injection isolation, truncation, explicit output-limit and
unknown-stop rejection, mismatched accounting and malformed response rejection,
stage snapshot reload, whole-preset snapshot admission, cache identity and
lifecycle, concurrent profile leases,
unavailable-source catalog behavior, and non-mutating admitted Ollama residence
checks, including definitive absent-listener admission and ambiguous transport
failure rejection, missing/malformed resident-digest rejection, active-cache
retention across selection changes, and serialized cancellation-aware
shared-child generation.
The unchanged exactness, provenance, fail-closed, NARA, DOL, clippy and
formatting gates remain required.

#### Explicit non-scope and deployment boundary

This slice does not add non-Qwen providers, cloud inference, arbitrary endpoint
entry, model downloading/copying from the UI, a context slider, automatic
quantization choice, OCR/vision, parser changes, prompt loosening, timeout
increases, coverage reductions or verifier bypass. Ollama model-store relocation,
Ollama GGUF import and installation of the exact qualified llama.cpp runtime
bundle are operator deployment steps. Direct GGUF registration records only an
explicitly selected existing file, never scans arbitrary filesystem paths,
copies model bytes or mutates the Ollama store. Existing models are not deleted
as part of migration or qualification.

The application service composes ingestion, parsing, normalization, structural
interpretation, chunking, analysis, synthesis, verification, and completion.
Thin Tauri commands select concrete adapters, invoke that service, report local
model-runtime readiness, and expose UI-neutral read models for recent runs and
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

Connect, entitlement, packaging, parsing, OCR/vision and customer-visible
summary presentation changes remain outside this runtime slice. The UI
continues to render cited claim cards; optional prose consolidation and
PDF-viewer navigation are separate product decisions. GPU residency and
KV-cache cost at contexts above each profile's qualified value remain separate
deployment evaluations; discovery of a larger model maximum does not qualify a
larger effective context by itself.

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
When pipeline processing fails after acceptance, the public Connect job error
preserves the typed pipeline failure's `recoverable` value; a separate code
allowlist must not silently turn a recoverable model outage into a terminal job.

Job processing calls the same UI-neutral Rust application service as standalone
use. Completed wire output contains plain text, bounded warnings, and the input
artifact ID/media type/size/hash, but no provider-private document/run ID,
database shape, filesystem path, email metadata, or credentials. Provider
output is admitted against the v1 byte/count/field limits before completion.
Connect supplies the delivery policy before analysis; standalone calls do not.
Current runs stop later model work at a whole-claim boundary, while final wire
projection also bounds historical artifacts to a whole-claim prefix. The bounded
result and delivery warning are stored with the completed Connect job; the full
standalone pipeline artifact is never rewritten to satisfy an optional wire
constraint.

This is a same-OS-user possession boundary, not application authentication. A
hostile process running as the same user can read the registration token; a
future trusted broker or OS package identity would be required to change that
threat model. Launch-on-demand, multiple-provider selection, callbacks,
workflow automation, remote execution, and cross-machine discovery are not
implemented.
