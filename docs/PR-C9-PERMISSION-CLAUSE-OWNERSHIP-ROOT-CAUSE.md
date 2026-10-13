# PR128 third-round root-cause note — source clause ownership

## Contract

References below are to the published285ad6e, before the additive test shifts lines.

Root cause: permission_equivalence at semantic_support.rs:3872 reads a complete owned quote with permission_propositions(...).unwrap_or_default(). The reader aborts its entire result on any unparseable clause, discarding already recognized sibling permissions. The no-source-permissions return at :3884-3890 then treats the erased permissions as no objection. This is a source failure-scope regression from f7ca3f5, introduced in my afe5fdd consolidation. The raw clause parser's conservative rejection is not the defect; the aggregate caller changed its failure scope.

Required change surface: first preserve the reviewed seven-case fail-first test through apply_semantic_fidelity_guards. After this note is independently accepted, restore per-clause source aggregation inside each OwnedPermissions construction, using the existing shared raw reader. Keep every recognized sibling in its existing evidence_id owner. Per-clause None drops only that clause. No new parsing helper, permissive parser, whole-item bypass or downstream verdict filter.

Explicit non-scope: leave literal activation, claim-side strict parse handling, modal/definition/plain-assertion adapter, qualifier licensing, model/runtime/framing/prompts/schemas and all production/qualification/merge holds unchanged. Do not address the non-blocking necessity_projection cost or claim-side trailing-may-not observation in this slice. No generation, delegated preflight or tokenizer calls.

Assumptions/blockers: the operator's Rule21 third-round cap applies because this is another regression introduced by my earlier consolidation. This root-cause note must be accepted before policy implementation. Claude issuecomment-6136333474 supplies the seven fixture directions and confirms the original policy's behavior, but this new consolidation note is not yet independently accepted.

Verification plan: reproduce all four wrong approvals while both qualifier-loss controls and the separate-evidence control remain Unsupported. After accepted source aggregation repair, the same seven controls must all withhold. Mutate source aggregation back to whole-quote defaulting and require the regression to fail. Rerun registered permission controls and SAME60 complete ordered lists; expect only the reviewed claim-621537 restoration versus f7ca3f5, zero releases past baseline. Preserve C9/A1/saved-completion proof with a no-relevant-diff check or affected-path replay, and run format/strict lint before one consolidation push. Any unexpected verdict change stops.

## Reader inventory and consolidation

- Source comparison: permission_equivalence builds one OwnedPermissions per evidence item; this is the first site losing valid siblings and the only required policy edit.
- Modal source projection: verify_proposition_clauses at :3835 already loops per semantic clause before invoking the same reader at :3846; keep it.
- Definition and ordinary source/claim modal readers: verify_modal_predicates consumes that adapter; keep it.
- Plain assertion: plain_assertion_for_modal consumes that adapter; keep it.
- Claim comparison at :3893 deliberately uses strict question-mark failure; keep it. Its non-blocking stricter behavior does not justify relaxing claim admission.
- Debug traces and test-only source observers may inspect the strict reader; they are not policy consumers.

Remove the whole-quote source unwrap_or_default aggregation introduced by afe5fdd. Retain per-clause None handling, raw identity, evidence-local licensing and distinct matching. This is restoration of the proven f7ca3f5 reader scope, not new model behavior or another semantic special case.

## Implementation summary / cold diff audit

Contract and reproduction only. Before clearance, add only a public fail-first test; verify that removing its inserted block reproduces the exact published file bytes. No policy fix or broad retest is permitted yet.

## Gap audit

NOT DONE. Reproduction/isolation/explanation and independent acceptance precede fix/proof/mutation/frozen replay. Existing CI is green at published285ad6e, but Claude's P2 blocks adoption. PR128 has no inline thread for this finding; the issue comment remains a first-class blocker. PR116's two hold threads remain open.

## Reproduction result

One fail-first test at published285ad6e plus the additive public probe, exit101: grant_negated_sibling, grant_malformed_list_sibling, may_negated_sibling and may_trailing_incomplete_sibling returned Supported. grant_control, may_control and grant_separate_negated_item stayed Unsupported. Removing the inserted test restores published semantic_support.rs byte-for-byte (SHA256 e10968c3a4a23d348955e4c7c9b57b1005ad3ab4864ef5d31d3ec7f437a7194b). No policy repair or broad tests followed this stop.
