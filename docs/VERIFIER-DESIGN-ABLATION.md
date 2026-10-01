# Verifier-design ablation: evidence contract

## Root cause

F3's public probe supplied governing clauses, but its models admitted unsupported payment-stage claims. Existing semantic guards preserved those approvals. This demonstrates insufficient acceptance behavior, not a proven model-capacity ceiling. Production retains supported verdicts at `src-tauri/src/pipeline/summary.rs` in the coherent verification path.

The original probe in `coherent/verification/tests.rs::clause_verification_live_public_fidelity` sends one claim per request. The claim that its misses arose from several claims in one request is contradicted by that code. Production `verification_batches_for_runtime` can batch claims; this experiment will compare that real batching behavior with explicit one-claim requests.

A preliminary, separate diagnostic ran before the new PR comments were read: fourteen claims, three instruction arms, and 9B/14B/27B. It is not this experiment, has no independent human label review, and supplies no pass for the gate below. Its instructions and cases must not be silently substituted here.

## Required change surface

- This contract and `VERIFIER-DESIGN-ABLATION-CASES.json` are the initial evidence-only commit, before harness implementation or inference.
- An ignored local snapshot/driver will build requests using F3's production clause-context builder and identifier mapping, strict production verdict parser, and final semantic guards.
- Snapshot-only condition adapters may vary thinking, diagnostic schemas/instructions, enum order or grouping exactly as listed below. No tracked production implementation changes.
- Reuse the existing inference lock, owned-server checks, model/server/library checksum verification, request capture and cleanup. No new serving infrastructure.

## Explicit non-scope

No production prompt, schema, preset, default, UI, storage, drafting, model promotion or private-document inference changes. PR116 remains held and PR100 frozen. No automatic production PR is authorized by a diagnostic pass. The 14B is excluded from this experiment; it is a different Qwen family.

## Assumptions and blockers

- Cases: thirty public synthetic claims, fifteen faulty and fifteen faithful, across five error classes. The ten original claims and source bytes are preserved. The twenty new claims form ten pairs, changing one governing stage, condition, qualifier or scope element per pair.
- Labels are authored expectations pending independent review. Equipment-scope subsets may be factually true but incomplete as standalone scope summaries. Record factual support and preservation independently.
- Commit and publish the complete case list on PR116. No expanded inference until the independent reviewer checks every label against the source and replies. Any requested case/label correction requires a new pre-inference commit and renewed review; never revise labels after seeing the experiment's results.
- Both models have seen some earlier examples. The ten original claims and the preliminary diagnostic are not held-out evidence. This expanded set is still a small diagnostic, not production qualification.
- Use Qwen3.5-9B and Qwen3.5-27B Q4_K_M blobs already measured. Keep their exact paths/hashes, checksum-verified llama.cpp binary/libraries, seed 7, temperature zero, actual context 32768 and 4096 generated-token cap fixed. The 4096 cap includes reasoning in C1; record truncation as a failed attempt.

## Conditions

| ID | Change from C0 |
|---|---|
| C0 | F3 prompt/schema, thinking off, production batch planner |
| C1 | Thinking on using the packaged model template; preserve raw reasoning and strip only its explicit framing before strict answer parsing |
| C2 | Thinking off; bounded structured assessments of source stage, conditions, qualifiers and scope, then whether the claim preserves them, then verdict |
| C3 | C0, but exactly one claim per request |
| C4 | C0, but enum order is unsupported, ambiguous, supported |
| C2+C3 | C2 assessment with exactly one claim per request |
| C5 | Thinking off; factual-support judgment and condition-preservation judgment separately, then verdict; supported requires both judgments to pass |

Use one predeclared claim order shuffled with seed 7. Never send expected labels, variants, diagnostic case names or rationales to the model. Model-facing IDs remain the production-generated identifiers. Freeze the order and all rendered requests before inference. Freeze C0's production-planned grouping and reuse it in C1/C2/C4/C5; C3 and C2+C3 intentionally split it. Record actual request membership rather than describing C0 as batched without proof. No adaptive rebatching after a failure.

Diagnostic schemas are strict, with complete IDs and bounded output. Verify that C2's assessment appears before its verdict in actual generation. Validate C2/C5 diagnostic output first, project verdicts only after validation, then run the unchanged strict production parser and guards. Inconsistent C5 fields must not turn into an accepted verdict. Retain raw diagnostic judgments, raw projected verdicts and final admission separately.

Thinking-on transport must demonstrably allow reasoning before its JSON answer; a grammar that forbids reasoning does not implement C1. Before live runs, test explicit reasoning framing, malformed/truncated framing, reasoning-only output and a valid final answer. Capture provider total usage and separately measure reasoning tokens with the same runtime tokenizer; if that cannot be measured, report unavailable, never infer it from elapsed time or silently use zero. An unsupported runtime condition is a recorded limitation, not a substituted experiment.

