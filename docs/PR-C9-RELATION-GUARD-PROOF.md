# Issue 127: offline relation repair proof

## Contract and status

Accepted revision 2 is frozen in PR-C9-RELATION-GUARD-REPAIR.md; its later acceptance is recorded separately in PR-C9-RELATION-GUARD-ACCEPTANCE.md. One offline candidate starts from PR126 source e12ac4a72330bb6eb000119e77806ecce313d591. This is a separate branch, stacked on codex/native-profile-integration. PR116/PR126, production, merge, and live qualification holds remain. No native/gateway generation, tokenizer or live preflight was used.

The writer finding remains open. A1 remains independently WITHHOLD. This deterministic last line of defense does not explain the model's all-preserved response or repair Synthesize fidelity.

## Reproduce, isolate, explain, fix, prove, prevent regression

1. Public approved matrix: the original guard falsely approved eight wrong claims while preserving every faithful control. Original frozen A1 also remained Supported. Fail-first tests were committed before policy implementation in cd8e23f83e46b63dae93db9eab71ef481e331fb8.
2. The origin is semantic_support.rs::evaluation_relations/evaluative_conclusions_supported and modal_force_supported. Consequences were discarded, source-absent concepts were waived, and only strong claim-side modal predicates were checked. The source-empty waiver came from 05dbb4b1703ef42669373d17204e05d291d7f6ae; the claim-only modal iteration from e6b65540. Both predate this port. Saved validation separately trusted stored support (22ac1e44), permitting stale support to reach real completion.
3. The candidate keeps subject, consequence and polarity for recognized causal relations, and preserves unknown subjects as extraction results rather than erasing them. The modal comparator consumes the unchanged shared predicate/parser owner, checks matching source propositions for lost or weakened force, and retains the existing strengthening checks.
4. Fresh Verify still checks before filtering and persistence. Saved coherent validation checks copies of verdicts with the same guard: prose always, presented fallback ledgers when applicable. A contradictory saved Supported record fails INVALID_VERIFIED_DOCUMENT before completion, without rewriting sealed artifacts.
5. All 16 approved fixtures pass; the frozen original A1 guard replay returns Unsupported. All 30 archived C9 verdicts remain unchanged through the existing planner/parser and new guard adapter. The existing scorer keeps 29 adjudicated labels and one unresolved case unchanged, with 23 passage misses across eight cases remaining diagnostic.
6. The actual CPU completion owner is exercised after persistence and database reopen for coherent prose and ledger fallback. Faithful controls deliver; stale support fails, leaves final artifacts absent, records failure, and preserves sealed verdicts. The causal, modal and saved-reader owner mutations each fail the committed regressions. Existing guard, writer and reader tests remain unchanged and pass: 378 passed, 20 ignored. fmt and strict all-target/all-feature clippy pass.

## Precision cost: not independently cleared

All 60 delivered prose candidates were retained in the inventory; there is no selected convenient subset. Eight are coherent, 52 ledger fallback. Production getters and source validation establish replay provenance. The 26 older coherent-to-ledger records already fail today's delivery-metadata validation before this repair; they still participate in the guard comparison. All 170 initially unmatched standalone records are Structured Contract extraction and remain non-scope.

The same frozen inventory has 29 Supported-to-Unsupported catalog changes, of which 23 were actually presented, across 14 aliases. Six are unused-catalog diagnostics. Twenty-five changes involve evaluation/causal preservation and four modal preservation. These are guard observations, not new independent labels or proof that every withheld claim is wrong. Baseline saved validation admits 34 records; the candidate admits 27, with seven new rejections. Historical already-completed artifacts are not rewritten; otherwise-admitted in-flight stale support fails completion.

