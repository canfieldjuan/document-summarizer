# Accepted offline qualification admission repair

Status: ACCEPTED by the operator in this chat ("I accept"). Contract-only recording before implementation. No model or tokenizer budget is authorized.

## Inventory before change

Retained inventory: native-stage-qualification-contract-20261009/synthesis-after-budget-stop/inventory.json, verified SHA25683774cb1790d6ebdf41cdd777101c2497c851d9c1288a1a837e1362010cdf401. The active source/runtime freeze and saved artifacts below are the executed setup used here. Preserved C9/native diagnostics and saved fresh Analyze artifacts are reused, never regenerated. Active published head dd607bb85580ffa6d7481b50dd0095a8db29b6d0; frozen source c42fa95035dfa3c27496234be49e0e02cac0e36c2fe9cf2a5ec73a7b970fbe59. Qwen3.5-9B Q4_K_M digest cd76ec205963b3b33350093e6904d9de16c4e666fd104e1f632d25c7f15f2a13; server digest0ca399edd758decd825a71823b04ba7ddbc8b2e10d2309d8bf623ee3c2283099; context32768, thinking off, repaired native message boundaries, /completion, existing library pins and prompt/schema/planner/parser/settings unchanged. This inventory claim is backed by the existing freeze and executed request, not a new capacity hypothesis.

The sole replacement spent exactly1 completion after31 delegated preflights. The actual packet/request exactly matched the independent approval. Native stop eos, not truncated,158 generated tokens; the current native owner accepted the response. End-to-end synthesis failed at the harness's downstream Verify preflight. Saved response alias raw-generated-responses SHA256 a4f853ebcb82526c8a4a62ca68da8c3678ecd4f5d9eaad7a2789df132f35b0e2; raw wire response SHA256 c4dda3aa8237b65d89dfe80dbdd24c2e10e390804c436d2dcb185dea7cf6a226. Output fidelity was unreviewed at the native stop. The subsequent independent review labels this same draft WITHHOLD (unfaithful), as recorded below; native success is not full qualification.

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

Operator acceptance now covers this offline admission-policy correction; it is distinct from the preceding lifecycle repair and its now-spent attempt. Any new native qualification remains separately unapproved. Actual native Verify admission remains unmeasured. Independent fidelity review is complete with WITHHOLD (unfaithful), so fidelity clearance failed; a mock admission pass cannot clear either gate.

## Verification plan

Reproduce first: public minimal fixture with a Synthesize completion followed by Verify preflight must fail on the current guard with QUALIFICATION_UNEXPECTED_STAGE. Isolate its one policy owner and introducing commit as above. Change the owner; prove the same fixture and captured-response offline replay now complete/persist/reload without a second generation. Boundary probes: allowed Verify preflight/forbidden Verify generation; forbidden Analyze operations; exactly64 admitted preflights and65th blocked; one generation and second blocked; failed admission/spent stop cannot delegate later; mixed stage sequence and concurrent reservations. Use the existing validated replay and mock budget test helpers. Run the affected tests and applicable formatting checks; no broad CI duplicate or model run.

## Implementation summary / cold diff audit / gap audit

DONE: the two public regressions failed on the old stage policy with QUALIFICATION_UNEXPECTED_STAGE and pass with the repair. The actual captured response also failed offline under the old owner, then passed under the repaired owner through the unchanged Synthesize/parser/persistence/reload APIs. Source packet5b48b3e5c458c768497ee4a7b968b5a2d1c4846dad427770bf951d08d80f9652 and request1bbd41b45b186b66ec305fc8417e20095cf5fc7ab1edc8c1fa3979d6eb6d4d40 remained exact. Replay consumed1 archived response and32 admissions (31 recorded Synthesize decisions plus1 mocked Verify admission);0 model/tokenizer calls. The14 focused budget-owner tests and7 Synthesize-scope tests passed, including the public production-owner regression. Native admission and fidelity are not claimed by this result.

