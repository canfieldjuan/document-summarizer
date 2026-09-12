# Select the Local Inference Gateway in product settings

## Why this slice exists

The authenticated gateway client, durable request ledger, run-bound adapter, and pre-run request
owner are merged, but no product entrypoint constructs `GatewayRuntime`. Current settings persist
only a qualified direct-model preset, every factory returns `QwenProfileRuntime`, Health identifies
the provider as Local Qwen, and the UI can select only Ollama or llama.cpp. The gateway is therefore
implemented but unreachable.

### Problem-derived contract

The root cause is that persisted runtime selection still models a concrete application-owned model
preset rather than the application's choice between its authenticated gateway task and the retained
direct-runtime fallback. A correct fix must:

1. persist the selected inference source separately from the existing direct preset, migrating
   version-one and version-two settings to their unchanged direct selection;
2. persist only an HTTPS gateway origin and paths to the application credential and issuing-CA
   files—never token or certificate contents—and validate the complete gateway configuration before
   it can become selected;
3. make desktop, automatic-profile, Connect-provider, continuation, retry, and Health factories
   construct the selected gateway with the application database while preserving version-one and
   version-two direct snapshots and reconstructing version-three gateway snapshots independently of
   the current selected source;
4. present one trusted application-owned inference selector and bounded gateway configuration flow,
   without exposing backend worker/model selection or weakening direct-model qualification;
5. report gateway task availability and identity truthfully rather than labeling every runtime as
   Local Qwen; and
6. retain direct Ollama/llama.cpp as an explicit migration fallback until live appliance and release
   proof justify a separate default/removal decision.

This slice must not change gateway protocol, request identity, inference prompts, task policy,
pipeline state, Connect contracts, appliance deployment, worker fallback policy, model promotion,
or release signing.

## Scope

- Upgrade model settings in memory/on next write to include a selected inference source and optional
  bounded gateway connection metadata.
- Add one runtime factory that dispatches selected new work and immutable historical snapshots to
  either the existing direct runtime or `GatewayRuntime`.
- Add commands/UI for selecting direct presets or configuring/selecting the gateway.
- Make Health render the selected provider/task accurately.
- Keep gateway selection unavailable on platforms whose private credential-file ownership boundary
  is not implemented; direct runtimes remain fully available there.

### Review Contract

Acceptance criteria:

- Loading settings versions one or two returns a version-three in-memory direct selection with the
  same preset and GGUF registrations; the next successful write persists version three atomically.
- Gateway selection persists an exact valid HTTPS origin and absolute token/CA paths only after the
  production gateway constructor and authenticated task health check succeed; malformed origins,
  unsafe/missing files, absent task authorization, and unsupported platforms leave prior settings
  unchanged.
- Selecting an admitted direct preset atomically selects the direct source and retains any valid
  gateway configuration for a later explicit switch.
- New desktop, Connect, and automatic-profile work constructs the currently selected source with the
  exact application database path; gateway profile suggestion binds its pre-run owner before model
  work.
- Version-one/version-two run snapshots reconstruct the existing direct runtime, while the exact
  version-three gateway task snapshot reconstructs the gateway using persisted connection metadata
  even after the user selects the direct source.
- The runtime status derives its provider label, runtime ID, model/task ID, and availability from the
  constructed runtime; gateway status never claims Local Qwen or exposes backend worker/model
  identity.
- The UI offers the Local Inference Gateway as an application-owned inference source, collects an
  HTTPS origin plus token/CA files, shows only bounded non-secret connection metadata, and retains
  qualified direct presets as fallback choices.
- Existing gateway lifecycle/idempotency, direct-profile qualification, desktop/Connect binding,
  automatic-profile ownership, pipeline, and release-contract tests remain green.

Affected surfaces: persisted model settings, runtime factory dispatch, desktop and Connect runtime
construction, automatic profile suggestion, runtime Health, and the inference selector UI.

Risk areas: secret disclosure, unsafe credential paths, settings migration, selecting an unavailable
source, immutable-run reconstruction, unbound gateway work, platform credential ownership, and
misleading model/provider labels.

## Mechanism

Settings version three adds a `direct`/`gateway` source discriminator and optional gateway metadata.
Configuration is committed with the existing owner-private atomic writer only after a production
`GatewayRuntime` can be constructed and its credential-scoped `document.summary.step@1` health
check succeeds. The serialized document contains file paths, not file contents.

The shared factory returns a boxed `ModelRuntime`. New work follows the selected source. Snapshot
reconstruction follows the immutable snapshot kind: gateway version three rebuilds the gateway task
adapter from current connection metadata, while direct versions one and two continue through the
existing qualified-runtime admission. Running work keeps its already-constructed runtime.

The existing selector becomes an inference-source selector. A gateway choice uses application-owned
fields and native file pickers; direct choices retain exact qualified preset labels. Health uses the
runtime's stable public identity, so the gateway is reported as an inference service and the direct
fallback remains Local Qwen.

## Intentional

- Existing installations remain on their current direct preset after migration. Making the gateway
  the default requires live appliance/client acceptance and is not inferred from transport code.
- Gateway backend worker/model and vLLM-to-Ollama fallback order are absent from settings and UI;
  those remain gateway-owned policy.
- Gateway connection changes fail closed and do not replace a working selection until authenticated
  task health succeeds.
- Windows gateway selection remains unavailable until the separate owner/DACL validation slice;
  enabling a bearer-token path under size-only validation would violate the merged transport
  contract. This does not block current direct-runtime Windows operation.

## Deferred

- Windows owner/DACL validation for token and CA files, followed by Windows gateway enablement.
- Gateway-as-default migration after live Linux and Windows client acceptance.
- Appliance discovery, certificate automation, credential provisioning, installer integration, and
  administrator UX.
- Live vLLM primary/Ollama fallback, capacity, restart, and multi-client acceptance evidence.
- Removal of direct LM Studio/llama.cpp compatibility after accepted cutover evidence.

## Verification

- `cargo test model_settings -- --nocapture` - passed (20 tests).
- `cargo test gateway_ -- --nocapture` - passed (40 tests).
- `cargo test runtime_status_distinguishes_ready_and_unavailable_without_generating` - passed.
- `cargo test connect_runtime_is_selected_before_job_acceptance` - passed.
- `cargo test new_desktop_worker_binds_the_admitted_run` - passed.
- `cargo test automatic_profile_owner_is_stable_and_bound_before_inference` - passed.
- `cargo test retained_direct_preset_is_not_a_noop_while_gateway_is_selected -- --nocapture` - passed.
- `cargo test --all-targets` - passed (489 library tests with 13 ignored, 3 office
  acceptance tests with 3 ignored, and 3 release-contract tests).
- `cargo clippy --all-targets --all-features -- -D warnings` - passed.
- `cargo fmt --check` - passed.
- `npm run build` - passed after the retained-gateway edit-path review fix.
- `git diff --check` - passed.

## Estimated diff size

Actual after review reconciliation: thirteen files, 908 additions, and 79 deletions. This exceeds
400 because one useful product slice must carry the persisted selection through every real runtime entrypoint,
immutable reconstruction, truthful Health, and the UI; splitting after persistence or factory
dispatch would leave an unreachable or misleading configuration surface.
