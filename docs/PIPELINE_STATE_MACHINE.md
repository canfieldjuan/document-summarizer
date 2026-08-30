# Pipeline State Machine

This defines the canonical sequence and transition rules for document processing.

## State-Machine Invariant
> Persisted pipeline state may only describe work that has actually completed and whose required durable artifacts exist.

All normal durable mutation goes through an expected-state and expected-version
compare-and-set boundary. The state row update and corresponding immutable event
append occur in the same SQLite transaction. Events are ordered by a unique
per-run sequence number; rejected transitions append nothing.

Order of operations:
1. Stable input state
2. Atomically move to the running state and append its event
3. Perform work
4. Validate output
5. Atomically persist artifacts, move to the stable output state, and append its event

## Canonical Linear Flow
```text
RECEIVED
  ↓
INGESTING
  ↓
INGESTED (Durable checkpoint)
  ↓
PARSING
  ↓
PARSED (Durable checkpoint)
  ↓
NORMALIZING
  ↓
NORMALIZED (Durable checkpoint)
  ↓
STRUCTURING
  ↓
STRUCTURED (Durable checkpoint)
  ↓
CHUNKING
  ↓
CHUNKED (Durable checkpoint)
  ↓
ANALYZING
  ↓
ANALYZED (Durable checkpoint)
  ↓
SYNTHESIZING
  ↓
SYNTHESIZED (Durable checkpoint)
  ↓
VERIFYING
  ↓
VERIFIED (Durable checkpoint)
  ↓
COMPLETE / COMPLETE_WITH_WARNINGS
```

## Exceptions & Interruptions

### Cancellation
An application-owned desktop worker may be cancelled from any implemented
active state or durable checkpoint through `VERIFIED`:

```text
active state or durable checkpoint
  -> CANCELLING
  -> CANCELLED
```

The request requires the caller's observed `state_version`. The transition to
`CANCELLING`, version increment, durable `cancellation_requested = true`, and
immutable `cancellation_requested` event are one transaction. A stale request
changes nothing. After that commit, an in-memory token tells the matching worker
to stop at a safe boundary. Worker acknowledgement commits `CANCELLED`, its next
version, completion timestamp, and event atomically. `CANCELLED` is terminal.

Checks occur between stages, around model health/generation calls, per analysis
chunk, and per verification batch. The application does not forcibly terminate
an in-flight parser, deterministic transformation, database transaction, or
Ollama HTTP request. The compare-and-set artifact transaction is the final
guard: if cancellation commits first, unfinished output and its success event
cannot commit. If it commits between a worker finalizer's read and failure
write, that finalization transaction acknowledges `CANCELLED` instead of
recording a conflicting failure. Already-durable checkpoint artifacts remain
unchanged. A stale worker that has lost state/version ownership does not mutate
the newer owner's state.

### Failure
Any active state can transition to `FAILED`. Parser failures, encrypted PDFs,
missing or identity-mismatched sources at parse time, malformed structures, and invalid parser output
persist a structured failure and `PARSING -> FAILED` event. They never create a
parsed artifact or reach `PARSED`.

For parsing, the `INGESTED -> PARSING` transition commits before native parsing
begins. Success validates and persists the parsed artifact in the same
transaction as `PARSING -> PARSED`. If that success transaction fails and the
database remains writable, the run is moved to `FAILED`; if SQLite itself can no
longer persist anything, `PARSING` remains truthful evidence of interrupted work.

For normalization, the persisted parsed artifact is loaded before `PARSED ->
NORMALIZING` commits. Normalization is deterministic and parser-independent.
Success validates document identity, version, page preservation, exact permitted
text cleanup, deterministic block IDs, visual markers, and source provenance.
The normalized artifact and integrity hash commit in the same transaction as
`NORMALIZING -> NORMALIZED` and its event. Invalid parsed input, invalid
normalized output, or a normalizer failure persists `NORMALIZING -> FAILED`
without a normalized artifact. If the success transaction fails while SQLite
remains writable, the run is likewise moved to `FAILED`.

For structural interpretation, the persisted normalized artifact and its
integrity metadata are verified before `NORMALIZED -> STRUCTURING` commits. The
deterministic interpreter references normalized block IDs and source spans; it
does not modify normalized text. Success requires exact page/routing
preservation, valid hierarchy and node identities, and 100% unique normalized
block coverage in source order. The structured artifact and integrity hash
commit in the same transaction as `STRUCTURING -> STRUCTURED`, its state-version
increment, and its event. Invalid normalized input or interpreter output
persists `STRUCTURING -> FAILED` without a structured artifact. Finding no
headings is valid and produces an `Unstructured` node rather than failure.

