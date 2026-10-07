# Preproduction C9 parity contract

## Pre-production inventory (completed before this accepted revision)

The unchanged original native C9 packet, its runner, settings, templates, token requests and raw results are retained under alias `original-c9`; its manifest SHA256 is `731b18c54a45f34ada58a8964fd0e599eecfcdd4d3a2ae1e6f15a245b01cca4c`. The inventory receipt `preproduction-inventory.json` has SHA256 `56b2715e5b977c7a85c42dca70dac40357f0b612b86734d7901c495ffa485836`.

| Surface | Validated experiment | Published production target | Parity proof |
| --- | --- | --- | --- |
| Cases and verdicts | Frozen original 30; 13 supported, 17 unsupported | Existing replay fixture contains those 30 plus four later controls | Select original execution order; exact input equality and verdict enums; keep adjudication separate |
| Model and settings | Qwen3.5-9B Q4_K_M, seed 7, temperature 0, thinking off, context 32768, output 4096 | Native qualified GGUF or separate gateway/Ollama execution | Freeze weights, effective settings, template, server and response provenance; distinguish backends |
| Native framing | Closed thinking block after assistant opener (archived runner lines 197-204) | `llama_cpp.rs::PromptFraming::load` omits that block | Mandatory zero-generation comparison of both renderings and token arrays on identical frozen cases, then origin correction if different |
| Prompt/schema | Joint four-dimension response; every bounded excerpt | Segment enum; relation-shape alternatives; one dimension per sentence; parent context | Same claims, exact quotations and complete sources through actual `plan`/`classify`; historical projection bisection only on divergence |
| Aggregation | Changed/omitted unsupported, uncertain ambiguous | Also requires positive preservation | Exact verdict comparison; original raw re-score has no all-not-applicable supported case |
| Application gate | Standalone experiment | Candidate enabled only in test binaries | Shared owners exercised in ignored test; ordinary-build gate stays closed |
| Earlier synthesis | Whole contracts, freeform 350-word brief, 1500 output tokens; retained summaries and ratings | General source-cited synthesis is a different task/protocol | Private inventory retained by alias/hash; no complete hashed request/runtime freeze or independent acceptance gold recovered, so no synthesis replay admitted |

The earlier synthesis inventory retains 387 files by alias/hash outside worktrees. Its runner saves summary text and metrics but not complete raw chat responses or rendered token boundaries. Missing historical provenance cannot be reconstructed as fact. This lane records that gap and budgets no synthesis calls.

Standing rule: every future C9 or General contract must open with this inventory: affected validated experiments, settings, runtime, framing, prompts and results; every departure with evidence; and its parity test. Prepare the inventory before writing the contract. A contract without it is not ready for acceptance.

