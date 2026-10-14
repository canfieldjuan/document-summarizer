# PR128 separate negated-grant round — proposed contract

Status: PROPOSED. Operator acceptance of this exact scope and independent review of the proposed labels are required before executable tests, policy changes or replay. “Continue” authorizes preparing the proposal, not accepting it. The prior consolidation remains passed at 8169514b209fbcb49599de660992886048c3cf7b.

## Inventory first: proven setup and results

- Accepted baseline: 8169514b209fbcb49599de660992886048c3cf7b, oversight PASS issuecomment-6159166477. Shared permission owner: src-tauri/src/pipeline/summary/coherent/semantic_support.rs SHA256 a8a081fca9772d578c7f472f14c2a784f6984a3c62e38d82e8474ae04df3ffcd.
- Prior proof: docs/PR-C9-PERMISSION-CLAUSE-OWNERSHIP-PROOF.md and durable c9-guard-origin-20261011/permission-clause-ownership-final-proof.json. It records seven source-clause withholds, registered permission controls, source-aggregation mutation, archived C9 verdict parity, original A1 Unsupported, saved-completion checks, format and strict lint. This round reuses those owners and tests.
- Frozen SAME60 input identity: delivered-candidate-freeze.json SHA256 edafc01a578e0cfae3e8a99c699ac608c8fb65fceaca226a07170ab5bdb5f728. No corpus discovery, baseline refresh, input substitution or source reassociation.
- Consolidation observations: SHA256 ceee845cb539968ed40b87185484496e08f455057040665352559cf96b2e5b53, copied byte-for-byte to private negated-grant-round-20261014/consolidation-baseline-observations.json. The original private c9-guard-precision-inventory-20261012 remains immutable. Full prior comparison SHA256 315a81f857f376ac38ab3ec23cb37ead598c3d03cf52878e9dacbcb51ca8d6bb; saved validation was31, with only the approved prior ART0051 restoration versus f7ca3f5.
- Archived C9 harness: comparisons/parity.rs::archived_c9_verdicts_survive_relation_guard. c9-native-baseline.json SHA256 3351e2bc357853ec389e13220e63497363c4ba404c8c87232784e20564656a2e, recorded-result pin9dc813f6618258a23333b344c295bb43e226d79c5caecc15bc021d5ed33e9936. c9-parity-manifest.json SHA256 c3d186a65d3a28183ed6bd39fb70b3c74b081f6d41a3ba2482902fa79c6937af; public C9 source fixture SHA256 079211fe128c9bced0c52e8c36c4e5fd665f4c506b33995eaee7fa57097de627.
- The manifest pins the original system prompt7c3b024ddc32922d8fc8b50e0c9b76aab4826cd440950b04b2cce039ce08db48 and gateway runtime/model/settings receipts91a07a2a0302853350fc5c95a12094336241790a5baecf71c0fc00d657b76c40 /8c06536e8971c2872ac27477564e1bc114c69f3e854520296c8594924c6638ec /3be33f0dfc9742436700233d3310154b6943810c857bcee3af319d70ab3fa12e. Archived logical inputs, selectable text/schema and recorded responses remain unchanged. No model or server starts; this is CPU-only policy work.

Every departure from that setup: represent a narrowly owned negative grant; populate its existing negated field for both source and claim; let existing modal and permission consumers compare that polarity; add public regression inputs and a representation-removal mutation. No inference request, prompt, framing, decoder, sampler, profile, runtime or storage departure is allowed.

## Root cause: what the code actually does

At baseline semantic_support.rs:3736-3737, permission_propositions returns None whenever a grant's prefix contains negation. At :3752 recognized grants are hard-coded affirmative. A source does-not-permit therefore becomes unowned; it cannot reach the existing actor/may/action projection at :3919. By contrast may-not is represented with negated=true at :3763. Source aggregation at :3956 correctly drops only the rejected clause, but cannot recover polarity lost in the raw reader.

Blame identifies the conservative rejection in afabcb9e and its raw-span refactoring in afe5fdd; the accepted8169514 source aggregation repair did not introduce this independent gap. Claude's two guard-seam probes are in issuecomment-6159022549. This session verified the code path; it has not independently run those probes. Reproduction remains the first executable step after acceptance. A guard test injects Supported; it is not new live-model or released-user exposure evidence.

## Required change surface and one owner

Only semantic_support.rs::permission_propositions owns the new representation. Its existing PermissionProposition, raw actor/action spans and negated field feed the existing consumers. Do not add a negative-grant scanner in a comparator, modal reader, saved-result validator or presentation step.

Proposed ownership rule:

1. Own only an unambiguous main-clause grantor followed immediately by do/does + not + permit/allow, with the existing explicit actor-to-action structure. The negative verb is the uninflected permit/allow spelling. Recover actor/action exactly as for an affirmative grant; retain the full action, conditions and qualifier spans. For each owned item set negated=true. Recognized ordinary affirmative grants still set false.
2. Main-clause extent must be established. Conditional, reported, uncertain, embedded or double-negated prefixes must not be flattened into categorical permission. If this cannot be established within the existing reader, leave the clause unowned and stop if the accepted tests require an unapproved parser change. No general grammar, lexicon or fuzzy-equivalence expansion.
3. Negative grants project internally to the existing actor/may-not/action identity. This is Verify's comparison normal form; it does not generate replacement source prose or grant new permission. Grantor continues to be excluded from actor identity, as in the accepted positive-grant rule; grantor substitution remains diagnostic under issue129.
4. An aligned negative grant cannot license positive may. It contradicts the same owned actor/action positive proposition. The existing modal polarity matcher consumes the representation even when full explicit-grant/source-grant comparison remains inactive. Do not widen that full-comparison activation boundary.
5. Cross-form may versus grant comparisons continue using the existing full action/subject/polarity matcher. Faithful negative may versus negative grant and faithful same-spelling negative twins must remain Supported. Preserve conditions; do not infer an inverse conditional paraphrase.
6. Evidence-local shorthand still requires an affirmative permission witness. An owned negative grant never supplies that license; use the existing !witness.negated check at :3814, not a new filter. Unrelated actors/actions do not inherit another proposition's negation.