## Verification plan

Before inference:

1. Validate unique case/claim IDs, fifteen pairs, three pairs per error class, fifteen faulty/fifteen faithful claims, original source/claim equality, and paired source equality. Read each new semantic edit; an edit-distance count alone is not a label proof.
2. Obtain the independent label review on the committed case file. Record reviewer comment and case hash.
3. Commit/freeze exact condition schemas/instructions and an immutable request manifest, including actual C0 grouping. Establish schema and parser controls on both valid and invalid outputs, unknown/missing/duplicate IDs, incompatible split judgments, and incomplete reasoning or JSON. No model result is needed for these deterministic controls.
4. Confirm only declared factors change between conditions; source text and claims are unchanged, contexts present, expected labels absent, and packaged templates preserved. Preflight all requests against the real context budget. Failures stay in the denominator.

Inference: every model/condition is run once over all thirty claims. Keep all attempts, including setup/preflight/runtime/parse/truncation failures. The unit for quality counts is the claim, not the batch. If a failed batch cannot yield validated individual verdicts, every claim in that batch remains a failed attempt. Do not count a crash, malformed output or missing verdict as a successful faulty-claim catch. A catch requires a valid unsupported/ambiguous verdict. Faithful parse/runtime failures count as false withholds, with a separate failure breakdown. No selective retry or post-result prompt tuning.

Report raw and final faulty catches/misses, faithful false withholds, failures, and per-class results for every model/condition. Preserve output tokens and wall time per request. For multi-claim requests, report per-verdict amortized token/time costs explicitly as amortized, not individually measured. Report C5's two judgments separately. Record model/source/runtime/template/condition hashes and GPU exclusivity.

## Decision rule

A condition nominates the 9B as a verifier-design candidate if it catches at least 12/15 faulty claims and has at most 1/15 false withholds. Report the same counts for the 27B comparison. The candidate rule is not production qualification and does not lift PR116's hold.

If a 9B condition passes, it supports sensitivity to that verification design on this diagnostic. Propose a separate contract-first production change with fresh independent/real-document checks. If none passes, conclude only that these tested configurations did not meet the rule. A finite failed ablation cannot prove an intrinsic capacity ceiling. The operator may then choose structured extraction or a larger verifier tier, with the same evidence limits.

## Implementation summary

Implemented and froze all seven diagnostic setups at `36c4acd0e3d5962c62ea3ba41327ce12c961158d`, then completed eighty requests and 210 claim judgments on each approved model. The case file and expected answers remain unchanged. The ignored local snapshot records native requests/responses, reasoning tokens, strict parsing and final admission. [Results and evidence receipts](VERIFIER-DESIGN-ABLATION-RESULTS.md) separate reading from narrowing policy. No production behavior changed.

## Cold diff audit

The evidence branch contains this protocol, the unchanged approved public case file, frozen request setups and measured results. The completion update touches this protocol and the result report only. The native-response audit checks all 420 claim mappings, approved source/claim bytes, wire schemas and frozen requests. Production source and the F3 PR branch do not change.

## Gap audit

DONE

The approved experiment, preflight, inference and reporting are complete. Scorer controls and the independent raw-artifact audit pass. Independent review of these results is next; production qualification and PR116 merge readiness remain NOT DONE. The preliminary fourteen-claim diagnostic remains separate. No additional inference or production change follows automatically from a candidate pass.

## Execution clarification after independent review

All thirty labels were approved without changes in PR116 comment 5936227862, and the operator authorized execution on the same case SHA256 b290394ba9a94d3cf707e9053ba64d9105face98ec73cb0da646ddb9eed99a1b. The file's original pending-review metadata is retained to preserve the approved bytes. Any case or answer change requires renewed approval before inference.

Report source-reading classes separately (twelve faulty/twelve faithful) from equipment-scope narrowing-policy compliance (three faulty/three faithful). Also report the original aggregate candidate rule without presenting policy misses as reading failures. Explicitly identify invoice-period-1-faithful as a billing-period/invoice-period synonym control; a rejection is a false withholding, never a caught error.

Production coherent summaries admit at most eight claims. Assign the frozen seed-7 shuffled order to synthetic document groups of at most eight, then use the production batch planner inside each group. C0 grouping is the reference for C1/C2/C4/C5; C3/C2+C3 split it. This avoids pretending one thirty-claim summary is admitted by production. Individual clauses and their claims still come through the production fixture, source-context builder, compact identifiers, parser and guards.

The thinking-on condition may add lazy grammar activation at the explicit closing think token in the existing completion transport. Only C1 opens the packaged thinking prefix; all other conditions retain the packaged thinking-off prefix. Record full wire requests/responses, token IDs and usage. This isolated diagnostic adapter is not a production capability claim. A missing, duplicate or unclosed reasoning boundary cannot become a valid answer.
