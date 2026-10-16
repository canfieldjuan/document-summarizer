# Precision consolidation stop report for PR128

## Contract

Operator and oversight accepted the precision amendment and definition-value owner, recorded in PR-C9-RELATION-GUARD-CONSOLIDATION-ACCEPTANCE.md. The amendment requires preserving independently faithful cases and stopping when they cannot be preserved. This local attempt reaches that stop. No further policy edit, generation, qualification or merge follows this report.

## Implementation summary

Local semantic_support.rs consolidates layout boundaries, classifies noun versus evaluative use, represents bounded necessity qualifications, and adds a source-owned assignment-value decision consumed by modal preservation. The original fixtures and writer helpers remain unchanged. Additive tests assert the final verdict for all 29 approved public cases and probe value, polarity, scope, mixed input and evidence boundaries.

Declared fail-first regression failed on the expected faithful rejections and changed-value false approvals. Final targeted tests pass: 4 passed, 0 failed, 1 ignored. The affected summary suite passed: 380 passed, 0 failed, 20 ignored; this was before three lint-only needless-borrow removals. Final targeted and full-inventory replays ran after those removals; inventory observations are byte identical. Format and all-target/all-feature clippy pass. Archived C9 verdict parity and real saved-completion tests passed in the summary suite; A1 remains Unsupported. Zero generation calls.

## Cold diff audit

- semantic_support.rs: semantic_clauses changes only the internal Verify relation layout reader; every existing caller borrows its owned clauses. evaluation_relations distinguishes bounded necessity and noun uses. definition_relation/definition_value_decision supply the final and modal owners. Existing saved validation and writer helpers are unchanged.
- relation_guard_precision_fixtures.json and additive tests preserve the independently approved public oracle; existing fixtures/tests were not relabeled.
- Acceptance and this report document the approved scope and the failed gate. No production deployment or remote policy push.

## Full frozen inventory

Full verdict lists, identities and evidence IDs were compared for all 60 frozen candidates. Baseline admission: 34; first candidate: 27; local consolidation: 30. Legacy admission errors remain visible.

Of the previously delivered flips independently reviewed by oversight, 9 faithful claims recover; 8 faithful claims still withhold; all 6 correct withholds remain. One new delivered flip requires independent review. All previously changed claims are retained in the comparison, including those restored to Supported, and all new changes are included. Private text stays in the private packet.

Private packet: precision-consolidation-private-v3.json SHA256 2a570653a5de6fb4e73a40c18defcd63b08825870640e152b592e75fc183ae06.
Stop receipt: precision-consolidation-stop-receipt.json SHA256 9de8e201f16676ef3edc338a37e1e7870921b5ff434454739a49b4ad0506e0d2.

## Remaining root-owner limitations

These are findings against the local code, not new approved fixture labels:

1. ART0030/0040/0062/0232: definition_relation captures the remaining value text through the end of a semantic clause. A summary joins the unchanged range to other cited facts with a conjunction, so the added proposition becomes part of the assigned endpoint. The complete-value comparison then falsely conflicts; the source modal also remains unresolved. Fixing this needs a reliable assignment-value extent at the owning proposition boundary. Truncating at a guessed date or approving a matching prefix would not establish that extent.
2. ART0051 nominal guarantees with a following rationale: the causal classifier treats the nominal occurrence as a verb when following words remain. This is an incomplete nominal/verb classification; the actual source rationale remains independently faithful.
3. ART0051 embedded article, compressed actor and action paraphrases: exact EvaluationRelation token identities do not preserve a/the inside a larger subject, equivalent actor phrasing, or offer/offering in the action. The simple leading-article control did not cover these structures. Broad token deletion, lexical rearrangement or blanket source-present waivers would not establish the same actor/action/scope.
4. ART0051 new minimum-age flip: restoring wrapped source propositions exposes source may permission to the modal reader, while the summary expresses permission using allow over a coordinated list. That representation is outside the existing modal-anchor vocabulary. Its new withhold is UNREVIEWED, not asserted correct or relabeled as false.

Shared class: surface clauses do not carry enough proposition ownership for these compound or nonliteral relations. I will not extend grammar or add exceptions from the private corpus without independent review. The current code is preserved as a failed local candidate, not a qualified repair.

## Gap audit

NOT DONE. The accepted faithful-case gate fails. Independent review of the complete change list and the remaining proposition-owner limitations is required before another policy consolidation. New boundary/classification/value mutation gates were not run after this stop; their absence is explicit, not a pass. PR116, production, native qualification and merge holds remain. Existing writer findings and the independent A1 WITHHOLD ruling remain unchanged.
