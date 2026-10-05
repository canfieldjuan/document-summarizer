# Gateway document task profile negotiation

## Root cause
GatewayRuntime currently fixes task1/context8192 and rejects C9 even when a gateway can supply the new source-passage task. The missing integration is task/capacity negotiation at runtime construction, not a summary, prompt or label change. The gateway now owns the capacity promise (gatewayPR30); the consumer must preserve it through budgeting, requests and saved runs.

## Required change surface
- gateway_client.rs: typed validated task profile; discover credential-scoped task2 from existing health then fetch its authenticated profile; task1 only when2 is absent. An advertised2 with missing/malformed profile is an error, not silent fallback. Select once per runtime. Shared request builder and health consume selected task. V2 capability enables C9; legacy rejects it.
- Profile version1 fixes output4096/schema250000 and the bounded-source-passages feature. Context is read from the server, must be an integer8192..1048576, and is stored in the run snapshot. Unknown/mixed/zero fields fail closed. This range bounds client planning, not an inferred model limit.
- gateway_runtime.rs: derive context, identity, capabilities and canonical snapshot fingerprint from the selected profile. Legacy snapshot remains byte-equivalent. Resume reconstructs the recorded task/context without new negotiation, keeping durable replay identity. V2 health verifies the gateway still advertises the same profile; pre-dispatch capacity enforcement remains gateway-owned.
- Tests: supported/legacy/unavailable discovery, malformed profile boundaries and defaults, no silent downgrade, actual v2 request wire, snapshot persistence/reconstruction/mutation and preserved replay identity. Existing C9/source/label rules remain frozen.

## Explicit non-scope
No model/context configuration, prompt/C9 schema/parser/label tuning, UI redesign, database migration, direct-runtime changes, fallback broadening or unseen tests. No installed deployment or task grant before reviewed server/client integration. PR116 fidelity hold remains.

## Assumptions/blockers
Stacked on PR116 at977eca739637e610f83d394a1ef1c67aed854f54; review independently as the gateway integration slice. GatewayPR30 review/deployment required for full installed-app proof. Existing platform support limits remain.

## Verification plan
Run gateway-client/runtime and model-settings callers plus context/planning tests and strict lint/format. CI owns repeated broad suites. Frozen server-side9B controls already pass the source capacity boundary. Full-app qualification follows reviewed deployment, with the original settings/documents/labels.

## Implementation summary
Implemented at 40024d4e735825a9b572e99a9ac796868aa0fda4. gateway_client.rs owns the selected typed profile and passes it to both preflight and execution. Credential-scoped health chooses whether to fetch v2's strict profile. Missing v2 retains legacy; advertised-invalid v2 fails. gateway_runtime.rs consumes that profile for C9 capability, model identity, context and snapshot digest; saved runs reconstruct the stored profile without network negotiation. The old universal C9 rejection, fixed task-version builder and independent runtime context constant were replaced by the selected profile, not supplemented with another downstream override.

Checks:52 gateway tests,20 model-settings tests,7 coherent-budget tests,21 C9 tests passed (1 opt-in ignored); all-target/all-feature strict Clippy and format passed. Final test cleanup removed a redundant self-comparison; the remaining legacy/v2 identity-collision test passed independently. The only failures in this client implementation were two new fixture timestamps containing fractional seconds; production ledger rejection was correct and fixtures were corrected. No runtime failure was patched downstream.

Evidence alias gateway-task-profile-20261005/summary.json SHA256 7f3858de61b135aecde99acfcad61a2b41d1eb00e28090062ed2b41f4d3ff2fb. Logs and source hashes are retained privately outside worktrees. No broad local duplicate of required CI was run; no installed full-app or unseen inference was attempted.

## Cold diff audit
- gateway_client.rs::GatewayTaskProfile, negotiate_profile, ProfileEnvelope::validate: profile is validated once and carried into request construction; boundary/default/missing-field/duplicate-task tests cover both acceptance and refusal.
- gateway_client.rs::request_core_for_profile and health: selected version reaches outgoing task, semantic request hash and capability; wire/replay tests and drift/unavailability checks pass. No ledger, credential, retry or ACK implementation changed.
- gateway_runtime.rs::new/from_snapshot/canonical_snapshot: new runs negotiate; old runs restore. Task, context and capabilities share the same profile. Legacy canonical text is unchanged. New/old SQLite snapshots round-trip; mutation fails.
- docs/CONTRACTS.md and this contract: scope the previous C9 rejection to v1 and describe v2 without claiming qualification.

boundary-probe: valid context8192..1048576 accepted; adjacent out-of-range/zero/false/empty/missing/mixed profile fields rejected; invalid advertised v2 does not downgrade; changed profile health fails; restored task1 cannot reuse task2 ledger state.
effect-trace: let gateway capacity and capability reach production planning | runtime profile -> context_tokens/supports_response_schema plus shared request builder |32768 runtime snapshot, C9 preflight, task2 wire and durable replay tests pass; actual full-document qualification remains pending.

## Gap audit
NOT DONE for qualification. Client implementation and local checks are complete. Independent review/CI, gatewayPR30 review/deployment/task grant, basePR116 fidelity gate, full-app and unseen qualification remain. No release or deployment is claimed.

## Review correction: suggestion identity

Root cause: my implementation at40024d4 made the request semantic hash depend on
the negotiated task while lib.rs::bind_profile_suggestion_request_owner still
allocated a single owner for all runtime profiles. Unlike run owners, suggestion
owners have no saved runtime. The store correctly refuses a different semantic
request under the old owner. This is an integration regression, not a defective
ledger guard.

Required change surface: ModelRuntime supplies an operation-contract scope;
GatewayRuntime derives it from its existing canonical profile digest; the
Automatic command uses it before owner allocation. See the sole behavioral rule
in [Automatic suggestion request identity](CONTRACTS.md#automatic-suggestion-request-identity).
Remove the unconditional suggestion-contract key in the caller. No downstream
retry, ledger mutation, migration, prompt, label or source-boundary change.

Verification plan: fail-first public-PDF regression through the real parser,
normalizer, suggestion command binding, gateway runtime/client and SQLite ledger
with a fixture transport. Exercise both task transition orders, identical-profile
replay, restoration of a historical task1 owner, and context-profile changes.
Run adjacent suggestion/owner/gateway tests plus format and strict Clippy. CI
owns duplicate broad suites. Independent local review and fidelity remain held.

Nonblocking review notes: new-runtime construction performs authenticated
discovery before health, so an unreachable gateway may now report an unavailable
status without runtime/model identity and status polling adds metadata requests.
The accepted future context range also exceeds the transport message capacity;
the deployed server advertises32768. Transport-aware planning for larger future
profiles is deferred, not claimed by this fix.
