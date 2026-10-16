# Proposed precision amendment for PR128 / issue127

Status: operator has directed preservation of the two faithful legal paraphrase classes. Independent oversight must accept this exact amendment and the proposed public labels before another policy implementation. The accepted revision-2 stop on faithful-case failure applies now; the first candidate is NOT CLEARED.

Base candidate: 1abee71cb3fb86e39ff4acbedce3aca4698d820d. Accepted original contract remains immutable: CONTRACT-REV2.md SHA256 04ff23ec4a734713da9acb6f8ed463b46e81a453f1ea86ee0b27b00a152b89a5. This is its proposed amendment, not a replacement baseline or a new generation trial.

## Root cause and findings reconciliation

Review: https://github.com/canfieldjuan/document-summarizer/pull/128#issuecomment-6124137236 . Every frozen precision flip was replayed through the unchanged candidate decision owners with CPU-only relation traces.

- CONFIRMED: semantic_support.rs:558-562 treats every newline as a semantic boundary. The source's wrapped subject and predicate become separate clauses, while the same unwrapped summary has one relation. Public wrapped_subject fails evaluation; heading_blank_boundary with a wrapped body fails too. Source-owned paragraphs/bullets must retain their boundaries, while ordinary wrapping within a body must not destroy its proposition.
- CONFIRMED: evaluation_relations at :2100-2118 selects concept occurrences lexically and searches backward for any copula; it can misread a noun modifier as a judgment. It treats every guarantee/guarantees occurrence as transitive. Public noun_guarantee and noun_guarantees reject verbatim copies. Domain uses of safety/health are not assertions that their grammatical subject is safe/healthy.
- CONTRADICTED AS A STANDALONE CAUSE: words already lowercase at :2096-2099 and a leading article is removed at :2153-2159. article_swap and acronym_case pass. The near-verbatim private failures are confirmed, but adding another case/article normalizer would duplicate an existing rule. The wrapped_acronym probe fails at the boundary/extraction owner.
- CONFIRMED: modal_force_supported at :2351-2378 compares omitted source modality against a plain assertion without classifying a source definition. It withholds both definitional_shall and dropped_obligation. The first is faithful; the second must remain withheld.
- CONFIRMED: the private necessary/required-to cases fail evaluation. The proposed minimal required_necessary case reproduces that failure. A simpler sentence without a preceding copula did not reproduce it; it is not the regression fixture.

Introducing change: my candidate 1abee71cb3fb86e39ff4acbedce3aca4698d820d removed the source-empty precision waiver and added source-side modal preservation, making legacy extraction weaknesses and previously tolerated paraphrases withhold. Newline splitting was introduced by fb32efec; the lowercase/article extraction predates this candidate. This is the first correction round on this candidate. Fix these concepts at their extraction/comparison owners, not by restoring the blanket waiver or whitelisting aliases.

## Required change surface and policy

One consolidation of semantic_support.rs and directly relevant tests/fixtures/proof documentation on the existing separate candidate branch. Verify and saved validation keep one decision owner. Writer helpers, feedback and extraction behavior stay unchanged.

1. Relation extraction must preserve propositions across layout wrapping within the same owned paragraph/bullet and separate real paragraphs, bullets, headings and evidence items. Do not join all whitespace indiscriminately, use a heading as substantive support, or infer a subject from another item. Reuse any applicable existing layout owner; if its representation cannot carry the required distinction, stop and report before expanding source association or storage.
2. Classify an evaluative/causal use at extraction, rather than treating every vocabulary occurrence as a relation. Preserve actual predicative/attributive judgments and causal verbs, their actors, effects, scope and polarity. Domain noun modifiers and nominal guarantee uses are not causal assertions. Remove extraction branches made redundant by that classification; keep the unknown-actor fail-closed signal for actual recognized relations. No exact-quote bypass, substring approval, downstream rescan, fuzzy passage lookup or corpus-specific exception.
3. Operator decision: preserve necessary as a paraphrase of required-to only when owned evidence establishes the same actor, action, object and conditional scope. It is not permission to approve an unsupported necessity judgment or a new means relation. necessary_scope_change and maintenance_means remain withheld.
4. Operator decision: preserve a source-owned definitional shall-be assignment as an is definition of the same subject and value/range. Classify an explicit definition from its owned context; shall-be alone is insufficient. A prescribed action/state remains an obligation. dropped_obligation and dropped_required_state remain withheld; changing a definition's value/range must not gain support. Unknown usage remains unresolved. Do not weaken must, recommendations, permissions, prohibitions or conditional obligations generally.
5. Keep independent true-positive withholds, including the A1 relation reversal, obligation-to-condition change and invented maintenance means. Removing an erroneous extraction must not silently approve these other unsupported relations. If the existing material-relation owner cannot preserve these constraints mechanically, stop and report its limitation rather than adding exceptions or widening grammar without review.