For chunking, the verified normalized and structured artifacts are both loaded
before `STRUCTURED -> CHUNKING` commits. The deterministic chunker groups only
within top-level structure boundaries, preserves every normalized block and
source span exactly once, and does not invoke a model. Successful artifact
insertion, integrity hash, `CHUNKING -> CHUNKED`, state-version increment, and
event append share one transaction. Invalid inputs or chunker output persist
`CHUNKING -> FAILED` without a chunked artifact. A document with no native text
may reach `CHUNKED` with zero chunks and `NO_TEXT_TO_CHUNK`; later summarization
must fail truthfully unless a future OCR/vision stage supplies text.

For summarization, the verified normalized and chunk artifacts are loaded
before `CHUNKED -> ANALYZING`. Model availability and each structured chunk
response are checked before the analysis artifact commits with `ANALYZING ->
ANALYZED`. Unknown block IDs, non-contiguous quotations, malformed JSON, and
mixed valid/invalid evidence fail the entire response. Evidence source spans
and deterministic IDs come from authoritative Rust state, not model output.

Synthesis loads the persisted analysis artifact, accepts only claims with known
evidence IDs, and deterministically renders page labels from their exact source
spans before committing `SYNTHESIZING -> SYNTHESIZED`. Verification then uses
`ModelRuntime` to classify every claim against only its validated exact
quotations. Rust requires complete, unique claim-ID coverage; restores each
claim's evidence IDs from the persisted synthesis; and rejects malformed,
partial, duplicate, or foreign verdicts. The complete verdict artifact and its
runtime/model identity commit atomically with `VERIFYING -> VERIFIED`.

Only claims classified as `supported` enter the final summary and citation
artifact. Unsupported and ambiguous claims remain in the durable verification
artifact and add `SEMANTIC_CLAIMS_WITHHELD`. This is an evidence-entailment
classification, not certification that the source itself is factually true.
If at least one claim is supported, the final summary artifact, independently
persisted citation artifact, `VERIFIED -> COMPLETE` or
`VERIFIED -> COMPLETE_WITH_WARNINGS` transition, state-version increment, and
event append share one transaction. Warnings select `COMPLETE_WITH_WARNINGS`;
an otherwise warning-free result selects `COMPLETE`.

If no claim is supported, `VERIFYING -> VERIFIED` first preserves all verdicts
for audit, then `VERIFIED -> FAILED` records
`NO_SEMANTICALLY_SUPPORTED_CLAIMS`; no final summary or citation artifact is
created. A failed verdict, summary, citation, transition, or event write cannot
create a false success event. A model, validation, or ordinary artifact-write
failure persists `FAILED` from the active stage. If SQLite itself cannot record
the failure, the active state remains truthful evidence of interrupted work. A
zero-chunk visual-only document fails analysis with
`NO_NATIVE_TEXT_FOR_SUMMARY`; OCR and visual analysis are not implicitly
attempted.

### Connect job mapping

Connect job state is a provider API lifecycle, not a replacement for the
pipeline state machine:

```text
accepted -> processing -> completed | failed
```

`accepted` is committed only when the provider-owned source file exists and the
document, run through `INGESTED`, and job-to-run mapping commit in one SQLite
transaction. A database uniqueness constraint enforces at most one
accepted/processing Connect job. `processing` is a compare-and-set update made
before the existing application service advances the mapped run. `completed`
is written only after the durable summary exists; `failed` carries a bounded
public error and never impersonates pipeline completion.

Connect job rows are status records rather than the immutable pipeline-event
ledger. Their legal payload/state combinations are constrained by SQLite, and
updates use expected current states. On provider startup, an accepted or
processing job left by process interruption becomes `failed` with retryable
`PROVIDER_RESTARTED`. The underlying pipeline run remains authoritative; the
automatic pipeline resume/rollback policy is still deferred.

### Ingestion transaction behavior

Extension/signature validation and source reading occur before durable run
creation. Document insertion, run creation at `RECEIVED`, both ingestion
transitions, and all three history events commit as one transaction. An ordinary
validation/read/database failure therefore leaves no document, run, or event;
it cannot leave a durable run stranded in `INGESTING`.

### Crash recovery status

The desktop acquires single-instance ownership before database setup. It then
reconciles runs left in the implemented active states `INGESTING`, `PARSING`,
`NORMALIZING`, `STRUCTURING`, `CHUNKING`, `ANALYZING`, `SYNTHESIZING`, or
`VERIFYING`. Each active state transitions through the existing expected-state
and expected-version boundary to `FAILED` with a structured, recoverable
`PROCESS_INTERRUPTED` failure and a stage-matched immutable event. A run left
in `CANCELLING` with its durable request marker transitions to `CANCELLED`; an
inconsistent row without that marker is rejected rather than falsely completed.

