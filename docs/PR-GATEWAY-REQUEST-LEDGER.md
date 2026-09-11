# Durable gateway request ledger

## Why this slice exists

The pipeline's replaceable `ModelRuntime` receives only a stage-local request ordinal. It has no
durable Local Inference Gateway request identity, while the gateway requires one lowercase UUIDv4
to be reused across delivery ambiguity and restart. Constructing a UUID inside an HTTP call would
therefore allow the same model step to be submitted under multiple identities.

### Problem-derived contract

The root cause is missing durable application ownership of gateway request identity and response
receipt. The correct foundation must:

1. allocate and persist one UUIDv4 before the first handoff;
2. bind it uniquely to a pipeline run, inference stage, and request ordinal;
3. reject reuse of that key for a different semantic request;
4. record the exact gateway request digest before submission;
5. durably retain a completed response and producing gateway provenance before acknowledgement;
6. retain acknowledged responses so an application restart never needs to repeat completed model
   work; and
7. migrate existing databases additively without changing pipeline or Connect job records.

This slice must not add HTTP transport, select the gateway in settings, change model prompts,
route profile suggestion, or replace the existing Ollama/llama.cpp runtimes.

## Scope

- Add schema version 17 with a private `model_gateway_requests` ledger.
- Add a storage boundary for reserve, submission, completion, acknowledgement, and reload.
- Enforce immutable identity/request fields and monotonic state transitions.
- Add migration and two-sided idempotency/collision/integrity tests.

### Acceptance criteria

- Reserving the same `(run_id, stage, ordinal)` with the same semantic digest returns the original
  UUIDv4 and expiry after the database is reopened.
- A different semantic digest for an existing key fails without changing the stored identity.
- A gateway request digest is persisted before a request may become submitted and cannot later be
  replaced by a different digest.
- Completion stores media type, content, SHA-256 integrity, deployment ID, and task-policy version;
  reload verifies the content hash.
- Acknowledgement is idempotent, retains the completed response, and cannot skip the completed
  state.
- Migration from schema version 16 preserves existing pipeline rows and creates the complete new
  ledger contract.

## Mechanism

The reservation function uses an immediate SQLite transaction and `INSERT ... ON CONFLICT DO
NOTHING`, then reloads the canonical row and compares its semantic digest. Competing callers thus
converge on the first persisted UUID rather than creating separate identities. Submission,
completion, and acknowledgement use guarded `UPDATE` statements and reload-after-write checks.

The ledger stores hashes and generated model output but not prompts, source documents, gateway
tokens, or model credentials. Model output is already private application data and must be durable
locally before the gateway can be acknowledged safely.

## Intentional

- Pipeline stages are limited to `Analyze`, `Synthesize`, and `Verify`; deterministic stages do not
  use inference.
- Profile suggestion remains direct because it has no admitted pipeline run and therefore no
  durable run identity yet.
- Existing runtime factories remain unchanged. The following transport slice will consume this
  ledger rather than inventing another persistence path.

## Deferred

- Gateway HTTP/TLS client and bearer-token file handling.
- Runtime selection/settings and per-run gateway binding.
- Retry/reconciliation policy for gateway failures.
- Profile-suggestion gateway routing.
- Live appliance proof and UI changes.

## Verification

- `cargo test gateway`
- `cargo test --all-targets -- --skip connect::provider::tests::entitlement_gates_manifest_jobs_and_status_while_registration_stays_owned`
- `cargo test --lib connect::provider::tests::entitlement_gates_manifest_jobs_and_status_while_registration_stays_owned -- --exact --nocapture`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo fmt --check`
- `git diff --check`

The unfiltered parallel full gate currently reproduces a pre-existing test-server race in the
isolated Connect entitlement test above. The exact test passes alone, and the remainder of the
suite passes when that one test is excluded; repairing unrelated Connect test concurrency is not
part of this storage slice.

## Estimated diff size

The final diff is four files with 1,023 additions: one schema migration, one private
store module, module registration, and this contract. The integrity, migration, concurrency, and
two-sided boundary proofs make the overage indivisible; transport and product behavior remain
excluded.