## Explicit non-scope

Budget: zero generation, delegated preflight and tokenizer calls; no GPU. No new Analyze/Synthesize/Verify invocation, request/prompt/schema/framing/model/runtime/decoder/sampler change, sentence-citation redesign, storage/version change, fallback/warning/retry change, archive relabeling, deployment or merge. PR116 and production qualification holds remain. The writer finding remains open, and the frozen A1 draft stays independently WITHHOLD.

## Assumptions and gates

The original accepted fixtures and adjudicated C9 results remain mandatory. The independent precision review's faithful/withhold classifications are not replaced with candidate outputs. New public labels are proposals until oversight approves them. Existing tests stay unchanged; an inability to preserve an approved faithful case stops implementation.

## Verification plan

1. Oversight accepts this exact amendment and labels all proposed public cases before policy edits. Source/test diagnostic copies are durable and contain unchanged production bytes plus observers only.
2. Commit minimal fail-first regressions from the approved layout and noun-use patterns. Record expected false rejections on this exact candidate. Include unchanged passing article/acronym controls to reject the incorrect normalization diagnosis.
3. Repair the origin once. Show the same probes pass, the wrong-relationship siblings remain withheld, and directly affected existing guard/reader tests pass unchanged. Prove the new boundary/classification owner with a deliberate scratch mutation, not source-text assertions.
4. Reuse all archived C9 requests/responses and the original A1 guard replay without new generation; preserve prior adjudicated results and diagnostic passage accounting.
5. Replay the same complete frozen inventory through old and candidate policies. Keep every artifact and every admission result, including legacy pre-existing rejections. Report every flip and previously flipped claim that becomes supported, plus any new flip. Compare full verdict lists, not only aggregate counts. Oversight independently reviews all changed claims against exact owned evidence and their delivery impact.
6. Saved stale contradictory support still fails before completion, faithful completion survives, sealed records remain unchanged. Run fmt/clippy and the contract-required affected reader tests. One consolidation push; stop for independent precision review. CI green cannot release the holds.

## Evidence aliases and SHA256

- Proposed public labels: review-public-fixtures-proposed.json f0d17f6b5984c5924827c72a3f2a6aa9e52dc6c5072254c796da28f42a24b663
- Public observations: review-public-proposed-observations.json 694ea8648568e089b4d5bd7732f81c726f19ffccb288992771f61e8aadb8992d
- Public CPU log: review-public-proposed-observations.log bf9f4c15354fd04b89c1a9b0f930993cd9498b80b161a36992bc97a5477652d0
- Private replay status log (aliases only): review-private-observations.log ed832881043b886cbf1b5eb0dbbfb8c098a2df47dd39a145865b406db32216fc
- Private owned-text extraction trace: review-extraction-trace-v1.json c4d9cd974ef85e9fdd2e713abac74ccbe30c0ddacee8d1c4f5cc6a7eed69d678

The successful observer tests mean that the wrong behavior was reproduced, not that the candidate is repaired or its proposed labels are independently cleared.

## Implementation summary

Only diagnostic observers/proposed fixtures and this amendment were prepared. Production policy unchanged. Operator approved preserving the two paraphrase meanings; exact amendment and supplemental labels await independent oversight.

## Cold diff audit

No production/source/test change is published by this amendment. Scratch diagnostic source retains candidate guard bytes as an unchanged prefix. The tracked change is this contract addendum only; the accepted original contract remains intact.

## Gap audit

NOT DONE: independent amendment/label approval, origin repair, pass-after and mutation proof, complete inventory re-review and precision clearance. No live qualification or merge is authorized.
