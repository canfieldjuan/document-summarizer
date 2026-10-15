# PR128 separate negated-grant round — contract revision 2

Status: PROPOSED, NOT ACCEPTED. Supersedes the local582b4ec proposal only after explicit operator acceptance relayed verbatim by oversight, including the corrected labels and N1–N5 choices below. No executable candidate before STOP A. Prior consolidation PASS remains bound to8169514b209fbcb49599de660992886048c3cf7b. Contract scope is the operator's separate round, issuecomment-6159036013; review corrections are6177293976. The original proposal is preserved on codex/negated-grant-proposal-v1 and in durable evidence; no Git history was rewritten.

## Inventory first: proven setup and results

- Accepted baseline: 8169514b209fbcb49599de660992886048c3cf7b, oversight PASS issuecomment-6159166477. Shared permission owner: src-tauri/src/pipeline/summary/coherent/semantic_support.rs SHA256 a8a081fca9772d578c7f472f14c2a784f6984a3c62e38d82e8474ae04df3ffcd.
- Prior proof: docs/PR-C9-PERMISSION-CLAUSE-OWNERSHIP-PROOF.md and durable c9-guard-origin-20261011/permission-clause-ownership-final-proof.json. It records seven source-clause withholds, registered permission controls, source-aggregation mutation, archived C9 verdict parity, original A1 Unsupported, saved-completion checks, format and strict lint. This round reuses those owners and tests.
- Frozen SAME60 input identity: delivered-candidate-freeze.json SHA256 edafc01a578e0cfae3e8a99c699ac608c8fb65fceaca226a07170ab5bdb5f728. No corpus discovery, baseline refresh, input substitution or source reassociation.
- Consolidation observations: SHA256 ceee845cb539968ed40b87185484496e08f455057040665352559cf96b2e5b53, copied byte-for-byte to private negated-grant-round-20261014/consolidation-baseline-observations.json. The original private c9-guard-precision-inventory-20261012 remains immutable. Full prior comparison SHA256 315a81f857f376ac38ab3ec23cb37ead598c3d03cf52878e9dacbcb51ca8d6bb; saved validation was31, with only the approved prior ART0051 restoration versus f7ca3f5.
- Archived C9 harness: comparisons/parity.rs::archived_c9_verdicts_survive_relation_guard. c9-native-baseline.json SHA256 3351e2bc357853ec389e13220e63497363c4ba404c8c87232784e20564656a2e, recorded-result pin9dc813f6618258a23333b344c295bb43e226d79c5caecc15bc021d5ed33e9936. c9-parity-manifest.json SHA256 c3d186a65d3a28183ed6bd39fb70b3c74b081f6d41a3ba2482902fa79c6937af; public C9 source fixture SHA256 079211fe128c9bced0c52e8c36c4e5fd665f4c506b33995eaee7fa57097de627.
- The manifest pins the original system prompt7c3b024ddc32922d8fc8b50e0c9b76aab4826cd440950b04b2cce039ce08db48 and gateway runtime/model/settings receipts91a07a2a0302853350fc5c95a12094336241790a5baecf71c0fc00d657b76c40 /8c06536e8971c2872ac27477564e1bc114c69f3e854520296c8594924c6638ec /3be33f0dfc9742436700233d3310154b6943810c857bcee3af319d70ab3fa12e. Archived logical inputs, selectable text/schema and recorded responses remain unchanged. No model or server starts; this is CPU-only policy work.

Every departure from that setup: represent a narrowly owned negative grant; populate its existing negated field for both source and claim; let existing modal and permission consumers compare that polarity; add public regression inputs and a representation-removal mutation. No inference request, prompt, framing, decoder, sampler, profile, runtime or storage departure is allowed.

## State as read at the accepted code head

Tree clean; local proposal commits contain docs only. All references here are to8169514, not to old issue line anchors. semantic_support.rs::permission_propositions(:3722) drops a negative grant at:3736–3738 and hard-codes an affirmative grant at:3752; may-not retains true at:3763. permission_equivalence(:3950) preserves the cross-form-only gate and returns no objection when the required source form or propositions are absent. permission_propositions_match(:3891–3896) requires actor and polarity equality. verify_proposition_clauses(:3919/:3930) projects only recognized grants as actor/may/[not]/action; rejected grants remain raw.

