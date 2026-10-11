# Verify safety-net diagnostic on the persisted A1 draft

## Authorization and inventory first

The operator authorizes this contract-first diagnostic on PR126: one native Verify invocation on A1/attempt-1's persisted draft. Oversight must review this contract and approve the exact frozen Verify request bytes before dispatch. Contract-only commit comes first; private adapter implementation follows contract clearance. No live request is authorized by publication alone.

The production code baseline is 5ae457c97fe48120bf52ce72246cc9cb16ef50df. Durable input freeze alias native-verify-safety-net-20261011/input-freeze.json has SHA256 da619ab858ebcef53b2aa7bedacdb4362e78c4e929531bb671c98cf28fdd07e0. Public-safe input-freeze-receipt.json has SHA256 f8520809dd47fce5439c46a18d07f1a3a073aea2541598a2585da983e2bbd1c7. Original evidence stays immutable; the diagnostic has private copies with directories mode 0700 and files mode 0600 outside worktrees.

| Frozen item | SHA256 |
| --- | --- |
| A1 raw completion content | cdff946f3c5f2a9697a3151b2cebc2bb63423be3b431092eba0650c90eef5aa5 |
| Persisted/reloaded draft | d89e071e23ad4f741b4395e148939aa1947e55efca5f1771ec794d9834e14fe2 |
| Original persisted database | 937d84431eb2b2c1e23f4236cb91013dc84cbaa0103fc54ea3a1ca8c006bcfe1 |
| Normalized source | 564acf578387374ad13e01be8e0b0ea96e823c0706494f68ca1e2866611c2275 |
| Chunked source | 586b54ae7c7df79de253c2237ee380090460ecd12577890036cd14c1d3ff60d2 |
| Persisted Analyze artifact | 57bbb7e7fcc2e0acaf5d546b562a2afa207e3c736b909c158b9c924dd0ef934a |
| Selected runtime snapshot | 7901cbf5aabae2a1a6981474f17892151b7fe3e51c8f52218d8165bd8a484db5 |
| A1 source/runtime freeze | e82b55cb475dfadc7b0898d3adf3d382a58d114f91026d7c7a4c27c5b2f70ed2 |
| A1 runtime observation | e440441f51d865ece29048a11f24ff51e6b6d63f2a2b647f9a824c13a10f0946 |
| Public source fixture | 34aa217d7a21a0ec31abd90d0d7700d42924f334ce107912850398ac3f23d7ea |

The read-only database inspection finds Synthesized state, synthesis attempt 0, four ledger claims, one coherent summary unit, and no completed summary artifact. Its saved draft equals the persisted/reloaded artifact. Retain the original run identity on a disposable database copy; the unchanged seed owner summary.rs:239-253 yields request seed 3980016541271597576 at attempt 0. Do not choose a fresh seed by creating a fresh run.

The proven A1 native setup uses Qwen3.5-9B Q4_K_M, model SHA256 cd76ec205963b3b33350093e6904d9de16c4e666fd104e1f632d25c7f15f2a13, server SHA256 0ca399edd758decd825a71823b04ba7ddbc8b2e10d2309d8bf623ee3c2283099, and the complete library pins in the frozen receipt. Its selected profile is qwen35-9b-q4km-v1, tokenizer qwen35-pre-cc5fb918-v2, context 32768, one slot, GPU layers 999, reasoning off, temperature 0, repaired tokenized message boundaries and closed thinking block, native /completion transport. Retain the server's existing flash-attention default with no override. Reuse the selected runtime factory, framing/tokenizer/schema serializer and native response validation; hash-check actual model/server/libraries before readiness and dispatch.

