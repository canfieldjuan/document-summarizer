# PROPOSED REVISION 2: source-owned relation guard repair for issue127

Status: amended proposal only. Operator accepted the original contract SHA256 85485cecd1314928f542870d05dab5cc6cb231929e6ee6bfb4a0f3a180384c5a; that acceptance does not cover the new released-app outcome below. Oversight comment6116321794 approved the original12 fixture labels but requires these three amendments before contract acceptance. Operator and independent oversight acceptance of this exact revision remain required before production implementation. This accepted investigation used CPU only. This contract authorizes no native/gateway generation, tokenizer call, live preflight, B1/B2, production qualification, merge or deployment.

## Amendment record and code-grounded product outcome

This revision responds to oversight comment6116321794. It changes contract wording and proof requirements only; no production or test implementation has started.

1. **Released-app effect and in-flight runs.** The existing guard is not conditioned on C9: summary.rs:1352 guards coherent summary claims; :1320 guards ClaimLedgerFallback ledger claims; :3885 guards delivery-coverage ledger fallback. For otherwise admitted prose paths in an ordinary build, matching modal loss/weakening or an invented/reversed/changed causal relation will turn a model-approved claim into a non-Supported claim before filtering. The user can receive fewer claims with the existing SEMANTIC_CLAIMS_WITHHELD warning, the existing ledger fallback where applicable, or NO_SEMANTICALLY_SUPPORTED_CLAIMS when no presented claim remains. No new fallback algorithm, warning format or automatic retry is added. Unknown material relations are withheld too, and their precision cost is measured below.

Completion revalidates saved results (summary.rs:896). A run verified under the old guard but completed after upgrade must fail completion if its saved Supported record contradicts the new policy; it must not silently regenerate, retry, rewrite old evidence or return stale support. The existing persisted failure path records that failure. Historical already-completed artifacts are not rewritten or deleted by this repair, and no retroactive recall is claimed. Tests must exercise the saved-to-completion transition, not merely call the guard directly.

This behavior does not unlock C9. comparisons.rs:24-46 admits the General/clause-coherent candidate only under cfg(test); summary.rs:1299 and :876 enforce that admission before new Verify or completion. Those ordinary-build VERIFICATION_NOT_QUALIFIED outcomes remain unchanged. Structured Contract extraction returns through its separate owner (summary.rs:1292 and :4482); it remains outside this repair. Do not describe the guard as a C9-only change or claim every released coherent run already reaches it.

2. **Precision outside C9.** All existing guard and reader tests must pass unchanged. Before implementing the candidate, inventory and freeze the saved delivered coherent summaries and ledger claims in the existing private heldout corpora, with their exact owned evidence and source/artifact hashes. The inventory identifies duplicate artifacts, excludes undelivered or missing-provenance items with reasons, and reports the corpus coverage; no convenient subset is selected. Run the old and candidate policies over that same frozen inventory at zero generation cost. Report every Supported -> non-Supported change by private alias, claim, previous/new verdict, cause and delivery impact; keep source quotations private. A change is a stated precision cost, not an instruction to tune it away. If evidence needed to replay a delivered item is missing, report it as an untested gap and stop before offline clearance rather than inventing a source association. Independent oversight reviews all flips before clearance. No numeric tolerance or new gold labels are assumed; a further user-facing cost requiring a different policy needs another acceptance.

3. **Writer scope, option (a).** Only the Verify guard and saved-result validation adopt the stricter relation policy. Synthesize's modal_strengthening_feedback behavior, its callers (:4317/:4435/:4555/:4841), prompts, feedback strings, generation, fallback selection and retry counts stay unchanged. The existing shared predicate extraction/normalization remains the parser owner; there is no new writer comparator or duplicated parser. If a shared helper edit would alter writer behavior, stop and narrow the implementation rather than widening the contract silently. Existing writer tests must pass unchanged.

The public fail-first list also includes a causal direction reversal, must -> should weakening, and a faithful is required to <-> must paraphrase. Original12 labels are independently approved. These supplemental minimal fixtures are new review inputs; their exact texts and labels must be approved before they become a regression oracle.

## 1. Inventory first: validated setup and its limits

Reference alias c9-preproduction-parity-20261007/gateway-restored/receipt.json SHA256 54977626b64ee78505f49d26e07b99c2d1e5655bcba8c67fc0eb11bc61e3cd47 binds the restored 30-call run at eec58548a632ade9666fe4347eaccf639817753c. Its raw requests, responses and case receipts were re-hashed against all artifact entries; no mismatch. Existing runner scripts/c9-preproduction-parity.py, fixture c9-replay.json, manifest c9-parity-manifest.json and parity.rs planner/scorer remain the proof owners.