Independent oversight must review every changed claim, its exact owned evidence and delivery impact before offline clearance. Private packet alias precision-diff-private-v2.json SHA256 a34d4d66da59530adbfa7e265778a0cdbe1443f4a907d1c925b6b4cbeffc7f4d. Public aggregate receipt alias precision-diff-receipt-v2.json SHA256 c2c85a734a9674effcbec0fb0b637b4e3c1a7e518f00e5dff3bb5bb0786b118d. No precision tolerance or replacement label is invented.

## Removed and retained rules

Removed: subject-only transitive approval, blanket source-empty concept waiver, subject-word absence shortcut, vacuous strong-claim-only modal scan, erased unknown subjects, and the now-unused contains_owned_words/contains_any_word helpers. My initial unpublished whole-claim empty-result check was replaced at extraction because it missed mixed known/unknown clauses; the mixed regression proves that failure before and the final result after.

Retained: existing evaluation concepts/negation, exact actor/action/object matching and its bounded context, existing require-form normalization and modal contractions, strengthening/polarity checks, coordinated adjective-subject handling, and the existing matching strong-source allowance when weak and strong statements coexist. The writer's shared parser, feedback and retry paths are unchanged. No blacklist for ensuring, A1-specific ID/phrase rule, whole-unit flag, or cross-clause word-overlap association was added.

Recognized causal grammar is deliberately limited: explicit ensure/guarantee, a because relation, and the causative Stop event (X stops Y -> X causes Y to stop). Both cause and consequence must match owned text in one recognized relation. Other arbitrary transitive verbs are not treated as causative; a negative boundary test rejects that inference. This is not general English entailment. Modal comparisons retain the existing weak/strong force classes and shared actor/action/object parser; distinctions within one force class are not newly qualified. Plain assertions recognize the owned action and its simple s inflection. Unknown material relations in these recognized forms are withheld.

## Verification incidents and cold audit

Initial saved-test construction hit duplicate-page evidence, a missing candidate span field, and invalid fallback metadata; these were test-setup failures, not product reproductions. The final fixture uses existing frozen legacy quote ownership and canonical ledger identity and separately proves real completion. No existing validator or test was relaxed.

One erroneous include-ignored suite selection ran seven extra harnesses without their required input environment; all stopped before runtime admission. It failed, and is not passing proof. The normal suite and the explicitly authorized CPU A1 replay were run separately afterwards. Clippy initially found three needless borrows; these were corrected before its passing run. No production/runtime setting changed to rescue a test.

Cold diff: semantic_support.rs owns extraction and comparison; summary.rs routes saved validation to that owner plus CPU evidence adapters/tests; coherent.rs additions are cfg(test) exports/fixture construction only; comparisons/parity.rs adds an archived-response guard replay without changing the planner. Contracts, acceptance, approved fixtures and this proof accompany the change. Prompts, schemas, runtime/model/settings, native framing, source association, C9 fixture bytes, versions, fallback algorithms and existing tests are unchanged. Private filename scan reports no match in the added diff; source quotations remain in the private packet.

boundary-probe: missing/unknown identities remain fail-closed; faithful causal paraphrase, changed consequence/direction, missing and mixed unknown relations, evidence joins, polarity, modal omission/weakening, unrelated-source modal and saved/fresh support boundaries are covered.

effect-trace: stale/invented support must not reach delivery | the existing relation owner and completion validator control it | fail-before completion delivered the claim; final coherent/fallback tests reject it and saved-reader mutation restores delivery.

## Gap audit

DONE: accepted contract, origin reproduction, one offline implementation, approved controls, baseline replay, corpus comparison, saved completion proof, mutations, fmt/clippy and cold diff.

NOT DONE: independent review of every precision flip, exact-head PR review/CI, live qualification, production adoption or merge. Stop here for review; no further generation or policy tuning is authorized.

Proof receipt alias offline-candidate-proof-receipt.json SHA256 1b7cf7ce4fe7f1234194b091c5ab32f017e032ba541498254e630085c114a466. Owner-mutation receipt alias owner-mutation-receipt.json SHA256 a0921a3d1153d384fa84590a5a420e973a160e8560c559620ba1e506895b2645.