A1 completed one Synthesize request and persisted this draft. Its independent fidelity review is WITHHOLD, as recorded in [oversight issuecomment-6104405951](https://github.com/canfieldjuan/document-summarizer/pull/126#issuecomment-6104405951), with unchanged criteria a91f1a26. It did not establish live native Verify support. Earlier restored C9 gateway proof used 30 calls and passed its recorded gate (receipt alias c9-preproduction-parity-20261007/gateway-restored/receipt.json, SHA256 54977626b64ee78505f49d26e07b99c2d1e5655bcba8c67fc0eb11bc61e3cd47). That smaller claim corpus does not establish fidelity for this multi-sentence unit. Reuse the existing qualification machinery rather than invent a new verifier.

### Every departure from the proven setup

- Input is the saved A1 draft; there is no new writing or extraction. Invoke Verify and final delivery owners, rather than the Synthesize-only entry point. Verify's existing output allowance is 4096 tokens per request; A1's Synthesize allowance is not a Verify setting.
- The diagnostic uses the qualification unit-test binary. comparisons.rs:33-47 deliberately admits C9 only under cfg(test); ordinary application builds stop with VERIFICATION_NOT_QUALIFIED. Keep that hold unchanged and report this distinction explicitly.
- Use a private database copy, a separate reservation and a private test-only Verify entry point/budget configuration. The original database and spent A1/B1/recovery accounting remain unchanged.
- The durable A1 export differs from tracked code in lib.rs (cfg(test) adapter import), model_settings.rs (snapshot helper visibility), summary.rs (test-only saved-analysis helpers and omitted budget regression test), comparisons/parity.rs and qualification_replay.rs (qualification adapters), llama_cpp_framing_tests.rs (capture/body gate tests), and llama_cpp.rs (building the actual transport request before its capture gate). The latter retains the production payload/framing and sends the same built request; it is the previously reviewed A1 actual-body capture seam. Reuse that seam for Verify, with a new approved ordered body list. No framing or semantic owner change is allowed. Record all adapter differences and their hashes in readiness; no unlisted difference passes.

## Root cause

This is an investigation, not a presumed Verify repair. The independently observed writer defect is S3's invented causal relationship: "ensuring that" makes retention cause the anchor, reversing the governing source relationship. Oversight also withholds S4 and S5; preserve the whole-draft label. The missing evidence is whether unchanged Verify rejects this actual draft. Do not infer a verifier bug or change a model from the writer finding.

Pre-registered expectation: the final summary-unit verdict must not be Supported, because S3 is unfaithful. Unsupported or Ambiguous withholds the whole unit. The expectation is not injected into model input, decoder choices or guard logic. Raw model support and final guarded support are reported separately. If the final unit is Supported, file a product finding with exact requests/responses and the controlling owner; do not tune or repair in this lane.

## Required change surface and code-derived call bounds

Only this contract is committed first. After oversight's contract clearance, add private test-only orchestration to the durable qualification export: open the copied existing database, validate/reload saved artifacts with current owners, collect the unchanged planners' requests, enforce a Verify-only budget, reuse the actual outbound-body gate and detached supervisor, and invoke the public Verify path once. No production semantic edits or new extraction/parser/renderer are required.

| Step and controlling code | Completions | Delegated preflights |
| --- | --- | --- |
| public verify_synthesized_document -> controlled_with_delivery, summary.rs:717-818; start_verification, db.rs:2575 | 0 | 0 |
| ledger planning/classification, summary.rs:1307-1321, 1952-1963, 2160-2220, 1580-1624 | 1 | 0 |
| summary unit planning, coherent/verification.rs:5-61; comparisons.rs:372-438 | 0 | 0 |
| joint four-dimension comparison, comparisons.rs:328-371 | 1 | 1 |
| deterministic fidelity guards, summary.rs:1352; coherent/semantic_support.rs:2142-2194 | 0 | 0 |
| trim contribution path, coherent/trim.rs:154-180 | 0 | 0 |
| short Contract coverage, coherent.rs:5458-5476 | 0 | 0 |
| key points and rendering, summary.rs:1380-1430 | 0 | 0 |
| delivery/persistence, summary.rs:779-835, 837-1008, 3859-3883 | 0 | 0 |
| **Separate invocation ceilings** | **2** | **1** |

The ledger planner greedily batches by claim count and serialized characters. Its owners cap 16 claims and 16000 characters per request, with context-minus-output-minus-512 framing reserve (summary.rs:94-111, 2069-2117). Inspection of this draft's owned ledger projection gives four claims and 3117 system-plus-user characters, hence one batch. This inspection is not a native wire freeze: the real unchanged Rust planner must confirm exactly one ledger batch and one summary batch before launch. Reject a different plan, without increasing the ceiling or adapting the input. Ordinals are 0 for ledger and 1 for the summary; the comparison preflight uses the same summary request and ordinal as its subsequent generation.

The summary planner makes one joint request per existing summary unit, not one request per sentence or dimension. The saved draft has one unit. comparisons::classify performs one preflight and one generation per batch with no retry. The untrimmed artifact has no COHERENT_SUMMARY_UNIT_TRIMMED warning, so the contribution checker schedules no calls; General is excluded from short Contract coverage; version 14.0.0 does not enter Direct key-point generation. Ordinary ledger classification generates once per batch and does not delegate a preflight. Native generation's tokenization/context validation is separate from a delegated preflight and must be recorded separately.

The hard budget is one invocation, at most two Verify completions and one delegated Verify preflight, separate from all existing A1/B1 budgets. Zero Analyze or Synthesize completions or preflights, no binary-search admission, alternative candidate, repair, retry, resume, reset or refund. Count reservations before delegation, failed admissions, actual completion POSTs and completed responses separately. Failed or interrupted invocations remain spent. Reuse the native request timeout and the supervisor's 1800-second wall ceiling, without extending either. Cleanup and independent service-stop receipts are required.

## Explicit non-scope

No prompt, schema, decoder ordering, segmentation, sampler, model, seed policy, context/cap, framing, runtime launch setting, source association, guard, renderer, saved label, API/storage migration, dependency, workflow, release/install, model-host migration or production opt-in changes. No new Analyze/Synthesize generation or replay of responses as live verdicts. No attempt to improve the draft or its label. No A1 rerun, B1 recovery launch, B2 or removal of any PR116 hold. Do not stop another app to free the GPU.

## Assumptions and blockers

- The native/default delivery policy is None, matching verify_synthesized_document and complete_verified_document. Connect's distinct delivery policy is outside this invocation; do not generalize the result to that path.
- The immutable input freeze binds draft, source artifacts, original database/run identity, production code, model/server/library pins and observed settings. Before dispatch, readiness additionally binds the private adapter source, actual Rust plan, logical requests, actual serialized native body bytes for both completions, comparison admission request, launch command, budget owner and every approval consumed by gates.
- Exact Verify wire bytes and the private adapter are NOT PREPARED at this contract-only stage. After contract clearance, prepare them with unchanged production planners and payload/framing owners, without sending /completion. Real tokenizer-only preparation may be necessary; record these HTTP calls separately and permit no delegated Verify preflight or generation during preparation. Approval cannot be based on stub tokenizer bytes or a Python projection. If exact preparation cannot be completed under these limits, stop for oversight; do not bypass a gate.
- Oversight must independently approve the ordered full request/body hashes and the draft/source binding. The prior Synthesize source approval is not a Verify approval. Readiness must bind all approvals actually read by the new adapter; a carried unrelated approval blocks launch.
- Before each send, compare the actual built outbound request body bytes against the approved body for that stage/ordinal. Missing, reordered, duplicate, mutated or extra bodies fail before network dispatch. A logical JSON match or post-response hash is insufficient. Tokenizer requests, health and server startup are observations, not completions; no uncounted /completion request is allowed.
- A failed planner, schema/context admission, malformed/truncated response, provenance error, body-gate mismatch, budget mismatch, occupied GPU or inference lock, or uncertain termination stops the attempt. No result substitutes for missing evidence. Preserve zero-generation failures separately from fidelity conclusions.

## Verification plan and user-visible outcome

1. Before implementation, oversight reviews this exact contract commit. Prepare the private adapter only after clearance; validate the original database remains Synthesized and cloned artifacts reload exactly. Real Rust planning must match the frozen two-request plan. Freeze all bytes and all runtime/adapter/input/approval hashes for separate oversight request/readiness approval.
2. Run zero-generation regression probes against the existing capture/budget/launch owners: correct ordered bodies pass the gate; changed, missing, wrong-stage, swapped, duplicate and extra bodies fail before a completion POST. Verify-only counters reject zero/invalid ceilings, Analyze/Synthesize operations, a third completion and second preflight. Preserve the existing actual-launcher output-folder and signal/cleanup proofs. No mock response is reported as native evidence.
3. Reconcile the separate budget atomically, hold the shared inference lock, confirm free GPU, validate pins and clearance, reserve once and launch once through the proven detached supervisor. Never reset an existing output directory or silently select another model. On a failure stop, report the failure and spend, and retain all artifacts.
4. Capture ledger verdicts verbatim, the summary's raw four comparison dimensions and selected passages verbatim, parsed overall verdict, final verdict after deterministic guards, native finish/truncation/tokens/latency/provenance, all preflight/admission/reservation/completion counts, tokenizer observations, persisted Verify attempt, and final summary or pipeline failure. Trace native/default completion through the real delivery owners when Verify succeeds. Do not call Analyze/Synthesize or the orchestration loop that could repair/retry the draft.
5. In Coherent mode the renderer uses only Supported summary units (summary.rs:1380-1399). It retains or removes the whole unit; it cannot delete just S3. If the sole unit is Unsupported or Ambiguous, default delivery sets require_coverage false when there are no retained summary units (summary.rs:779-790), so no ledger coverage fallback is applied. The public Verify path records the attempt and fails NO_SEMANTICALLY_SUPPORTED_CLAIMS (summary.rs:823-833): no completed summary is delivered. If final Supported survives all owners, record the actual persisted/rendered summary and whether it still contains the unfaithful S3. Unexpected fallback/validation/failure is reported as observed, never replaced with a hypothetical summary.
6. File a product finding if the final unit is Supported, including the actual user-visible outcome and qualification hold. An ordinary application build still rejects this C9 artifact under VERIFICATION_NOT_QUALIFIED. A false approval in the qualification binary is not evidence of a current released user exposure. Stop after reporting; any fix/tuning is a separately accepted scope.

## Implementation summary

Contract-only document and durable input freeze. No adapter implementation, native preparation, preflight or generation has been run. The production Verify owners, source artifacts and independent WITHHOLD label remain unchanged. Required private test adapter and exact byte approvals are explicit pending gates.

## Cold diff audit

The tracked change is only docs/PR-NATIVE-VERIFY-SAFETY-NET.md, which records scope, inventory/departures, code-derived bounds, separate budget, request approval gates, preregistered expectation and user-visible outcome. Durable input copies and freeze receipts are evidence, not model input changes or production edits. A documentation-only commit does not require rerunning code suites; record the no-code-diff check.

## Gap audit

NOT DONE: independent contract review; private adapter and its zero-generation boundary proofs; real Rust request plan and exact native request/body freeze; independent request/readiness approval; the sole native invocation and verbatim result/delivery report. All PR116 holds remain. B1 recovery remains separate and receives no reservation or launch from this authorization.