Original direct C9 used Qwen3.5-9B Q4_K_M model cd76ec205963b3b33350093e6904d9de16c4e666fd104e1f632d25c7f15f2a13 and server 0ca399edd758decd825a71823b04ba7ddbc8b2e10d2309d8bf623ee3c2283099, the same eight library hashes as the failed native run. Context32768, one slot, GPU layers999, reasoning off, token-ID /completion, closed thinking block, temperature0, seed7, output4096. Recorded effective generation settings differ from the failed run only in seed (excluding input-dependent grammar). The old direct startup, props, identities and responses bind these statements; no new live observation is claimed.

Recorded results must be distinguished:
- Original direct C9 scores: 30 attempts,15 faulty caught,0 faulty missed,2 originally faithful withheld,0 failures; raw score passed=false, pair gate passed=true. Do not call this 30 independently faithful outputs.
- Restored gateway:30/30 exact verdict parity and30 structurally valid responses;29 adjudicated cases pass,1 unresolved label excluded from semantic scoring. semantic_label_passed=true for the unresolved case is a scorer waiver, not an independent label. Original/adjudicated labels remain immutable here.
- Relations111/120 equal; recorded passage containment227/250. The23 misses across8 cases remain diagnostic, not a new stricter absolute gate.
- Native framing/planner replay makes0 generation calls. It proves request projection, not native fidelity on this whole draft.

Full measured inventory alias c9-guard-origin-20261011/preproduction-and-failure-inventory.json SHA256 d1726568735cbf77e385a3818df3c7bd1316d3ea38834966bf9677f39aef3f56 includes every recorded request-setting difference, code-owner hashes, per-case size metrics, labels and source integration departures.

### Every relevant departure in the failed setup

| Dimension | Validated compact C9 cases | Failed A1 whole-unit request |
| --- | --- | --- |
| Claim | 1 sentence,46-176 characters | 5 sentences,756 characters |
| Source association | 1 quote and1 governing clause | 5 quotes and4 governing clauses; association is per unit |
| Prompt | same3456-character C9 instruction | identical instruction; no causal-link dimension added |
| User payload |455-873 characters |4045 characters |
| Decoder | same v5 shape/ordered fixed relation choices; input-owned catalogs | same shape after catalog redaction; claim3889/source4868 strings |
| Schema bytes |22678-153033 |1007004; actual native body1014482 |
| Context/output |32768/4096 |same |
| Seed/ordinal |7/comparison sequence |original run seed3980016541271597576; server observed246161928; comparison ordinal1 after ledger ordinal0 |
| Transport | original direct C9; restored proof via document-v2 gateway | selected production native factory and managed pinned server |

The failed schema exceeds the gateway profile's250000-byte cap. Same whole-unit gateway admission/parity is not established; do not bypass that limit or call this a capacity finding. Same model/server/libraries and instruction do not mean same semantic input or request sequence.

comparisons.rs, coherent/verification.rs and coherent/semantic_support.rs are byte-identical to the restored baseline. coherent.rs's baseline diff is test recorder/adaptor work; its production guard helpers are unchanged. llama_cpp.rs differs in profile thinking selection, whole role-marker framing, message_delimiters metadata, payload/capture owner and cache-key handling; model_settings.rs differs in admitted9B default/profile, typed metadata/thinking and one native launch-config projection. The inventory and saved exact diffs list these departures. The reviewed A1 capture/budget adapters are further test-only departures, already bound by its freeze and readiness. Do not redesign any of them in this repair.

## 2. Root cause: reproduced guard information loss

The first observed semantic error in the live run is wire-1's preserved relations. The model's reason for that judgment is not isolated by this offline investigation. The absence of an inter-proposition causal dimension is a confirmed instruction property, not proof that adding a dimension fixes it.

