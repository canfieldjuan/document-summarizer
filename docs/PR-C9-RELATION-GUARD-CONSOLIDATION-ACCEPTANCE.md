# Accepted precision consolidation for PR128

## Contract

Operator accepted the precision repair and, after oversight approval, explicitly directed continuing implementation. Oversight approvals are issuecomment-6124728844 and issuecomment-6128274791 on PR128. The immutable revision-2 contract and proposed revision-3 amendment remain preserved. This acceptance records the additional constraints without rewriting those documents.

### Root cause

The Verify relation reader breaks a wrapped proposition at every newline, classifies vocabulary occurrences as evaluations without distinguishing noun uses, and lacks a source-owned assignment-value comparison. The candidate's removal of a source-empty waiver and addition of source-modal preservation exposed the faithful-case regressions. Changed assignment values were also approved by the earlier policy. Reproductions and introducing revisions are recorded in the precision amendment and the final-oracle finding.

### Required change surface

Consolidate the internal relation boundary, classification and comparison owners in semantic_support.rs, with additive public precision fixtures and final-verdict regression tests. Preserve the original fixtures and existing tests.

Recognized assignment relations carry subject, assignment classification, complete value/range, scope and polarity. Pair by subject independently of value equality; compare the complete owned values for every recognized assignment, including sources without shall. This is the only owner permitting definitional shall-be to become is. The modal reader consumes its decision; it must not separately re-parse definitions. Passive actions/states remain obligations, including payment and extension with the same term subject.

Recognized range connectors through, to, en dash and hyphen may normalize within a range. Endpoint values must not normalize. Through-to and en-dash precision controls preserve support. Changed endpoints and Basic-to-Premium assignments withhold. Unrecognized assignments receive no new withhold from this owner; existing guards retain their outcome, including the modal rule for an unclassified shall construction.

Layout handling preserves wrapping within an owned proposition, and separates paragraphs, bullets, headings and evidence items. Noun uses of safety, health and guarantees do not create judgments. Necessary/required-to paraphrases preserve the same actor, action, object and conditional scope. Unknown actors, invented means/causes, changed scope and lost obligations remain withheld.

### Explicit non-scope

Zero inference, delegated preflight and tokenizer calls. No GPU use, writer/parser feedback change, prompt/schema/model/runtime/sampler/framing change, API/storage/version change, archive relabeling, deployment, live qualification or merge. PR116 and production holds remain.

### Assumptions and blockers

Approved public labels and the independent precision judgments are the oracle. All changed frozen-inventory claims require independent oversight review. If the accepted relation constraints cannot be represented mechanically within this scope, stop and report the limitation before expansion.

### Verification plan

Commit additive final-verdict regressions, deliberately reproduce faithful rejections and wrong approvals before the fix, then prove the same cases after repair. Preserve original relation fixtures and directly affected tests. Reuse archived C9 parity, frozen A1, real saved-completion validation and the entire frozen inventory, comparing full verdict lists. Prove boundary/classification/value ownership with deliberate mutations in durable scratch copies. Run required format/lint gates. One consolidation push; stop for independent precision review.

## Implementation summary

Acceptance only. No policy edits in this commit.

## Cold diff audit

Only this acceptance record is added. Frozen proposals and production source remain unchanged.

## Gap audit

NOT DONE: implementation, regression/mutation proof, frozen-inventory replay and independent precision clearance remain.
