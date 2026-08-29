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

### Ingestion transaction behavior

Extension/signature validation and source reading occur before durable run
creation. Document insertion, run creation at `RECEIVED`, both ingestion
transitions, and all three history events commit as one transaction. An ordinary
validation/read/database failure therefore leaves no document, run, or event;
it cannot leave a durable run stranded in `INGESTING`.

### Crash recovery status

Automatic active-run detection, rollback, and resume are **not implemented**.
A durable active state in a future long-running stage may truthfully represent
an interrupted process, but this version does not automatically recover it.
Connection reopen preserves the state and history exactly as stored.
