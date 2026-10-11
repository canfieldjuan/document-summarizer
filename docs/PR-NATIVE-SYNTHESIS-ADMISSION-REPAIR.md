# Accepted offline qualification admission repair

Status: ACCEPTED by the operator in this chat ("I accept"). Contract-only recording before implementation. No model or tokenizer budget is authorized.

## Inventory before change

Retained inventory: native-stage-qualification-contract-20261009/synthesis-after-budget-stop/inventory.json, verified SHA25683774cb1790d6ebdf41cdd777101c2497c851d9c1288a1a837e1362010cdf401. The active source/runtime freeze and saved artifacts below are the executed setup used here. Preserved C9/native diagnostics and saved fresh Analyze artifacts are reused, never regenerated. Active published head dd607bb85580ffa6d7481b50dd0095a8db29b6d0; frozen source c42fa95035dfa3c27496234be49e0e02cac0e36c2fe9cf2a5ec73a7b970fbe59. Qwen3.5-9B Q4_K_M digest cd76ec205963b3b33350093e6904d9de16c4e666fd104e1f632d25c7f15f2a13; server digest0ca399edd758decd825a71823b04ba7ddbc8b2e10d2309d8bf623ee3c2283099; context32768, thinking off, repaired native message boundaries, /completion, existing library pins and prompt/schema/planner/parser/settings unchanged. This inventory claim is backed by the existing freeze and executed request, not a new capacity hypothesis.

The sole replacement spent exactly1 completion after31 delegated preflights. The actual packet/request exactly matched the independent approval. Native stop eos, not truncated,158 generated tokens; the current native owner accepted the response. End-to-end synthesis failed at the harness's downstream Verify preflight. Saved response alias raw-generated-responses SHA256 a4f853ebcb82526c8a4a62ca68da8c3678ecd4f5d9eaad7a2789df132f35b0e2; raw wire response SHA256 c4dda3aa8237b65d89dfe80dbdd24c2e10e390804c436d2dcb185dea7cf6a226. Output fidelity is unreviewed; native success is not full qualification.

## Root cause

My commit57b0e4e1722c94f44078a9617887719b128e2b5e added the same stage prohibition to generation and admission (src-tauri/tests/support/live_runtime.rs:159,210). Synthesize's existing owner calls coherent_verification_exceeds_runtime_context (coherent.rs:3193) then preflight_request for a Verify request (summary.rs:2054) without generating Verify text. The admission guard rejects that required check as QUALIFICATION_UNEXPECTED_STAGE before delegation. The existing synthesis_only_rejects_other_stages_before_delegation unit test explicitly asserts this wrong behavior for Verify preflight and passed offline. The first incorrect component is the test-only qualification policy, not the summary owner or model.

## Required change surface and differences

- In src-tauri/tests/support/live_runtime.rs, give the shared qualification budget one operation-aware stage policy. Generation remains Synthesize-only with its existing one-completion ceiling. Preflight permits Synthesize and the owning stage's required Verify admission checks; Analyze and other stages remain rejected. Every admitted preflight shares the existing64 ceiling, reservation-before-delegation, error/stop terminality and concurrency owner.
- Replace the unit expectation that Verify preflight must be rejected. Preserve forbidden Verify generation tests and both cap/error directions. Add an integration regression exercising Synthesize's actual downstream admission owner with a public fixture and spy/replay runtime.
- Enumerate the existing post-generation Verify preflight path alongside the pre-generation61 bound. The61 bound applies only before generation;64 remains the hard total limit. Record the actual count for the captured output offline. No claim that every possible generated draft fits64 is added.
- Correct the accepted-scope description in docs/PR-NATIVE-PROFILE-INTEGRATION.md and this proposed amendment to distinguish generation scope from admission scope. Do not retain a second ad hoc admission bypass in private adapters.
- Use the captured native response in a new durable offline replay namespace, through unchanged production Synthesize/parser/persistence/reload owners. Mock only the transport/admission boundary, record all requests, and never call a model/tokenizer/server. This proves the harness-policy repair and exposes the draft for independent fidelity review; it does not prove native admission for unseen Verify requests.

## Explicit non-scope

No production behavior, source association, prompt/framing, schema, sampler, model/runtime pins, capacity/call caps, dependency or UI changes. No new model or tokenizer calls. No replay/resume/reset of any spent invocation; no replacement attempt, merge/install/deploy or PR116 hold clearance. Preserve original source export, manifest, receipt and raw outputs byte-for-byte.

## Assumptions and blockers

Operator acceptance now covers this offline admission-policy correction; it is distinct from the preceding lifecycle repair and its now-spent attempt. Any new native qualification remains separately unapproved. Actual native Verify admission and independent fidelity remain unresolved; a mock admission pass cannot clear them.

## Verification plan

Reproduce first: public minimal fixture with a Synthesize completion followed by Verify preflight must fail on the current guard with QUALIFICATION_UNEXPECTED_STAGE. Isolate its one policy owner and introducing commit as above. Change the owner; prove the same fixture and captured-response offline replay now complete/persist/reload without a second generation. Boundary probes: allowed Verify preflight/forbidden Verify generation; forbidden Analyze operations; exactly64 admitted preflights and65th blocked; one generation and second blocked; failed admission/spent stop cannot delegate later; mixed stage sequence and concurrent reservations. Use the existing validated replay and mock budget test helpers. Run the affected tests and applicable formatting checks; no broad CI duplicate or model run.

## Implementation summary / cold diff audit / gap audit

NOT DONE: no code edited, no repair test or replay implemented, no signoff. DONE: exact live reproduction, origin/code/blame trace, existing public fixture confirming the wrong admission policy, immutable output and complete cleanup receipts. After acceptance, one class fix and one push for this concept; do not remove production admission or special-case the known response.
