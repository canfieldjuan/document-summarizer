# Proposed evidence-local permission qualifier contract — PR128

Status: PROPOSED. Contract and fixture review only. No implementation or acceptance is recorded here. Oversight issuecomment-6130601348 relays the operator's rejection of the residual ART0051 precision cost and overrides the prior convergence stop for that case. The unchanged f7ca3f5 freeze remains the starting candidate.

## Contract

### Root cause and code trace

The frozen replay reports ART0051 claim-621537… Unsupported, reason modal_preservation. Current semantic_support.rs SHA256 fef4b45b59e9d95f2cb5ad1daa3bab79f11f922323341e3c75ba19e5ffa7a2dd is byte-identical to that replay's frozen policy. This is reused recorded reproduction, not a new executed regression.

In src-tauri/src/pipeline/summary/coherent/semantic_support.rs:3279 permission_propositions stores complete actor/action token vectors. At :3344 permission_equivalence flattens permissions across evidence items, losing their owning item identity. At :3390 it requires exact full proposition equality; :3547 rejects the claim when no such match exists. It cannot express a shorthand licensed by the particular source item's other permission. The permission owner was introduced by our local afabcb9; f7ca3f5 added local fraction/range/optional-plural projection but no qualified noun structure. This is our comparator's missing representation, not evidence of a 9B limitation.

The requested new shape alone cannot release the whole residual claim. Direct inspection also finds: singular/plural number-group descriptor identity (:1064 leaves that difference intact), optional of in an outside preposition, and an active relative clause versus a participial noun modifier. Full action tokens preserve those differences. They are additional proposed formal projections, explicitly pending clearance below; no acceptance is inferred from the qualifier decision.

### Proven setup and every departure

Reuse the inventory in PR-C9-RELATION-GUARD-REPAIR.md: scripts/c9-preproduction-parity.py, c9-replay.json, c9-parity-manifest.json and parity.rs own the proven compact C9 lane. Restored gateway receipt c9-preproduction-parity-20261007/gateway-restored/receipt.json SHA256 54977626b64ee78505f49d26e07b99c2d1e5655bcba8c67fc0eb11bc61e3cd47 binds Qwen3.5-9B Q4_K_M model cd76ec205963b3b33350093e6904d9de16c4e666fd104e1f632d25c7f15f2a13 and server 0ca399edd758decd825a71823b04ba7ddbc8b2e10d2309d8bf623ee3c2283099: context32768, one slot, reasoning off, closed thinking framing, temperature0, seed7, output4096. Recorded restored verdict parity is30/30, with the archived adjudication/containment limitations stated in that inventory. This contract adds no live run and makes no new model-performance claim.

The starting form candidate's source hash above, forms-frozen-relation_guard_form_fixtures.json SHA256 1fa3bd944062827b81add0da32ca9ceb32ab71f47862a8ecdcf66600d49d8da3, and forms-complete-private-final.json SHA256 44d08fd0a8a389b90fd9f491a2ebbc06fc25106d5dfba3fc773b0764a61b1279 are preserved. The new departures are only evidence-local qualifier representation and, if explicitly cleared, the three listed permission-local form projections. Generation requests, prompts, framing, model, schema, settings and budgets have no departure. No generic synonym dictionary or corpus lookup is introduced.

### Required change surface: one permission owner

1. Keep each source permission attached to its EvidenceItem identity, raw clause and qualification spans at extraction. Build a typed qualified-noun view there; permission_equivalence consumes that view instead of re-scanning arbitrary evidence or deleting tokens downstream. The complete non-qualifier action, actor, modal polarity, scope and conditions remain owned and ordered. Both allow/permit and may readers use this same owner; distinct claim tiers still need distinct matching source permissions.
2. Only the following new semantic equivalence is proposed: claim non-<adjective> <head noun> may match source <same head noun> not declared <same adjective> [by <agent NP>] under the same otherwise-aligned permission. Require an affirmative permission containing the exact non-<adjective> <same head noun> in that SAME owned evidence item. Another item, uncited text, another adjective/head, or a negated witness cannot license it. Preserve compound morphology from raw text; do not infer it from a token bag.
3. Optional agent extent must be established before projection: a bounded terminal nominal phrase (article, nominal modifiers and noun, including nominal of links), with no relative clause, modal, verb, conjunction or time/condition suffix. Unknown extent yields no equivalence. Never delete everything after by. A qualifier containing currently, only, unless or other additional scope is not the registered not-declared shape. No positive declaration or permission negation is neutralized. The scope is this explicit evidence-local licensing policy; repeated shorthand is not claimed to prove an unrestricted linguistic synonym.
4. Supplemental FORM scope requiring explicit oversight clearance: number-group descriptor singular/plural identity only beside the same parsed numeric group and identical quantifier/extent; outside of <same complement> versus outside <same complement>; and active <head> that [also] <finite verb> <same object> versus <same head> <corresponding participle> <same object>. Preserve all suffix conditions and polarity. No ordinary actor plural broadening, removal of modal/tense qualifications, arbitrary stemming, other prepositions, or verb/object substitution. Reuse existing canonical numeric and controlled inflection owners where possible, confined to permissions; do not widen shared writer parsing. If safe extent or identity cannot be established, withhold and stop. These are proposals, not already approved work.
5. Remove the flattened source permission list's lost-ownership representation and duplicate full-vector matching in the two permission branches, routing both through one complete proposition matcher. Preserve exact-match behavior when no approved projection applies. Existing fraction, consecutive range, explicit optional plural and owned infinitive/gerund rules remain single owners. No whole-claim exception or later label override is allowed. Raw quotes, offsets and saved evidence are unchanged.