Status: ACCEPTED with the mandatory native-framing amendment, per [operator acceptance](https://github.com/canfieldjuan/document-summarizer/pull/116#issuecomment-6028998115). Acceptance is recorded in a separate contract-only commit before implementation. Implement and run this lane before further faithful-drafting v2 work or C9 behavior changes. PR116 remains held and the ordinary application qualification gate remains closed.

Direction: [PR116 comment 6028832128](https://github.com/canfieldjuan/document-summarizer/pull/116#issuecomment-6028832128). This proposal authorizes a bounded comparison and an origin fix only if that comparison isolates a porting defect. It does not authorize another prompt experiment.

## Root cause and established facts

The missing proof is experiment-to-production parity on unchanged inputs. A particular semantic regression has not yet been isolated. Testing changed production drafts against a changed verifier cannot identify which porting change caused a difference.

The original experiment is recoverable. Its frozen requests, templates, case labels, runner source, raw request/response pairs, execution order and native runtime identities are copied byte-for-byte into `original-c9/`. Credentials are excluded. The inventory records every copied file hash and verifies the original manifest entries and frozen-file hashes. The offline re-score verifies schema membership, relation shapes, response completion and aggregation for every raw output.

Measured baseline: 30 cases, 13 supported and 17 unsupported. All 15 originally faulty claims were rejected, with zero wrong approvals. The operator later adjudicated `equipment-scope-2-faithful` as withhold; `invoice-period-1-faithful` remains unresolved. Preserve both original labels and these later decisions. The latter case's historical unsupported verdict is a parity target, not proof that its semantic label is settled. The other 13 faithful cases must remain supported. Evidence: `baseline-rescore.json`, SHA256 `5aed2bd54f9a684119ab0826fd97186baea64b9c73ff6c08dd5ac9525e52cb97`; `operator-decisions.json` is the unchanged operator record.

Published target: PR116 head `e4a98cf1e57d079cccc6ece3a83bf3abf9107ec2`. Its `summary/comparisons.rs` constructs segment choices, makes four dimension requests per sentence, and aggregates them back to the parent. `plan` and `classify` own this behavior; `summary.rs::classify_verification_batches` routes comparison batches to the classifier. The `candidate_enabled` gate is test-only. This lane exercises those shared implementation owners through an ignored test binary; it does not claim installed-app activation or full-worker qualification.

The original experiment used a joint four-dimension response and all bounded excerpts, native Qwen3.5-9B with seed 7, temperature 0, thinking disabled, a 32768-token context and a 4096-token output allowance. Its native prompt ends with the closed thinking block. At the published target, `llama_cpp.rs::PromptFraming::load` ends at the assistant opener. The current gateway uses a separate Ollama rendering path. Prove that native difference with the mandatory same-input rendering comparison and restore validated framing at its owner if different. Do not attribute a gateway failure to it without a matched comparison.

The later failed drafting candidate and its outputs remain frozen separately. This lane starts from the published target, excluding the failed grouping and drafting candidates.

## Required change surface

1. Create an isolated `codex/` worktree from the pinned published target after acceptance. Add the lane in the existing `summary/comparisons` test module and a local driver under `scripts/`. Reuse the original 30 records already present in `comparisons/fixtures/c9-replay.json`, checking each against the frozen experiment. Its four later diagnostic controls remain distinct; do not silently replace or expand the original set.
2. Add a versioned manifest binding original inputs, labels, prompts, schema bytes and ordering, raw outputs, model/runtime identities, execution order, historical source revision and target revision. Maintain later adjudication as an explicit overlay. Do not alter historical artifacts or promote an unresolved label to faithful/faulty.
3. Construct the production `VerificationPrompt` and owned `CitedClaim` from each frozen case. Assert exact equality of claim text, evidence quotations, full governing clauses and source associations before invoking `comparisons::plan`. Only application identifier translation is allowed, with a recorded bijection. No redrafting, normalization, source narrowing, hand-selected passages or added context. The planner alone determines current segmentation, parent context, source presentation and decoder schema.
4. Run generated batches through `summary.rs::classify_verification_batches` and the real gateway runtime using the current document task protocol. Capture actual requests and raw responses around these owners. Do not reimplement planning, parsing or classification in Python. Replay adapters that transform historical passages into new segments are useful offline diagnostics only; they cannot count as live parity.
5. Add an opt-in command that performs static validation and runtime preflight before any inference, prints its case/call budget, obtains the exclusive inference lock and uses a fresh durable request owner. Freeze gateway deployment/process/profile, GGUF identity, template, effective sampling, context/output limits and source tree. Use seed 7 for these fixed cases, retaining the experiment's greedy/no-thinking settings. Gateway and native backend differences must remain visible in the receipt.
6. Store all requests, responses, completion diagnostics, case mappings, source identity checks, schema/selection validity, exact verdict comparisons, output sizes, observed token counts when available, timings and actual call counts. Check every gateway response against the deployment receipt. A process restart, provenance change, output-limit stop, invalid response or mutated input stops the affected run and cannot pass.
7. Make this a standing acceptance gate for every C9 or General production change. Ordinary CI runs deterministic manifest, scorer, adapter and ordinary-build gate tests. Live qualification is explicitly opt-in on the qualified host. A missing, partial, failed or stale live receipt cannot satisfy the acceptance/merge gate. Bind the receipt to the exact tested head, tracked source hashes, input/label manifest and runtime identities; require a new run for any relevant code, prompt, schema, fixture, dependency or runtime change. Do not expose an application switch that bypasses the existing qualification gate.

## Pass criteria

- Every original case is present exactly once, in the frozen execution order. Missing, duplicate or substituted cases fail.
- All 30 decision enums equal the recorded experiment decisions: 13 supported and 17 unsupported. Report delivery parity separately from exact enum parity; replacing unsupported with ambiguous does not silently pass exact parity.
- Every originally faulty case and the adjudicated equipment case remain withheld; all 13 adjudicated faithful cases are supported. Keep the invoice case's semantic label unresolved while requiring its recorded decision for this parity test. Zero wrong approvals is necessary but cannot conceal lost faithful approvals.
- Each response is complete and satisfies the current owned-passage and relation-shape rules. Report passage containment, relation differences and source relevance separately from decision parity. Do not require identical passage strings across different representations or claim that a matched verdict proves each relation rationale correct.
- No input selection, generation repair, output rewriting or automatic retry is used to obtain a pass. Transport reconciliation must not start a second logical generation; an uncertain outcome stops for inspection.

A full pass establishes parity on this frozen diagnostic corpus. Then compare the remaining public A/B requests with that corpus and report the demonstrated input differences, including synthesized multi-rule claims and missing exclusions. It does not prove every verifier behavior or release the full-worker/unseen gates. v2 remains deferred until this result and its review establish the next scope.

## Divergence investigation and origin fix

Run the published production endpoint first. If it diverges, keep all inputs and expected decisions fixed and investigate the port instead of tuning new instructions.

1. Send the exact historical C9 requests and schemas through the same current gateway as a backend bridge. This uses the full-excerpt grammar unchanged; it is diagnostic and is not the production lane. Preflight every request, including gateway limits. The largest original compact schema measured 153033 bytes; admission must still be established by the actual owner. Do not shrink a rejected schema or raise a cap.
2. Freeze the real request projections at the historical port milestones below. Test only milestones needed to bracket the divergence, always using the complete original set. Reuse already measured endpoints. Do not assume behavior is monotonic: confirm adjacent passing and failing changes before naming the first breaking change. If the observations do not support that attribution, report the unresolved interaction and stop at the budget.
3. Before live inference, compare the original native runner and current `llama_cpp.rs::PromptFraming::load` using the same frozen cases, with zero generation. Save both renderings, token arrays and hashes. If they differ, restore the validated closed-thinking boundary at that owner; add fail-before/pass-after regression evidence and remove the superseded port special case. This required port correction restores demonstrated behavior and is not a new prompt/protocol. Report native framing parity and gateway verdict parity separately. Native live comparison remains conditional on a runtime discrepancy, within its existing 60-call allowance. Native-only evidence cannot establish a gateway remedy.
4. Reproduce and isolate the smallest public failing case at the first responsible component. Explain the wrong assumption and identify whether our port introduced it. Fix that component, remove the superseded port special case, and add a regression that fails before and passes after. Restoring a demonstrated historical behavior is permitted; inventing another prompt, passage protocol or source selector is not.
5. After the origin fix, rerun the complete original set through the corrected production owners. Report before and after on the same inputs. If it still diverges, stop; this contract does not authorize repeated candidate tuning.

Historical projection milestones to inspect:

| Revision | Port surface to isolate | Maximum generation calls for its full set |
| --- | --- | --- |
| `fa93c8ad14f4567c202287ce5d3c211cd56c7ceb` | Initial Rust full-excerpt port | 30 |
| `89b5b738e4519b6eebe51418ed210c9c1ee53c10` | Decoder field ordering | 30 |
| `3e16b954d426461fc2d6ff25c3861a63b587d7d0` | Positive-preservation aggregation | 30 |
| `080197280b03746ce931d0b120c79da2b1aa2304` | Full excerpts replaced by whole segments | 30 |
| `4c8b9dc75f1b38f39eb8a9b681b8c04ff8413da6` | Relation shapes and one dimension per request | 120 |
| `e4a98cf1e57d079cccc6ece3a83bf3abf9107ec2` | Per-sentence planning and parent context | 120, already covered by the first pass |

Changes that only affect admission/capability or the production hold are audited separately; they must not be silently disabled to make an older representation run. Export a historical projection through a minimal test adapter where necessary, record its exact code and compatibility wiring, and verify that the adapter leaves semantic inputs, instructions, ordering and schema unchanged. An inadmissible historical projection is reported as inadmissible, not a semantic failure.

## Inference budget and stop rules

The original 30 claims each contain one terminal sentence. The production planner must confirm this before generation; the expected first pass is 30 cases times four dimensions = **120 calls**. There are no synthesis or source-selection calls.

| Phase | Maximum calls | Condition |
| --- | --- | --- |
| Published production parity | 120 | First live phase after static gates |
| Historical requests on the same gateway | 30 | Production divergence requires backend isolation |
| Historical Rust projections | 240 total | Only required bracketing milestones; no repeated endpoint runs |
| Historical and current native framing pair | 60 total | Only if necessary for a runtime discrepancy |
| Corrected production confirmation | 120 | One proven origin fix |
| Total hard ceiling | **570** | Includes every phase above |

The normal passing path costs 120 calls. Conditional phases are not automatic spending. Print the exact planned count and remaining total before each phase. Static/schema/budget/provenance failures stop before generation; the initial parity phase may collect all case verdict mismatches to identify the divergence pattern, but never continue through an invalid or incomplete response. No retries or extra candidates may exceed these phase caps. If further work needs a larger budget or a changed representation, return with evidence and a revised contract before running it.

## Earlier synthesis experiments

Two earlier whole-contract inputs and saved summaries/ratings are present under the retained summary experiment directory. Their runner used a freeform contract prompt and a separate response budget/configuration; its result entries save metrics and summaries, not a complete hashed request/runtime freeze. Presence-based fact scores are not semantic gold labels. This is different from the completely recovered C9 packet.

The inventory above was completed before this revision. For synthesis artifacts by alias and hash, the retained aliases cover runner and prompt bytes, original inputs, settings, saved summaries and ratings; complete historical raw responses and hashed runtime freeze remain missing. Keep private material under `~/Desktop/doc-classify-corpus/heldout/c9-preproduction-parity/`; only aliases and hashes enter the repository or PR. Add fully evidenced synthesis cases to the same lane registry as a distinct task with its own gate. Do not fabricate missing historical provenance or call historical ratings production acceptance labels. If a complete qualifying freeze cannot be recovered, report the exact gaps; if it can, publish its call budget and acceptance criteria as an amendment before its first live replay. The 570-call ceiling above contains verifier calls only and does not silently authorize synthesis runs.

## Verification plan

Before live work, test the lane against a missing case, duplicate case, changed label, altered source text/context, tampered raw output, wrong runtime receipt, invalid comparison, unsafe approval, faithful false rejection and a stale tested head. Cover both pass and failure directions, partial/mixed inputs and empty/over-budget call plans. Prove the adapter preserves complete inputs and the production owners generate every evaluated decision. Keep exact unsupported/ambiguous distinctions in the scorer.

Run the narrow lane tests, adjacent planner/classifier tests, formatting, strict lint and ordinary-library production-gate tests. Retain the existing boundary tests and preflight the original cases on gateway before running. Do not rerun unrelated broad suites solely to duplicate required CI. Live results, not a passing test process or generated receipt alone, determine parity.

## Explicit non-scope

No faithful-drafting v2 implementation or rerun, new model, new semantic instructions, new evidence protocol, cap increase, label laundering, fuzzy matching, downstream verdict filter, historical evidence rewrite, production activation, merge, deployment or unseen-batch consumption. No changes to Story, Contract extraction, public delivery schemas or storage migrations. PR116's hold remains until its separately required qualifications and operator release decision.

## Implementation summary and cold diff audit

Contract and evidence preparation only. The original C9 packet was copied with hash verification and its raw outputs re-scored offline. No repository code, prompt, model setting or runtime was changed; zero inference calls were made. Operator acceptance and its amendment are recorded here before implementation. Audit the eventual diff against the owners and boundaries above, including every test adapter and any origin fix.

## Gap audit

NOT DONE. Contract accepted; lane implementation, mandatory native framing proof/correction, exact production projection/preflight, live parity and review remain. The causal porting difference is unproven. Earlier synthesis provenance remains an explicitly tracked recovery task. The faithful-drafting v2 lane waits.
