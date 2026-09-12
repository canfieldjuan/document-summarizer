# Run-bound Local Inference Gateway runtime adapter

## Why this slice exists

The merged authenticated gateway client can execute only with a durable `(run_id, stage, ordinal)`
key, but `ModelRuntime::generate` receives only the stage/ordinal request. Desktop and Connect own the
authoritative pipeline run identity immediately before worker execution, yet neither binds that
identity into its runtime. The client therefore cannot participate through the existing pipeline
without global state, prompt smuggling, or a second execution path.

### Problem-derived contract

The root cause is the missing run-context seam between admission and `ModelRuntime`. A correct fix
must:

1. let each accepted desktop or Connect worker bind its authoritative pipeline run ID into its
   newly constructed runtime before any model call;
2. preserve every direct runtime by making run binding an inert default rather than changing its
   generation interface;
3. implement one gateway runtime that refuses unbound generation, opens the application database,
   derives the durable key from the bound run plus typed request stage/ordinal, and delegates to the
   merged authenticated client;
4. expose only stable gateway/task identity through `ModelResponse` and immutable run snapshots;
   backend worker/model identity remains gateway-owned and is not invented by the application;
5. map gateway availability, protocol, credential, storage, expiry, and rejection outcomes into
   typed `ModelRuntimeFailure` values without leaking token, path, prompt, or response content; and
6. preserve current Ollama/llama.cpp selection, snapshot reconstruction, prompts, pipeline state,
   Connect admission/idempotency, and UI behavior.

This slice must not make the gateway selectable in product settings, change defaults, add gateway
configuration persistence, alter the profile-suggestion flow, or claim live appliance/Windows
proof.

## Scope

- Add an inert `ModelRuntime` run-binding hook and invoke it in desktop new-run, retry,
  continuation, and Connect accepted-job worker paths.
- Add `ModelRuntimeKind::InferenceGateway` and a version-three task-profile snapshot whose stable
  fields identify `document.summary.step@1`, its application-side request profile, and its bounded
  context—not the appliance's private model.
- Add a private gateway `ModelRuntime` adapter over the merged client and schema-v17 request ledger.
- Add deterministic executor, desktop, and Connect tests for unbound refusal, exact durable-key
  projection, output/error mapping, immutable task identity, and authoritative run binding.

### Review Contract

Acceptance criteria:

- `ModelRuntime::bind_run` is a default no-op, so all existing direct runtime implementations remain
  source- and behavior-compatible.
- Desktop binds the run returned by each successful new-run/retry/continuation admission before its
  worker can call the runtime; focused tests observe the same persisted run ID.
- Connect binds the run accepted with the job before spawning its worker; the existing HTTP
  acceptance test observes the stored job's exact pipeline run ID.
- Gateway generation before binding returns a typed configuration failure and produces no ledger or
  executor effect.
- Bound gateway generation opens the configured database and passes exactly `(bound run_id,
  request.stage, request.ordinal)` to the authenticated client path.
- The version-three snapshot round-trips through SQLite and reconstructs only as the exact gateway
  task profile; direct version-one/version-two snapshot behavior stays unchanged.
- Successful output reports stable gateway/task identity and the durable ledger retains the real
  deployment/task-policy provenance returned by the gateway.
- Gateway failures preserve the source recoverability decision and expose bounded stable codes
  without private content.

Affected surfaces: `ModelRuntime`, desktop and Connect worker admission, runtime-kind serialization,
gateway adapter/client preflight, immutable run-profile persistence.

Risk areas: binding the wrong run, mutating accepted state before runtime readiness, snapshot
backward compatibility, bypassing the durable ledger, private-data leakage, and duplicated runtime
paths.

## Mechanism

`ModelRuntime` gains a default inert `bind_run(&mut self, run_id)` hook. Runtime instances remain
owned by one accepted worker; desktop and Connect call the hook after the authoritative run exists
and before moving the runtime into that worker. Direct runtimes inherit the no-op. The gateway
adapter stores the bound ID, database path, authenticated client, and immutable task snapshot. Its
`generate` method opens SQLite, constructs the request-ledger key from typed values, and delegates
to `GatewayClient`; it never derives identity from prompts or process-global state.

The gateway snapshot uses the existing persisted shape for backward compatibility, but version
three defines its stage fields as task-profile identity. The digest is over the public application
request profile, not an assertion about whichever private worker/model the appliance selected.

## Intentional

- The adapter exists before product selection so its run/durability contract can be reviewed without
  mixing settings, UI, installer, or deployment decisions into the transport boundary.
- The task profile retains the current 8,192-token application budgeting ceiling. Raising it requires
  independent task qualification; appliance capacity alone is not evidence.
- Blocking in-flight cancellation remains deferred exactly as in the transport contract; the
  existing pre-call `ExecutionControl` check remains intact.
- Pre-run automatic profile suggestion still uses the current direct runtime because it has no
  durable pipeline run identity. Gateway selection cannot land until that separate path has an
  explicit durable request owner.

## Deferred

- Persisted gateway endpoint/token/CA settings, Health presentation, selected-runtime factory
  dispatch, and default migration.
- A durable owner for pre-run automatic profile suggestion.
- Windows owner-ACL validation for gateway credentials and CA files.
- Live appliance, restart, fallback, capacity, installer, and signed Windows proof.
- Removal/deprecation of direct runtimes after accepted cutover evidence.

## Verification

- `cargo test gateway_runtime -q` — 5 passed.
- `cargo test new_desktop_worker_binds_the_admitted_run -q` — 1 passed.
- `cargo test continuation_runtime_factory_receives_the_persisted_run_snapshot -q` — 1 passed.
- `cargo test retry_desktop_worker_binds_the_new_retry_run -q` — 1 passed.
- `cargo test connect_runtime_is_selected_before_job_acceptance -q` — 1 passed.
- `cargo test --all-targets` — 480 library tests passed and 13 ignored; 3 office-acceptance
  tests passed and 3 ignored; 3 release-contract tests passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- `cargo fmt --check` — passed.
- `git diff --check` — passed.

## Estimated diff size

Actual: eight files, 793 additions, and 15 deletions. The over-400 budget is justified by one
indivisible cross-entrypoint seam: the trait hook, both authoritative worker owners, the private
adapter, snapshot compatibility, and deterministic proofs for all four admission paths must land
together or the adapter would be either unreachable or unsafe.