Other direct permission_propositions callers: owned source collection:3957, claim collection:3980, and test-only traces:53/:308/:310. Existing behavior is pinned by qualifier tests:252, source-clause siblings:322, activation:406, identity:450 and diagnostics:483. Generic modal_statements_match in coherent.rs:6435 uses predicate/actor/object context/polarity but carries no grantor. modal_force_with_definitions(:4133) skips unaligned source predicates at:4189. Therefore reader representation alone cannot prove attribution or a dropped-negative-qualifier twin. None of this is a new live-model or released-user exposure claim; Phase B starts with injected-Supported public reproductions.

Blame: afabcb9e introduced rejection of negative prefixes; afe5fdd refactored the raw spans. The accepted8169514 per-clause aggregation repair did not introduce this separate gap. Fix the lost representation at that reader, then consume its facts at the existing comparator. Do not add a second parser or comparator.

## A1 / N1 — representation and unowned negative facts

Options:
- **Recommended:** own simple categorical do/does-not grants and their don't/doesn't spelling aliases. Preserve scope and keep other negative/uncertain forms unowned, but retain origin-produced bounded negative facts so a positive inverse cannot fall through. Cost: more conservative withholds for noncanonical paraphrases; no false claim of having interpreted an uncertain prohibition.
- Alternative: own every never/not/uncertain prefix as categorical negation. This reduces some false withholds but can falsely approve temporal strengthening, questions or conditionals as unconditional prohibition; not recommended.
- Alternative: leave unowned forms discarded with no final-verdict check. This retains the demonstrated wrong-approval class and is not an acceptable completion of this goal.

Exact recommended window/forms: immediately before permit/permits/allow/allows, own do or does followed by not (two word tokens), or the matching contraction alias as its same two-token expansion. Auxiliary forms use the uninflected permit/allow spelling. Grantor and explicit actor-to-action extent must be established. Store Grant, the existing negated=true, raw grantor, actor and complete action/qualifier spans at this one reader. Ordinary positive grants keep their current representation and behavior.

Never or bare not immediately before a grant verb, might-not/other pre-existing modal uncertainty, double negation, negation outside that two-token window, reported/conditional/interrogative scope, missing actor/to/action, and previously unowned bare/shared-actor lists do not become asserted negative Grant propositions. Not-only is unowned AS NEGATION: preserve any existing affirmative additive reading, never turn it into a prohibition. Do not add vocabulary beyond these N1 forms and existing scope/modality helpers.

Unowned does not mean safe positive evidence. For a within-window negative/uncertain grant with established grantor/actor/full-action extent, the same origin reader must return a scoped unowned-negative fact separately from asserted propositions. permission_equivalence consumes that fact before the empty-owner/cross-form early returns. It blocks a positive inverse of that same relation, including contraction/never/might-not controls; it does not negate unrelated actors/actions. Identical bounded uncertain surface on both sides may retain the existing Supported result, without promoting it into a categorical Grant. A fact with unknown actor/action cannot be invented or broadened to the entire unit. No whole-unit flag and no downstream re-scan.

Question punctuation is currently lost by semantic_clauses(:1205/:1235) before the permission reader. If question metadata is needed, produce it at that existing clause boundary and retain it for the permission reader, while preserving the exact old string projection for other callers. This bounded N1 metadata surface must not create a second splitter, change paragraph/heading behavior, or infer interrogative scope after punctuation was discarded. Do not implement a generic English parser. If that cannot satisfy the accepted fixtures within N1, STOP for scope clarification.

## A1 / N2 — same-form comparison, attribution and scope

Operator chose “Bounded now, #129 next (Recommended)”, relayed by oversight. This settles the bounded direction, not STOP A acceptance of this complete contract or its labels.

Options:
- **Recommended / chosen direction:** trigger the existing permission_equivalence/propositions_match when an owned negative Grant occurs on the claim side OR any cited source side. Apply the existing comparator to that negative relation, preserving full action, polarity and named grantor. Negative claims require matching owned authority and complete scope; a named grantor absent from the source or a different named grantor withholds. The dropped-negative-qualifier twin withholds. Exact faithful negative pairs can stay Supported. The N1 unowned-negative facts have their own narrowly scoped no-positive-inverse treatment in the same comparator. Affirmative/affirmative grant comparisons retain their old report-only behavior.
- Alternative: global full grant/grant comparison. It might catch more positive drift but would reopen the prior unaccepted positive paraphrase costs and expand this round; deferred to #129.
- Alternative: representation plus existing modal polarity alone, with no negative-family comparison. It catches the two simple inversions via modal_force_with_definitions, but misses the grantor and dropped-qualifier twins because modal contexts carry no authority and skip an unmatched object. It fails the required scope.

