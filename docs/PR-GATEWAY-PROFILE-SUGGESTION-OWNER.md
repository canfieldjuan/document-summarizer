# Durable request ownership for automatic profile suggestion

## Why this slice exists

Automatic profile selection performs model inference before a document is admitted as a pipeline
run. The merged inference-gateway ledger, however, requires every request key to reference a
`pipeline_runs(run_id)` row. Selecting the gateway in a later settings slice would therefore make
automatic profile selection fail before summarization starts, even though normal pipeline workers
already bind their durable run IDs correctly.

### Problem-derived contract

The root cause is that durable inference identity is modeled as pipeline-run identity even though
the application has a legitimate pre-run inference operation. A correct fix must:

1. introduce one durable request-owner namespace that can represent both existing pipeline runs and
   pre-run profile suggestions without inventing placeholder pipeline history;
2. migrate every schema-v17 gateway request to the corresponding pipeline-run owner without
   changing request identity, state, completion, acknowledgement, or provenance;
3. allocate profile-suggestion owners transactionally from the exact source content hash and an
   explicit version of the prompt/schema/sampling contract, so repeats and concurrent callers reuse
   one identity while contract or content changes receive a new identity;
4. bind that owner before profile-suggestion preflight or generation, including the expanded retry,
   while retaining inert binding for direct runtimes;
5. keep pipeline workers bound to their authoritative run IDs through the same generalized runtime
   hook; and
6. preserve automatic-selection output, normal pipeline history, direct runtime behavior, source
   privacy, Connect behavior, and current product runtime selection.

This slice must not create a second gateway ledger, create a provisional pipeline run, persist raw
document content, change profile-selection semantics, expose gateway settings/UI, or select the
gateway as the product default.

## Scope

- Add schema version 18 with immutable model-request owners and durable profile-suggestion owner
  mappings.
- Rebuild the gateway ledger over owner identity and migrate existing run-bound rows exactly.
- Generalize the runtime binding/key names from run identity to request-owner identity while keeping
  existing pipeline-run callers source-compatible.
- Allocate and bind the stable profile-suggestion owner before the existing inference call.
- Add migration, concurrency, boundary, and command-path proofs.

### Review Contract

Acceptance criteria:

- Migrating a schema-v17 database preserves every gateway request field while changing only its
  foreign-key target from the matching pipeline run to the matching immutable request owner.
- Initialization and every later `pipeline_runs` insert create exactly one `pipeline_run` owner with
  the same ID, and schema validation rejects missing or misattributed owners.
- The profile-suggestion owner store returns one stable owner for the same valid lowercase SHA-256
  content hash and suggestion-contract version across reopen and concurrent calls; a changed hash or
  version returns a different owner.
- Invalid hashes, empty/oversized contract versions, nonexistent owners, and owner-kind mismatches
  fail closed at the storage boundary.
- `ModelRuntime::bind_run` remains available to existing workers and delegates to a default inert
  `bind_request_owner`; the gateway adapter keys requests with the exact bound owner.
- The Tauri automatic-profile command creates or loads its owner from the prepared document's exact
  content hash, binds it before preflight/generation, and uses the same owner for request ordinals
  zero and one.
- Existing profile-suggestion classification/output tests, gateway lifecycle tests, desktop/Connect
  run-binding tests, schema migrations, and version-one/version-two model snapshots remain green.

Affected surfaces: SQLite migration and invariants, gateway ledger key vocabulary, runtime binding,
automatic profile-suggestion command admission.

Risk areas: migration data loss, orphan owners, mutable idempotency scope, content-hash validation,
cross-process races, binding the wrong owner, pipeline-history pollution, and request duplication.

## Mechanism

Schema version 18 adds `model_request_owners`, whose rows identify either an existing pipeline run
or a pre-run profile suggestion. Existing and future pipeline runs use their run ID as owner ID;
profile suggestions use a generated UUID recorded in `profile_suggestion_requests` under a unique
`(source_content_hash, task_contract_version)` key. An immediate SQLite transaction serializes
get-or-create, so competing processes converge before either can submit inference.

The migration backfills pipeline-run owners, installs an insert trigger for future runs, rebuilds
`model_gateway_requests` with `owner_id` as its primary/foreign-key component, and copies all
existing fields. Owner and suggestion identity rows are immutable. Validation checks columns,
triggers, owner coverage, kind attribution, and foreign keys on every database open.

`ModelRuntime` gains a generalized inert `bind_request_owner` hook; its existing `bind_run` default
delegates to it. The gateway adapter stores that owner and constructs the same durable
stage/ordinal key. The profile-suggestion command prepares the PDF, obtains the stable owner using
the versioned classifier contract, binds it, and then runs the existing health/classification path.

## Intentional

- Pipeline-run owner IDs remain equal to run IDs, preserving all existing gateway request keys and
  avoiding a compatibility translation layer.
- Suggestion ownership is created for direct runtimes too. Identity belongs to the application
  operation, not the selected backend, and this makes a later gateway selection atomic rather than
  conditional on runtime-specific type inspection.
- Reopening the same content under the same classifier contract may reuse an acknowledged gateway
  completion. Changing prompts, schema, sampling, seeds, or output semantics requires incrementing
  the suggestion-contract version.
- No suggestion result is added to pipeline history. The durable rows contain only hashes, contract
  identity, timestamps, and gateway protocol state—not source text.

## Deferred

- Persisted gateway endpoint/token/CA settings, Health presentation, runtime factory selection, and
  default migration.
- Live gateway deployment, restart/fallback/capacity evidence, installer work, and signed Windows
  proof.
- Cleanup/retention policy for acknowledged pre-run request owners; current data is bounded to one
  metadata row per unique content/contract pair and must not be deleted before a reviewed replay
  policy exists.
- Removal of direct LM Studio/llama.cpp runtimes after accepted gateway cutover evidence.

## Verification

- `cargo test request_owner -- --nocapture` — 5 passed.
- `cargo test gateway_ -- --nocapture` — 38 passed.
- `cargo test profile_suggestion -- --nocapture` — 9 passed and 1 live-Ollama test ignored.
- `cargo test automatic_profile_owner_is_stable_and_bound_before_inference -- --nocapture` — 1
  passed.
- `cargo test --all-targets` — 486 library tests passed and 13 ignored; 3 office tests passed
  and 3 ignored; 3 release-contract tests passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- `cargo fmt --check` — passed.
- `git diff --check` — passed.

## Estimated diff size

Actual: nine files, 966 additions and 39 deletions. The over-400 budget is justified
because the transactional migration, generalized owner boundary, the first real pre-run consumer,
and their recovery/concurrency proofs must land together; splitting them would leave either unused
schema or a selectable runtime path that can create unowned work.