Implementation: src-tauri/tests/support/live_runtime.rs::QualificationBudget::admit_operation is the single operation-aware stage-policy owner used by generation and preflight. It replaces both copied stage prohibitions; no downstream bypass was added. The unit expectation that Verify preflight must be rejected was replaced, with forbidden Verify generation retained. The integration regression in summary.rs is inside its existing test module. PR-NATIVE-PROFILE-INTEGRATION.md now enumerates the post-generation admission path and distinguishes the pre-generation61 estimate from the64 hard total. The original replacement freeze and all spent artifacts are unchanged; their old rules describe historical attempts.

Evidence aliases: minimal-fail-before SHA256 f3b58b3f67e6fff2966c809886091f9441b12b106beb2cfee116d7e7096d0acc; production-owner-fail-before SHA256 21782e6108c69f29169c63c7a4309338365bde8a454d6348efe8730682588f1c; synthesis-scope-pass-after SHA256 760be2a9a7a77239840f5252896f798f43e504abcb673f69a7d8370ce239e750; budget-owner-tests SHA256 9510c7ebdec413ed6ef18ad5492d32cd8dc89887f49cc7cffd7ce218fbef0070; captured-replay-fail-before SHA256 78914ad7fd9048b83a33e1258c486357060001b30e27dcda3a43440423d1af4c; captured-replay-pass-after SHA256 b2673cd0e5832114a7f6c5bbbee54f425a037ffb724ee032cc0482a2b7142198; persisted-reloaded-draft SHA256 d89e071e23ad4f741b4395e148939aa1947e55efca5f1771ec794d9834e14fe2. Private artifacts contain owned source text and stay private.

NOT DONE: actual native Verify admission, independent draft fidelity clearance (review complete, WITHHOLD), full qualification, merge/release or any PR116 hold clearance. No new live run, model or tokenizer call is authorized. Cold diff audit and publication are recorded in the subsequent implementation receipt and PR reply.


## Recorded oversight dispositions at 41c1bdd

[Oversight PR REVIEW](https://github.com/canfieldjuan/document-summarizer/pull/126#issuecomment-6104403941) clears the offline admission-policy repair at 41c1bdd832489d056ffe267a0d7d5cc22a6d07ba, pending CI and the bot's review. The repair permits the required Verify admission, counts it against the unchanged 64-preflight ceiling, keeps Verify generation forbidden, and leaves production unchanged. This disposition does not clear any PR116 hold.

[Independent draft fidelity review](https://github.com/canfieldjuan/document-summarizer/pull/126#issuecomment-6104405951): **WITHHOLD (unfaithful)**. Criteria a91f1a26065c787cc9d9fafb4d8718d1576008d498dfe9532d0f78bc65017830 were applied unchanged to independent-draft-review-packet SHA256 1a7884485710d77e5bc6735f60ae4ab802cb29a0f421191328391cddbd31a9ea.

- S3 reverses the anchor relationship ("ensuring that") and drops "should".
- S4 attributes the fixture's purpose to the subsection rule and drops "exact source provenance".
- S5 drops R6's "should".

The reviewer records S1 and S2 as faithful, and R7 in S5 as faithful. All R1-R7 are present in some form, but R3 and R6 lose their modal force and R4 is partial. E2 is missing; that omission does not change the label. The unchanged criteria require withholding for any single unfaithful item. Structural parse, exact evidence quotes, persistence/reload and native eos are not fidelity clearance.

P3: **non-blocking, no slice**. Verify preflight is admitted even before the Synthesize completion; the 64 cap still bounds it. Record the observation without changing the policy.

Current terminal disposition: offline harness repair complete and independently cleared at the reviewed code head, subject to CI and bot review; fidelity review complete and the fidelity gate failed with WITHHOLD; actual native Verify admission remains unmeasured because it was mocked. The native attempt stays spent and all PR116 holds remain. No new generation, prompt/framing/schema change, or replacement run to improve the label. Any next native step waits for an explicit operator decision.

Documentation-only reconciliation: this section records the existing review dispositions without changing their label or criteria. The earlier 14 budget-owner tests and 7 Synthesize-scope tests remain evidence for the repaired code at 41c1bdd832489d056ffe267a0d7d5cc22a6d07ba; they are not draft fidelity proof. No code suites are rerun for this documentation-only update. Cold diff and no-relevant-code-diff evidence are recorded in the publication receipt.