Comparison is per owned relation, not per unrelated negative clause in the same quote. Do not reject a faithful positive relation merely because an unrelated actor/action is negated. Existing modal contradiction checks remain active; the two original inverted-grant probes are predicted to withhold through that path as well as the accepted negative relation checks. Do not duplicate polarity parsing. Retain the original per-clause source aggregation and affirmative licensing rule (!witness.negated); negative grants cannot license shorthand.

Costs, explicitly: full negative-action comparison can newly withhold faithful negative paraphrases with different wording, and strict negative grantor attribution can withhold references implied by context but not owned text. It prevents the required unproved named-authority and scope-widening approvals. Positive/positive grantor or action drift remains unchecked as before. That positive family is tracked in #129 (including issuecomment-6204718433) and is the very next separately contracted round after this goal reaches its STOP B; it is not implemented here.

## A1 / N3 — cross-form polarity and semantic choice

Recommended decisions:
- Positive may claim against an aligned negative Grant source: Unsupported; the cited negative grant does not license that positive permission. Do not call this independent proof of a universal prohibition.
- Negated may claim against an aligned positive Grant source: Unsupported; opposite polarity.
- Named negative Grant claim against a source that states only negative may: Unsupported, because the claim invents a grantor. This corrects the prior proposal's mislabeled Supported case.
- Negative may claim against only a named negative Grant: **recommended conservative option is Unsupported**. Absence of permission from one authority need not establish a categorical actor prohibition. Alternative: explicitly accept the mirrored positive-grant mapping and label this Supported. That has fewer false withholds but can falsely approve the broader categorical statement; it is a semantic decision for the operator, not punctuation normalization. The proposed fixture uses the conservative option and requires explicit acceptance or revision.

Faithful negative grant/grant, faithful positive grant/grant and existing positive may controls retain their expected results. The existing positive unnamed-grantor equivalent is a documented pre-existing gap in #129; do not encode it as newly endorsed gold or silently fix it in this negative round.

## A1 / N4 — existing outcome

Use ClaimVerdict::Unsupported via apply_semantic_fidelity_guards(:4279), through the existing modal/permission decision owner. A permission_equivalence failure returns false from modal_force_with_definitions; the existing rejection-reasons observer calls that category modal_preservation. No new verdict, warning code, delivery state or whole-unit flag. These guards still only downgrade an injected Supported verdict and never generate new support.

Alternative: introduce a new contradiction delivery state/reason. That would widen API/storage/presentation scope with no need for this repair, so it is not recommended. Unsupported costs a possible false withhold when the bounded parser cannot prove ownership; retaining Supported would risk the demonstrated wrong approval.

## A1 / N5 — non-scope, budget and stop

No general English entailment or new lexicon beyond N1; no ensuring blacklist, A1-specific exception, whole-unit flag, writer/prompt/model/schema/decoder/sampler/profile/gateway/native/tokenizer change, source reassociation or source shortening. No new scripts, harness tooling or diagnostic code in the repo. Keep both prior non-blocking P3s outside this repair. No global positive grant comparison and no #129 implementation in this goal.

N5 is fixed by the objective, not permission to choose a broader fix. Alternative general-language or writer work is a separate accepted scope. Keeping N5 bounds can leave faithful paraphrases withheld, while expanding them without proof could falsely approve claims; neither is hidden by a new exception.

Budget: zero native/gateway generation, tokenizer sends, delegated live preflights and GPU use. Keep PR128 draft and stacked on its existing base; do not mark ready, merge, or touch PR126/PR116 branches. All production/qualification/merge holds remain. Full lib and required Rust checks still run in Phase B as the objective requires.

## A2 — frozen source-grounded prediction, not candidate execution

Oversight clarified A2 is a read-only prediction; an executable candidate comparison waits until B4. The frozen input/capsule hashes were checked. Inventoried 60 records, 825 ordered ledger/summary claims and 1060 cited source usages. Direct N1 spelling scan found zero matches. A broader negative-prefix superset yielded47 usages across22 aliases, deduplicating to3 clauses. Manual source inspection classified all3 outside the declared immediate window and without explicit actor-to-action grant extent. Their existing behavior stays unchanged; they are not newly owned or newly scoped unowned-negative relations. No private text is published.

