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
Pending.

## Cold diff audit
Pending.

## Gap audit
NOT DONE: negotiation, requests, snapshots, tests and full-app proof remain.