The mechanical guard gaps are isolated and reproduced directly through unchanged production functions:
1. EvaluationRelation (semantic_support.rs:1816) retains subject and negation but no consequence. evaluation_relations (:1906) therefore discards the effect of a transitive ensure/guarantee claim. A claim can keep the cause/subject and change the consequence without failing the guard.
2. evaluative_conclusions_supported (:2000) skips a claim concept when the source lacks the same lexical concept (:2017-2019). Commit05dbb4b1703ef42669373d17204e05d291d7f6ae changed that branch from returnfalse to continue. Reversing it blindly would reject the faithful public control that paraphrases an explicit causal assertion without the word ensure.
3. modal_force_supported (:2063) checks only guarded_modal_predicates(claim,true) (:2125). A removed modal creates no claim-side candidate. Commit e6b65540 introduced that claim-only iteration. Existing modal extraction in coherent.rs:6330-6450 and the writer's modal_strengthening_feedback are readers of the same concept; do not create another predicate parser.

These branches predate PR126 and are already present in the restored C9 baseline. They are not a regression introduced by the9B port in this lane. No assertion about the authoring session is made from Git's user name alone.

Observation receipt c9-guard-origin-20261011/guard-observations.json SHA256 726571e4cb6949414c78d76faea04b91925d474d6788488064d842a7a0a3113a records12 proposed public fixtures:5 faithful controls remain Supported;6 of7 wrong claims remain Supported; the existing should-to-must check correctly withholds the seventh. The frozen original A1 claim and its original cited evidence replay to Supported through the same guard. The original12 labels were subsequently independently approved in oversight comment6116321794. The observations are still reproduction evidence, not a passing product-quality test.

## 3. Required change surface and behavior

Implement one source-owned mechanical relation policy, at the existing extraction/validation owners, for the recognized evaluation/causal and modal classes:
- Preserve the complete recognized cause/actor/action/consequence/polarity relationship rather than comparing only a subject. Compare both sides of each matched proposition, including the consequence; do not require a summary to repeat unrelated source facts. Use owned evidence/context; no model-supplied quote, fuzzy span lookup, occurrence-count guess or cross-clause inferred connection.
- Preserve modal force for the same actor-action-object proposition, including a claim that has become a plain assertion. Reuse and consolidate the existing modal predicate normalization/matching. Missing source modality is not an empty claim-side loop that proves safety. An unrelated modal elsewhere is not grounds to withhold the claim.
- Do not equate absence of an ensure token with absence of causal support. Explicit causal assertions expressed without that token must remain eligible; the faithful paraphrase control is mandatory. Do not solve the case with a ban on ensuring, a new keyword list, whole-unit withhold flag, or an A1-specific phrase/ID check.
- When the recognized material relation cannot be established mechanically, report that as an unresolved relation and withhold rather than manufacturing Supported. The candidate must preserve the registered faithful controls and archived adjudicated faithful cases; if it cannot, stop and report the limitation instead of adding fixture-specific exceptions.
- The shared preservation decision owner feeds Verify and saved-result validation only. Writer readers retain their current strengthening-only behavior and existing extraction/normalization owner. Writer feedback, prompt, source selection, generation, retry counts and unit structure remain unchanged. This is a declared last line of defense, not a claimed fix of writer fidelity: the writer finding and WITHHOLD label stay open.
- Fresh Verify must persist the final checked verdict before filtering/delivery. A saved Supported record contradicted by the same relation policy must fail the existing validation path before completion, without mutating sealed historical artifacts or silently regenerating/retrying. Fresh and saved readers use one policy, not a second downstream text scan.

Expected files: coherent/semantic_support.rs (relation representation and comparison), coherent.rs (existing parser/re-export and tests only; writer feedback behavior unchanged), summary.rs (existing saved coherent validation and relevant tests only if needed to route the same decision). Test fixtures/contract documentation accompany the change. comparison planner/parser and runtime files remain unchanged. No unrelated guard or public/storage schema refactor.

## 4. Explicit non-scope

No C9 prompt, dimension, decoder/schema/order, sampler/model/server/framing, context/output cap, passage enumeration or full-clause reader change. No sentence splitting or new per-sentence citation representation. No new generation, GPU reservation, native/gateway call, B1 recovery/B2, loop retry, extra budget, merge, installation or deployment. No re-labeling archived C9 cases, tightening passage containment, or claiming the9B is insufficient. No modifications to any frozen live input, response, driver or approval artifact. Namespace extra-file rejection belongs to the next separate driver freeze, not this semantic repair. Existing ordinary-app VERIFICATION_NOT_QUALIFIED and all PR116 holds remain.

## 5. Assumptions and blockers

