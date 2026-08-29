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
The domain state graph reserves `CANCELLING -> CANCELLED`, but no cancellation
command, safe-work-unit coordination, or cancelled-run resume path is
implemented. `CANCELLED` is currently terminal.

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
spans before committing `SYNTHESIZING -> SYNTHESIZED`. Verification checks the
claim/evidence graph, exact quotations, normalized provenance, runtime identity,
ordered source-chunk coverage, deterministic rendered text, and artifact
integrity. It does not claim semantic entailment or factual correctness. Its
artifact commits with `VERIFYING -> VERIFIED` and records
`SEMANTIC_VERIFICATION_DEFERRED`.

The final summary artifact, independently persisted citation artifact,
`VERIFIED -> COMPLETE_WITH_WARNINGS` transition, state-version increment, and
event append share one transaction. A failed summary insert, citation insert,
transition, or event therefore rolls back both artifacts and cannot create a
false completion event. A model, validation, or ordinary artifact-write failure
persists `FAILED` from the active stage. If SQLite itself cannot record the
failure, the active state remains truthful evidence of interrupted work. A
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
`PROCESS_INTERRUPTED` failure and a stage-matched immutable event.

The full recovery batch is one SQLite transaction. State, version, failure, and
events therefore all commit or all roll back. Stable/terminal runs are ignored,
and repeated reconciliation is idempotent. Recovery is explicit desktop
startup behavior rather than part of `init_db`, so opening another connection
does not mutate pipeline state. It runs before the Connect provider starts and
before Connect performs its separate job-status restart reconciliation.

This is truthful interruption reconciliation, not work replay. It preserves
completed checkpoint artifacts and prior history but does not automatically
rerun parsing or model requests. There is still no same-run resume command;
users retry by submitting the document again. Reserved `VISUAL_ANALYZING` and
`CANCELLING` execution recovery remains deferred because those execution paths
are not implemented.

The standalone desktop history is a read-only projection of this persisted
truth. It lists terminal, failed, and interrupted runs without advancing or
repairing them. A summary can be opened only when its integrity-valid artifact
exists and the authoritative run is `COMPLETE` or `COMPLETE_WITH_WARNINGS`.
Runtime readiness and frontend display state never mutate the pipeline state
machine or its event ledger.