Thus: zero newly owned negated-grant records, aliases=[], zero newly scoped unowned-negative records, predicted verdict changes=[], saved validation predicted31 unchanged. All825 ordered verdicts and claim/evidence identities are preserved in the private complete prediction. This is NOT a measured candidate result. The corpus gives zero positive coverage of the new behavior: public fail-first/twin/mutation fixtures must provide that coverage. Any actual B4 difference from this prediction is a STOP, including a seemingly beneficial change; preserve evidence and obtain independent disposition, never relabel or tune away the failure.

Public A2 aliases/hashes are appended below. Private ownership classifications and the complete ordered prediction remain in heldout/negated-grant-round-20261014. Executable B4 must use the unchanged frozen runner, originals/capsules/read-only DBs and compare full catalogs against8169514, not just counts or the original pre-guard policy.

## Proposed controls and approval

The revised JSON contains28 final-guard controls and11 reader boundaries, all proposed labels, not model-generated gold. It adds final inversion controls for every uncertain/unowned spelling, faithful negative-surface twins, grantor mismatch and dropped negative scope, plus conditional/question inversions. Contraction is now proposed as an owned spelling alias. Additive not-only remains nonnegative. The conservative N3 label is an explicit choice, not an accepted rule yet. Original v1 artifacts/labels remain preserved in durable evidence. Independent label review and verbatim relayed operator acceptance of this exact revision precede any executable tests or policy changes.

## Phase B gates after STOP A

1. Commit exact acceptance before code. Start from the pinned consolidation executable bytes. Reproduce both original wrong approvals with the public regression at8169514; record every accepted faithful/unowned control's baseline behavior. Expected fail-first must fail for the registered class; otherwise STOP.
2. Fix only the shared N1 origin reader/boundary metadata and existing permission comparator. Retain one owner per concept and remove the old unconditional rejection only where the contract permits. Preserve all uncertain/unowned marks at the origin. No downstream workaround, duplicate comparison, weakened assertion or skipped control.
3. Named mutation gates: remove negative representation; flip polarity matching; restore report-only for N2. Each must fail its registered public regression. Also exercise the scoped unowned-negative positive-inversion path and grantor/qualifier second sides. Restore source bytes and retain receipts outside worktrees.
4. At the final head run cargo test --lib and repo-required Rust checks, fmt and strict Clippy. Reuse registered permission/source-sibling controls, archived C9/A1/saved completion. Run the exact frozen SAME60 executable comparison, retaining every ordered verdict/identity. It must equal the A2 zero-change prediction; any discrepancy or input/hash mismatch is a STOP, not another candidate.
5. One implementation push after the gates; update the PR128 round section with alias/count/hash evidence and a self-review map from each new branch to the test/mutation that fails without it. Read every oversight note before pushing. Follow exact-head CI/review notifications rather than polling loops.
6. Fix review defects at their introducing owner with a fail-first test, within the accepted scope only. Stop and ask if a proper repair exceeds N1–N5. Finish only at oversight's exact-head PASS, green required checks and zero unresolved threads; report to oversight and STOP B. Do not merge or begin #129 in this goal.

## Implementation summary and cold diff audit

Revision2 is contract and proposed labels only, on top of preserved582b4ec. No executable source/test/runtime/manifest input is changed. The A2 snippets are read-only inventories outside the repo, not a new candidate/harness. The original failed broad-scan assertion is preserved as an investigation stop followed by manual classification; it is not a candidate verdict or a hidden passing test. No code, test, model, tokenizer, preflight or GPU execution is claimed.

## Gap audit

NOT DONE. STOP A: full contract and corrected28+11 labels await independent review and the operator's verbatim relayed acceptance. Then reproduction, implementation, mutations, full gates, actual SAME60 parity and final exact-head oversight PASS remain. The earlier N2 bounded-direction decision does not accept this complete contract.

## Revision2 bound artifacts

- Proposed fixture SHA256: c562055dbaa69c527fb70dfe2f3743dbf3c546e77f4dde09e25373b8b6b02268
- Public A2 prediction SHA256: f3a145c47a698376d851570e8551fdaacaba24a124507194cb737dec2c033edb
- Private complete ordered prediction SHA256: e521ddd3d017b31b11aeddcb956fdea883285b013cae66e9adcb5b7f5dcdfcc7