- The original12 public expectations were independently approved in oversight comment6116321794; supplemental expectations require independent review before becoming the repair's quality oracle; independent WITHHOLD of A1 remains binding.
- Causal grammar coverage is not proven beyond these recognized mechanically checkable relations. Do not claim general English entailment. The missing model dimension and whole-unit vs single-claim scope remain separate investigation topics.
- Acceptance permits one offline implementation candidate on a separate codex/* branch, not changes pushed onto PR116/PR126. All evidence outside worktrees with700/600 modes. No live qualification is permitted by acceptance of this contract.
- If the candidate needs a prompt/schema, source-association, storage/verification-version or a fallback algorithm, warning format or automatic-retry change to meet the gates, stop for a contract amendment before implementing it. Do not hide such a change in a guard repair.

## 6. Verification plan: reproduce -> isolate -> explain -> fix -> prove -> prevent regression

Before implementation, accept this contract and retain the original approved public labels and independently approve the supplemental public fixture labels. Preserve the exact pre-fix snapshot and raw observations.
1. Commit fail-first public tests for invented causal links, changed consequences, dropped should/may/must, their faithful counterparts, unrelated-source modals, direction reversal, modal weakening and the nonliteral-modal faithful control. On the current owner, wrong claims are Supported; expected failures are the false-approval assertion, not compilation or admission errors.
2. Fix at the shared relation representation/comparison owner. Remove the subject-only transitive shortcut and vacuous claim-only scan made redundant by the shared policy. Preserve the reason for the old source-empty precision waiver by passing the faithful nonliteral causal control. List each retained compatibility rule and why; no stacked special cases.
3. Re-run the same negative/positive tests and the original A1 guard replay. Every registered wrong case is non-Supported; every registered faithful control remains Supported. A1's final unit must be withheld; raw all-preserved model comparisons remain immutable.
4. Reuse existing C9 fixture/manifest/planner/scorer for an offline replay of all30 archived responses. Requests/decoder projections remain identical; all29 adjudicated decisions keep their labels, the unresolved case stays unresolved. Do not claim independent passage perfection; retain23 reference misses across8 cases as diagnostics.
5. Require every existing guard and reader test to pass unchanged. Inventory/freeze all saved delivered coherent summaries and ledger claims in the existing private heldout corpora, and replay both policies offline. Report all Supported -> non-Supported flips and missing-provenance gaps as specified in the amendment record; independent review is required before clearance.
6. Exercise saved result validation and the actual CPU-only delivery owner with a stale Supported record and a faithful Supported control. Stale contradictory support must not deliver; faithful delivery must remain valid. No runtime.generate or repair/orchestration loop.
7. Run the directly affected guard/reader tests, fmt and clippy as required by the implementation contract. Boundary-proof partial/mixed evidence, unknown IDs, empty relation extraction, cause/effect transfer, repeated subjects, negation and modal omission; preserve existing identity/ownership fail-closed behavior. Deliberately undo each policy fix in a scratch copy and show the regressions fail at that owner.
8. Report one consolidation diff, exact tested head, before/after probes, earlier introducing commits and retained/removed rules. Stop after offline proof and independent review. Any live re-qualification gets its own frozen request contract and operator decision.

Model/preflight/tokenizer budget for this contract:0. This is not live fidelity clearance, and no PR hold is released.

## 7. Implementation summary (current investigation only)

Production untouched. The private snapshot contains173 tracked source files,3 unchanged existing frontend build assets and4 Tauri-generated build schema files (180 files total, all hashed in the investigation receipt). Only the guard file has an appended test observation module; its original bytes remain unchanged. The first build failed on missing assets/import, both corrected privately; no probe ran during that failed build. CPU command ran4 tests successfully: observer, existing faithful-control guard test, original30-case planner projection and existing native-baseline scorer. These pass as investigation/replay checks while the product reproduction remains wrong. No implementation fix or actual model call was made.

## 8. Cold diff audit

The guard's original file bytes are an unchanged prefix of the private probe file; all other tracked files match the source-before-probe freeze. Probe fixtures and observer code contain public minimal strings, not an A1-specific fix. CPU output reports observed wrong support separately from proposed expectations. Full baseline/source/request differences and artifact hashes are inventoried. Current tracked checkout stays clean.

## 9. Gap audit

DONE: offline inventory, code trace,12 public observations, frozen A1 guard replay, reused C9 offline projection/scorer and this proposal.

NOT DONE: amended operator/oversight contract acceptance and supplemental fixture labels; wider heldout inventory/precision replay; production implementation; fail-first/pass-after/mutation regression proof; saved-result/delivery regression; fresh live qualification. #127 remains open. Causal-model misclassification is not fixed or explained away by a guard proposal.
