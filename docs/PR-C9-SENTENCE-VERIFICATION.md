# Proposed C9 per sentence verification contract

Status: PROPOSED, not accepted. This commit records the contract only. The operator must accept this exact revision before implementation or inference, and that acceptance must be recorded separately before code. Direction to write the contract is not acceptance.

This proposal follows [issuecomment-6020285805](https://github.com/canfieldjuan/document-summarizer/pull/116#issuecomment-6020285805). It supersedes the uncommitted proposal in issuecomment-6019175152, which incorrectly retained the failed trial's replacement instruction. That instruction is excluded here. The failed trial and its evidence remain preserved.

## Root cause

The frozen dimension candidate produced incorrect semantic relations on independently labelled public worker units. The subsequent single instruction trial failed immediately: A1 was labelled withhold, but four structurally valid preserved relations produced Supported. Its parser and aggregation propagated those relations correctly. The observed error starts at the model response; its internal cause is not established.

The relevant request owner is `summary/comparisons.rs::plan`: it creates four dimension requests for each entire coherent claim unit. `classify` executes those requests and `aggregate` derives the verdict. The bounded hypothesis is that verifying each complete sentence separately will expose relationships missed when judging a multi-sentence unit. This contract does not claim that decomposition fixes semantic fidelity.

The implementation base is `3ccf3be8a59c2999c3d1e82e38facad4b7d6ffe4`, a descendant of the frozen dimension candidate `4c8b9dc75f1b38f39eb8a9b681b8c04ff8413da6`. Keep the published production gate. Do not base implementation on, cherry-pick, or activate the failed instruction trial. Preserve the complete original system-prompt composition from `4c8b9dc`: the existing entailment and clause-verification instructions, comparison instruction, requested dimension and definition, and conditional source-framing instruction. Preserve the legacy wording too. No instruction replacement, additions, examples or cleanup is allowed.

## Required change surface

1. **One sentence-boundary owner.** Extract the first sentence-boundary pass of `summary/comparisons.rs::segment_ranges` into a shared helper without changing its decisions. Both the existing evidence segmenter and sentence verifier consume those ranges. Do not duplicate the splitter or use an approximate regular expression. The original segment catalogs, including long-sentence fallback, must remain byte-identical.
2. **Complete sentence units.** Use complete sentence ranges before the existing 240-character sub-clause splitting. Preserve each parent claim, sentence ordinal and exact byte range in a Rust-owned association table. Give sentence claims distinct internal identities so the existing duplicate-claim check remains effective. These are application identities, not model-selected evidence IDs. Preserve all non-whitespace text and reconstruct the original claim with its intervening whitespace; do not drop a difficult sentence or rewrite its wording.
3. **Change the candidate at its origin.** Implement sentence expansion, association validation and parent aggregation in `summary/comparisons.rs::{Prepared,plan,validate_plan,classify}`. Replace the whole-unit planning assumption there; do not keep competing whole-unit and sentence modes or implement a separate test-wrapper planner. Preserve the exact-text enum shape, parser and existing four-dimension aggregation. The existing candidate activation gate still blocks ordinary builds, including all-feature builds; no new opt-in is allowed. Tests and public fixtures belong in the existing comparison test owner and fixture directory. Update only directly affected caller assertions where the changed request count requires it.
4. **Owned evidence and full context.** Every sentence retains its parent's entire cited evidence catalog: the same quotations, complete governing clauses and source framing. Build selectable claim segments only from that sentence using the existing segmenter. Include the complete parent claim in a candidate `parent_claim_context` input field for interpreting cross-sentence references; it is untrusted context, not separately selectable evidence. The system instruction is unchanged. Preflight and report this added projection; do not hide its size or count neighboring claim text as source support.
5. **Aggregate in Rust.** Keep the existing sentence rules: changed or omitted makes Unsupported; otherwise uncertain or no positive preservation makes Ambiguous; otherwise Supported. The parent is Unsupported if any sentence is Unsupported, otherwise Ambiguous if any sentence is Ambiguous, and Supported only if every expected sentence is Supported. An all-not-applicable sentence cannot be rescued by a supported sibling. Missing, duplicate, foreign, invalid or incomplete results fail closed. No model-supplied aggregate or partial delivery of a parent unit is allowed.
6. **Admit the whole document first.** Before its first generation request, validate the complete expansion, count all planned comparison requests, and preflight every projection through the actual runtime. Retain the original full-parent-claim plus unique-source input admission check and the existing sentence-request admission checks. Apply the prompt budget to the actual wire projection, including parent context. Sending one sentence at a time must not bypass the document cap. The classifier returns exactly one original parent claim result with its original evidence IDs, so the public output and persisted result shapes do not change.

## Request count and latency report

`summary.rs::MAX_VERIFICATION_BATCHES` is 64. Four dimension requests per unit give a theoretical ceiling of 16 C9 comparison units. `summary/coherent.rs::MAX_SUMMARY_CLAIMS` separately caps coherent summary units at eight. Analysis, synthesis and ledger verification are additional calls outside the C9 plan count.

With sentence counts S1 through Sn, comparison calls become `4 * sum(Si)`. The unchanged cap permits at most 16 sentence claims across the document. Preserve the existing cap owner; do not reset the budget for each parent, partition an oversized document into separate admitted plans, truncate it or silently raise a limit. Whole-document fallback remains outside this contract under issue #124.

After acceptance and static implementation, but before any live generation, produce a frozen cost report containing:

- Each public A/B unit's exact sentence ranges, sentence count, original call count, proposed call count and total per document.
- Every required worker/control projection's claim/source size, actual prompt size, schema size, native/gateway admission and rejection reason. State explicitly whether all of full A and full B fit existing caps. Check all original control reference passages against the eligible sentence/source catalogs.
- A latency estimate per unit, per document and for the full planned experiment, using recorded `4c8b9dc` baseline request timings. Identify the timing field, sample count and observed spread; separate queue/transport time where the receipt permits. This is an estimate, not measured sentence-run latency. Do not use the failed replacement-prompt trial as the latency baseline. If baseline timings cannot support an estimate, report that limitation before sampling; do not create an extra warm-up or pilot run.
- The exact maximum generation-call count, frozen input/seed/runtime identities, and execution order. Actual per-request, sentence, parent and document elapsed times must be retained during the run and compared with the estimate.

Actual sentence counts and expanded latency are not yet measured. If any required A/B document or control fails static coverage or admission, stop before live sampling and report the blocker. A cap increase or scope reduction requires a separate operator decision; neither is authorized here.

## Explicit non-scope

No prompt revision, evidence-ID selection protocol, free copied quotes, fuzzy matching, verdict rewrite, synthesis/source-selection fix, model/template/thinking/settings change, label change, cap increase, public API or storage change, dependency change, UI change, deployment or merge. No production activation or historical artifact reinterpretation. Issues #123 and #124 are out of scope. No private or unseen documents enter this development experiment. No unrelated tests or formatting change.

## Assumptions and blockers

- Operator acceptance of this committed revision is required. The earlier instruction-trial acceptance does not authorize this experiment.
- The public nine-unit parent labels are fixed. They do not provide independent sentence labels. Report each sentence's text, dimension relations and derived verdict without inventing new gold labels.
- Cross-sentence references must remain interpretable through retained context. Unresolved meaning remains Ambiguous; do not borrow support from another sentence. The context projection is an explicit proposed change and must be included in review and admission proof.
- The production gate and [operator hold](https://github.com/canfieldjuan/document-summarizer/pull/116#discussion_r4196409571) remain. Development success does not establish full-worker delivery or unseen qualification, and only the operator clears the hold.

## Verification plan

1. Record explicit acceptance in a separate contract commit before code. Preserve the frozen candidate, failed trial, public source artifacts and independent labels. Reconstruct baseline requests with the production owners and compare all instruction components with `4c8b9dc`.
2. Prove the shared helper preserves old evidence catalogs and schemas on all existing segment regressions: abbreviations, decimals, Unicode terminals and closers, line breaks, whitespace, long sentences and unsplittable fallback. Add public reconstruction/ownership tests for empty, single, multiple and repeated sentences. Do not merge identical sentences or derive identity solely from their text.
3. Reproduce the parent-coverage failure with a public scripted result where a wrong first, middle or last sentence is hidden by a supported sibling. Before parent aggregation this must fail the expected verdict; afterwards it must withhold the complete parent. Cover uncertain/all-not-applicable siblings, missing/duplicate/foreign results and interruption between dimension or sentence requests. Reuse the production four-dimension parser and aggregation; no parallel semantic parser.
4. Test document admission at 15, 16 and 17 sentences, including uneven distribution across parents. Rejection must precede every generation call. Retain the 4096/4097 combined-input boundary and both runtime schema limits under the actual parent-context projection. Report measured native and gateway maxima. Complete the cost report and static passage-containment pre-check before generation.
5. Run the existing native and gateway grammar probes on the sentence schema projections before semantic sampling. Preserve relation-shape negatives and all unchanged transport boundaries. Check the installed gateway, running process and every completion against the deployment receipt. Capture all raw requests, responses, errors, sizes and exposed provenance with fresh durable ownership.
6. Freeze and run the nine labelled parent units once in A1 through B6 order, sentences in source order and dimensions in their existing order. Use each input's original seed, fixed model and settings. Any invalid response, runtime failure or completed-parent label disagreement stops the entire run immediately. Do not complete more units, rerun, tune or relabel after a failure.
7. Only after that worker pass is clean, run each of the four original controls three times through the same sentence path and original control seed. Require exact parent-verdict parity, owned passage membership and complete containment of the recorded C9 passages within the selected pieces. Stop on the first failure here too. There are no additional worker repetitions or diagnostic model calls in this contract.
8. Require zero wrong approvals. Faithful parent labels require Supported; withhold and needs-more-context cannot be Supported. Report the four earlier false-rejection cases A2, A3, B3 and B5 sentence by sentence, including unchanged failures. B4 remains needs-more-context, not a proven false statement. Report coverage separately from relation validity. Missing/unrun units cannot count as passes, and token counts or finish reasons not exposed by the runtime remain unavailable.
9. Run focused regressions, directly affected callers, formatting/lint and the existing ordinary-library production-gate integration tests under all features. Verify completed historical views remain readable and pending unqualified C9 artifacts remain blocked. CI owns duplicated broad suites. A passing development run permits a proposal for separately accepted activation and delivery proof; it does not authorize activation. Full public A/B worker proof and independent unseen qualification remain separate gates.

## Implementation summary

This change adds only this proposed contract. No sentence-helper extraction, orchestration, scorer, fixture, production change or inference is implemented. The earlier local proposal and failed trial remain immutable evidence.

## Cold diff audit

`docs/PR-C9-SENTENCE-VERIFICATION.md` is the sole intended repository change. It corrects the baseline/instruction choice, defines the sentence owner and parent verdict rule, adds the required cost report and grammar/control gates, and preserves acceptance and release boundaries. Verify a documentation-only diff against `3ccf3be` and that no source, tests, fixtures, dependencies or configuration changed. Code tests are not rerun for this proposal.

## Gap audit

NOT DONE

Implementation remains blocked on operator acceptance of this committed contract. Sentence counts, cost/latency estimates, admission, grammar, development semantics, full-worker proof and unseen qualification remain unproven. Contract preparation is complete only after its committed contents and publication are verified.