The full recovery batch is one SQLite transaction. State, version, failure, and
events therefore all commit or all roll back. Stable/terminal runs are ignored,
and repeated reconciliation is idempotent. Recovery is explicit desktop
startup behavior rather than part of `init_db`, so opening another connection
does not mutate pipeline state. It runs before the Connect provider starts and
before Connect performs its separate job-status restart reconciliation.

This is truthful interruption reconciliation, not work replay. It preserves
completed checkpoint artifacts and prior history but does not automatically
rerun parsing or model requests. Explicit stable-checkpoint continuation and
new-run retry remain user actions. Reserved `VISUAL_ANALYZING` recovery remains
deferred because that execution path is not implemented.

### Explicit retry lifecycle

An eligible terminal failure does not transition backward. User-initiated retry
creates a distinct child run instead:

```text
source run: ... -> FAILED               (unchanged forever)
                         \
retry run:               RECEIVED -> INGESTING -> INGESTED -> PARSING -> ...
```

The retry and source share `document_id`, while their `run_id` values, state
versions, and event histories remain independent. Slice 9 reuses only the
durable `INGESTED` checkpoint; parsing and every later stage run normally. The
child creation event has reason `retry_run_created`; its two ingestion events
have reason `retry_checkpoint_reused`; admission to parsing has reason
`retry_processing_started`.

Eligibility requires `FAILED`, `resumable = true`, a structured recoverable
failure from a post-ingestion stage, and no existing direct retry. The caller
supplies the source state version, and SQLite rechecks state/version plus the
one-child constraint in the same immediate transaction that creates the child,
lineage, four events, and active `PARSING` state. A crash before parsing
completes is therefore visible to ordinary active-stage startup recovery
instead of leaving a stable, inaccessible `INGESTED` child. Rejected or stale
calls change neither run.

The parser remains the source-identity boundary. Missing or changed bytes cause
the new child to fail at `PARSING`; the source failure remains unchanged. A
failed child can itself be the source of another explicit retry. Nothing is
retried automatically.

The standalone desktop history is a read-only projection of this persisted
truth. It lists terminal, failed, interrupted, and retry-linked runs without
advancing or repairing them. A contextual retry action invokes the Rust retry
boundary with source run ID and expected version; the frontend does not choose
checkpoints or mutate state. A summary can be opened only when its
integrity-valid artifact exists and the authoritative run is `COMPLETE` or
`COMPLETE_WITH_WARNINGS`. Runtime readiness and frontend display state never
mutate the pipeline state machine or its event ledger.

### Explicit stable-checkpoint continuation

Stable incomplete runs continue on their existing identity:

```text
INGESTED -> PARSING -> ... -> COMPLETE | COMPLETE_WITH_WARNINGS
PARSED -> NORMALIZING -> ... -> COMPLETE | COMPLETE_WITH_WARNINGS
NORMALIZED -> STRUCTURING -> ... -> COMPLETE | COMPLETE_WITH_WARNINGS
STRUCTURED -> CHUNKING -> ... -> COMPLETE | COMPLETE_WITH_WARNINGS
CHUNKED -> ANALYZING -> ... -> COMPLETE | COMPLETE_WITH_WARNINGS
ANALYZED -> SYNTHESIZING -> ... -> COMPLETE | COMPLETE_WITH_WARNINGS
SYNTHESIZED -> VERIFYING -> VERIFIED -> COMPLETE | COMPLETE_WITH_WARNINGS
VERIFIED -> COMPLETE | COMPLETE_WITH_WARNINGS
```

The request includes `run_id` and expected `state_version`. Rust reloads the
authoritative run, derives its checkpoint, rejects stale or non-stable states,
and invokes only the existing forward stage boundaries. No new transition edge
or backward mutation exists. Previously committed events remain the unchanged
prefix of the same run's history, and state versions continue increasing once
per successful transition.

Required checkpoint artifacts are loaded and integrity-checked before the next
active state commits. Corruption or absence therefore leaves the stable state
and event prefix unchanged. Once a stage enters its active state, its existing
success/failure rules apply: successful artifact/state/event writes remain
atomic, and an ordinary stage or persistence failure is recorded as `FAILED`
when SQLite can persist that truth.

Continuation from `INGESTED` through `SYNTHESIZED` requires a model runtime to
eventually analyze, synthesize, or classify claim support. Continuation from
`VERIFIED` does not require a runtime and cannot repeat model calls. Failed runs
never use this path; explicit retry creates a new child run under the preceding
contract. Desktop startup preserves stable checkpoints without replay, and the
UI offers continuation only as an explicit operator action.