Future implementation surface: semantic_support.rs permission extraction/matcher and additive reviewed fixtures/tests. This contract-only commit adds this document and the adjacent PROPOSED fixture JSON under docs; it does not wire them into tests.

### Explicit non-scope

Zero generation, GPU, delegated preflights or tokenizer calls. No Synthesize/writer change, source reassociation, prompts, schemas, sampler, model, server, framing, transport, settings, storage/API, dependencies, baseline relabeling or archived evidence edits. No B1/B2 and no replacement run. No correction of the pre-existing diagnostic termination/ensuring gaps in this slice. All PR116, production, qualification and merge holds remain.

### Assumptions and blockers

Independent oversight must review this exact contract, every proposed fixture label and the supplemental form scope before implementation. If only the qualifier rule is cleared, that clearance does not authorize the supplemental projections or support a whole-claim restoration claim. The exact private residual remains available under its alias in the frozen packet; no private source text is published. Required acceptance is a real blocking gate, not elapsed time or a green check.

### Verification plan after clearance

- Reuse the production guard test path and production-reader replay. First declare and execute the public minimal fail-first licensed positive; expected failure is Unsupported instead of Supported. Prove the unchanged raw residual through the existing owner trace before repair. An unexpected result stops work.
- Review the adjacent proposed fixtures before adopting any as an oracle. Required negative twins cover declaration polarity, adjective/head mismatch, missing same-item shorthand and a different tier qualifier. Added controls cover wrong-item/uncited witness, extra conditions, ambiguous agent extent, permission negation, empty/mixed tiers and each supplemental form's unfaithful twin. The complete synthetic permission list must pass, with its missing-condition twin withheld.
- After the origin repair, both isolated positives and original residual must pass; all negative twins must remain Unsupported through final verdicts, including saved-completion validation. Mutate ownership, qualifier polarity/head, agent extent and each cleared form owner; the corresponding regression must fail.
- Re-run the SAME frozen60 candidates, comparing full ordered verdict lists and identities against f7ca3f5 AND original baseline. Zero releases past baseline; all6 correct delivered withholds retained; report every change and delivery outcome. Corrected diagnostic labels from6129693411 remain, including3 pre-existing undelivered wrong approvals and3 faithful withholds. Never call this a zero-false-approval result for the whole corpus.
- Reuse archived C9 verdict parity, frozen A1 withhold and saved-completion gates; focused tests, format and clippy. Do not duplicate broad CI without a required local gate. Stop on an unexpected verdict change, failed ownership/negative test, missing replay evidence or another semantic equivalence need; do not tune or infer another scope.
- Freeze source, fixture and complete replay receipts outside worktrees; scan exact public diff/comment for private names. Independent review of the complete changes comes before publication/adoption; then exact published-head CI and review. No hold is cleared by these checks.

## Implementation summary

Contract and proposed public fixtures only; policy code remains the frozen f7ca3f5 bytes. No executable regression or replay is claimed in this contract-only step.

## Cold diff audit

This document records the origin, evidence-local rule, additional formal blockers, forbidden changes and required gates. PR-C9-PERMISSION-QUALIFIER-PROPOSED-FIXTURES.json supplies synthetic review inputs with proposed labels, not approved labels or executable tests. The old form acceptance and evidence remain immutable; their residual-cost recommendation is superseded by6130601348.

## Gap audit

NOT DONE: exact contract/fixture/supplemental-scope clearance, fail-first reproduction, implementation, regression/mutation and complete frozen replay remain. Published green CI covers fe6fad5, not the local f7ca3f5 policy or this proposed repair.