What stays unowned: missing actor, missing to/action, bare/shared-actor lists the reader already cannot own, contractions, never/cannot-style noncanonical grant negation, double negation, and uncertain/conditional/reported grant prefixes. Unowned means no newly invented representation, not a new whole-quote rejection or a claim of Supported. Existing may-not, per-clause source aggregation and fail-closed malformed claim behavior remain unchanged.

Required test surface: add the independently accepted public fixtures to the existing semantic guard test module (or one adjacent JSON fixture); keep the original qualifier fixture and labels immutable. Use existing EvidenceItem/CitedClaim/apply_semantic_fidelity_guards helpers. Raw-reader boundary cases inspect representation at its owner; final verdict cases exercise the real deterministic guard with injected Supported.

## Proposed fixtures and label acceptance

See docs/PR-C9-NEGATED-GRANT-PROPOSED-FIXTURES.json (SHA256 f988ad1176a49c21ac84fb7486f041acfe5088b1d0c7a597ce37c980dc4b8095). It has16 proposed final-verdict controls and8 unowned-reader boundaries. They include both reviewer probes, exact negative twins, allow/do spelling, both may/grant directions, faithful positives with negative siblings, actor/action separation, unchanged condition text, and a negative shorthand-license witness. Expectations are proposed, not model-derived or candidate-generated gold. Accept or correct every label before it becomes a regression oracle.

## Explicit non-scope and budget

No prompt/model/server/framing/schema/sampler/profile change; no Analyze/Synthesize generation; no source slicing, reassociation, quote rewriting, broad shared parser refactor, UI, persistence, migration, dependency, cap increase or optimization. Whole-document admission, issue129's plain invented grants/grantor fidelity, claim-side stricter parsing and necessity_projection's cost stay outside this round. All PR116/production/qualification/merge holds remain.

Budget:0 generation,0 delegated preflights,0 tokenizer sends;0 GPU use. No live qualification or replacement run. Reuse existing offline harnesses instead of creating a new model proof.

## Verification and stop rules

1. Commit exact acceptance before code. Confirm unchanged baseline source/input hashes.
2. Reproduce before fix: the two inverted-grant probes must retain the wrong Supported verdict. Run every accepted fixture and record faithful-twin behavior; some twins may already pass. An unexpected result or different failure is a STOP, not permission to tune the oracle.
3. Isolate raw reader None versus proposed negated representation and its real consumer path. Fix only at that reader after the reproduction. Inventory every consumer listed above; remove the blanket negative-prefix rejection only for the newly owned canonical form. Retain rejection for uncertain extent/noncanonical forms. No redundant side parser or waiver.
4. Prove all accepted positives/twins and raw-boundary expectations, the existing permission controls and seven source-clause regressions. Mutate away only the new negative representation (return to its baseline rejection); the inversion regression must fail. Restore frozen source bytes and record the mutation result.
5. Re-run SAME60 via the unchanged archived_delivery_uses_production_getters_and_validation runner, using the same freeze, capsules and read-only database copies. Compare every ordered ledger/summary verdict, claim_id and evidence_ids directly to the8169514 observations. Also retain the original pre-guard comparison and disclose any releases. Print the complete change list and saved-validation changes; counts alone do not pass.
6. No private-inventory verdict changes are pre-authorized. Any change from the accepted consolidation is a STOP for independent source/label disposition; preserve all outputs and do not relabel, adjust source selection, add equivalences or rerun a candidate to improve the score. Zero wrong approvals remains the requirement. No-input/identity/freeze difference also stops.
7. Reuse archived C930 decisions, original A1 withhold and saved-completion/reload tests. Run targeted checks, format, diff and strict all-target/all-feature lint. CI owns duplicated broad suites. Preserve durable logs outside worktrees, private material only in heldout; public artifacts use aliases/SHA256.
8. One completed round push after gates; fresh exact-head CI, bot reconciliation and independent oversight PR/precision review before adoption. The prior consolidation PASS covers only its existing head; this proposal does not extend it. Standing merge/release holds are not lifted by acceptance or a later round PASS.

## Implementation summary / cold diff audit

Contract and proposed labels only. No executable source/test/fixture used by production changes in this commit. Durable inventory and source freeze pin the accepted baseline; the private baseline copy is byte-identical. New labels are not wired into the test harness. No new tests or model calls ran.

## Gap audit

NOT DONE: operator acceptance of this exact proposal, independent supplemental-label acceptance, fail-first reproduction, policy implementation, mutation, frozen replay, regression gates and final review. This is the concrete reviewable scope required by issuecomment-6159036013; no implementation acceptance is implied by its preparation.
