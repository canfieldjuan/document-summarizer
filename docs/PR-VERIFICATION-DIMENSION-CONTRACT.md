# C9: complete segments and one constrained dimension per request

Status: SUPERSEDED by the accepted original excerpt restoration in [PR-C9-PREPRODUCTION-PARITY.md](PR-C9-PREPRODUCTION-PARITY.md), implemented at eec58548a632ade9666fe4347eaccf639817753c. The acceptance and results below are historical records, not the current protocol.

Status: ACCEPTED by Juan in this chat on October 5, 2026 (America/Chicago).
The operator accepted revision `05dd213` with the exact reply: "Accept the revised
contract". This separate acceptance commit precedes every production/test change.
Acceptance authorizes the declared implementation and gates, not release or merge.

Base: `080197280b03746ce931d0b120c79da2b1aa2304`, the frozen failed segment
candidate. New branch: `codex/verification-dimension-contract`. Preserve the
failed segment and position branches and all original receipts without revision.
The segment result remains a failure; it is not qualification evidence for this
new protocol. PR116 remains fidelity-held.

This replaces the unaccepted standalone relation-shape proposal, combining the
[new scope direction](https://github.com/canfieldjuan/document-summarizer/pull/116#discussion_r4190665234)
with the [latest combined-contract request](https://github.com/canfieldjuan/document-summarizer/pull/116#discussion_r4190683407).
The latter explicitly selects the candidate itself as base, superseding the
earlier comment's ambiguous reference to its base. These comments request a
contract; they do not accept this new contract or authorize implementation.

## Root cause and evidence reconciliation

Code citations here refer to the pinned base, not current PR thread anchors.

1. **Confirmed structural defect.** In `summary/comparisons.rs:222-229`, source
   and claim list bounds are independent of relation. In `:307-316`, Rust
   requires specific empty/nonempty combinations. Both canonical and transmitted
   schemas admit the saved alias response with nonempty `not_applicable` source
   evidence; actual Rust correctly rejects it. An offline matrix finds 20
   schema-admitted shapes against eight parser-admitted shapes: 12 mismatches.
   This permits the observed contradiction; it does not establish why the model
   chose that relation. The mismatch originated in this arc's earlier C9 port,
   `fa93c8ad`, and I carried it into the segment candidate unchanged.
2. **Confirmed admission limitation.** `comparisons.rs:128-166` requires every
   piece to fit 240 Unicode scalar characters and permits only sentence ends,
   semicolons/colons or paragraph boundaries. An unsplittable unit fails before
   inference. Frozen public B units 2, 3 and 4 were rejected there; only 3/6 were
   admitted. Commas/conjunctions and an exact whole-sentence fallback are absent.
   This was a deliberate restriction in my prior accepted segment contract; the
   new scope explicitly replaces it, rather than adding a downstream filter.
3. **Confirmed semantic failure; proposed remedy unproven.** At `:230-240` and
   `:397-436`, one request requires all four comparisons. Equipment selected
   empty `not_applicable` conditions/qualifiers, missing four recorded passages,
   although those passages were available. Its supported verdict matches the
   independent label, but the required coverage gate fails. Code and output do
   not prove that combining dimensions caused the omission. One dimension per
   request is a hypothesis to test under the unchanged fidelity requirements.

The saved segment run had zero wrong approvals, 3/4 verdict parity, 26/32
reference coverage and exact catalog membership in every response. Neither that
result nor the offline schema probe clears fidelity. Original raw evidence is
under aliases `verification-revision-contract-20261005` and
`verification-relation-shapes-20261005`; new receipts use
`verification-dimension-contract-20261005`.

## Required change surface

### 1. Shape-constrained generation at the comparison owner

In `summary/comparisons.rs`, define each relation's list constraints once. Use
that definition for decoder alternatives and Rust shape checks. Keep separately
specified truth-table assertions in tests so both consumers cannot agree on a
wrong rule without detection.

| Relation | Source selections | Claim selections |
| --- | --- | --- |
| preserved / changed | 1-4 | 1-4 |
| omitted | 1-4 | 0-4 |
| uncertain | 1-4 | 0-4 |
| uncertain, source absent | 0 | 1-4 |
| not_applicable | 0 | 0 |

Each request's response is one comparison object containing, in order,
`source_spans`, `claim_spans`, `relation`. Use bounded complete-object alternatives
and finite relation enums. The source/claim item enums contain exact owned
segment text, listed once per side and shared through schema references.
Do not construct a second four-dimension response schema.

Rust checks exact text membership on the correct side before aggregation and
rejects invalid shapes even if grammar is bypassed. No normalization, fuzzy
matching, free copied text, evidence refill, relation rewriting or ID fallback.
The entire claim and governing context stay available to interpret each piece.

### 2. Deterministic segmentation fallback

Keep the existing sentence-boundary owner, abbreviation/decimal behavior,
terminal/closing-quote grouping, exact source bytes and full-context projection.
For an over-target sentence, prefer the furthest existing semicolon/colon or
paragraph cut within 240 scalars. If no such cut can bound the next piece, try
the furthest comma or coordinating-conjunction boundary within that target.

The fallback is lexical and source-independent: a comma must be followed by
whitespace; a conjunction is a complete word (`and`, `or`, `but`, `nor`,
matched case-insensitively without rewriting text). Split after a
comma or before the conjunction, preserving all punctuation and conjunction
text on one side. Do not split inside words, numbers or substrings. These cuts
are packing boundaries, not assertions that each piece is a complete proposition.

If any part still cannot fit, discard that sentence's tentative cuts and offer
the whole original trimmed sentence as one exact segment. Do not keep both the
sentence and its partial pieces. The 240-scalar bound becomes a preferred size,
with this explicit whole-sentence exception. No source text is silently lost;
non-whitespace coverage remains complete and non-overlapping before exact-string
deduplication. The original complete contexts remain intact after deduplication.

Existing combined input, projected prompt and runtime schema caps still apply
to the resulting request. A long exact piece may increase output cost; measure
it and retain the existing output allowance. No arbitrary character cut or cap
increase is permitted to make a fixture pass.

### 3. One dimension per request, one aggregate verdict per claim

Update `comparisons::plan`, `VerificationBatch`, request construction and the
comparison dispatch in `summary.rs` as needed for an explicit prepared owner
for each `(claim, dimension)`. Supply the full original claim/context and the
same segment catalogs to each request, with only that dimension's definition
and shape-constrained response. Prompt edits are limited to this decomposition;
retain the existing entailment rules and dimension meanings.

Execute sequentially in the existing order: stage, conditions, qualifiers,
scope. Preflight every actual request before inference. Use the existing shared
Verify ordinal allocator and durable request owner; no collisions with ledger
verification or another claim. Cancellation and runtime identity checks apply
to each call. Do not introduce parallel dispatch or cross-request state reuse.

Collect exactly one validated result for each dimension before producing a
claim verdict. An invalid, missing, duplicate or misassociated result fails
closed; never aggregate the successful subset or short-circuit a supported
verdict. Existing rules remain: any changed/omitted is unsupported; otherwise
uncertain or no preserved relation is ambiguous; otherwise supported. All-four
not-applicable remains ambiguous. No model-supplied aggregate verdict.

The existing `MAX_VERIFICATION_BATCHES = 64` bounds actual comparison requests,
not just claim groups. Check the expanded count before inference without raising
that cap. Four requests per claim can therefore admit fewer claim groups; report
that consequence and any resulting rejection rather than hiding it. Preflight
and execution must consume the same expanded plan and ordinal sequence.

Reserve a fresh protocol/version identity in `pipeline/contracts.rs` and
`summary.rs`. Failed verification13/14 and schema-v2/v3 identifiers remain
reserved. Preserve supported completed historical results and existing mismatch
handling; no historical live planner or issue123 resume redesign.

### 4. Remove superseded assumptions

- Remove independent permissive list/relation schema construction and the
  duplicated handwritten shape match, replacing both with the shared rule owner.
- Remove rejection solely because a sentence lacks a cut within 240 scalars,
  and the earlier hard-cap assertion for fallback segments. Retain real input,
  prompt, schema and output caps and complete-source checks.
- Replace the all-dimensions wire response and one-response-per-claim dispatch,
  including fixture adapters that assume `verdicts[0].comparisons` or score only
  `batches[0]`. Keep one public/internal aggregate claim verdict.
- Keep the positive-support guard from `3e16b954`, exact membership, missing-side
  reporting, token-boundary coverage scorer and source-preservation regressions.
  They remain necessary. Do not import range parsers, token echoes, context-ID
  allocators or the failed position branch's production implementation.

## Explicit non-scope

No synthesis/extraction, heading policy, full clause list, UI, dependencies,
installed model/defaults/settings, control labels, recorded C9 passages, new
semantic classifier, ID switch, retries, release or merge. Prompt changes are
only those necessary to request one existing dimension at a time. Issue124's
whole-document fallback and issue123's resume identity remain separate.

## Verification plan

1. Before the origin fixes, reproduce the actual production-schema mismatch
   with a minimal exact-member `not_applicable` response. Add a failing test
   requiring decoder/Rust agreement. Exercise all 20 list-presence shapes,
   lengths 0/1/4/5, both one-sided uncertain variants, valid omissions, mixed
   valid/invalid dimensions, missing/extra/duplicate fields, wrong-side and
   near-copy text, falsy/unknown relations and legitimate not-applicable cases.
2. Reproduce comma-only, conjunction-only and unbroken over-target admission
   failures on public fixtures. Prove all source bytes survive the revised
   policy, including Unicode, decimals, punctuation groups and wrapped text.
   Test below/at/above the target, whole-sentence fallback, fallback after an
   unusable primary cut and deterministic ordering. Retain old valid layouts.
3. Test the real four-request planner/executor: complete association, fresh
   ordinals alongside ledger calls, full preflight, mid-sequence cancellation,
   invalid/missing results, no partial aggregation, and expanded count bounds.
   Reopen copied historical artifacts unchanged. Run focused tests, formatting
   and strict lint; leave duplicate broad suites to CI.
4. Measure gateway and native admission with the actual per-dimension projection:
   all four request sizes, maximum combined claim/source sizes by fixture,
   first rejected neighbors, expanded request-count limits and boundary
   regressions. Re-run all six saved public B inputs: target all six admitted;
   for each remaining rejection report the input alias, dimension and cause.
   Preflight success alone does not prove grammar execution or fidelity.
5. Before any live probe or control call, run the static catalog coverage check
   against every recorded C9 passage in the four approved controls. Each exact
   reference must fit inside one available piece on its original side, at the
   scorer's existing token boundaries. A split reference fails the gate before
   inference; do not join pieces, move cuts for that case, or drop a reference
   from the denominator. Preserve the complete per-reference report. This
   admission/coverage proof must use the final splitter that will be frozen.
6. Freeze candidate source, prompts, scorer, original references, model identity
   and settings before inference. Actual native and gateway grammar probes are
   a prerequisite to the control run. Include public valid shapes and requests
   that explicitly ask for prohibited list/relation combinations. Preserve the
   final wire schema, outputs and runtime identities. Verify shape rejection at
   the grammar/converter boundary where supported, not merely Rust rejection
   after generation. If enforcement cannot be established on either runtime,
   stop and record it; no unconstrained retry or field-order workaround.
7. Run each of the four approved gateway controls three times with the frozen
   9B settings, seed and output allowance. Each repetition has a fresh run owner
   so gateway memoization cannot substitute an old result. Each complete control
   requires four dimension requests: 12 planned control runs and 48 planned
   generation calls, plus separately reported grammar probes. Preserve all raw
   attempts, including failures and any dimensions not executed after an error.
   Complete the prescribed repetitions without changing the candidate; no retry
   replaces a failed attempt. Report actual calls and latency per dimension,
   per control and overall, including added cost relative to one-request C9.
8. Every run must pass verdict parity, exact owned membership, valid shapes and
   containment of every recorded C9 passage in its original dimension/side.
   The recorded bytes must fit within one selected piece at token boundaries;
   no concatenation or prefix credit. Report exact phrase matches and extra text
   separately. Zero wrong approvals across all runs is mandatory. A single
   failed run fails the candidate; no averaging, relabeling, tuning or ID switch.
9. Freeze and report the outcome. Passing development controls do not lift the
   PR116 hold: full public A/B worker proof, independent review and unseen
   qualification still remain. Failed controls are preserved, not tuned against.

## Assumptions and blockers

One-dimension decomposition may improve selections but is not a proven remedy.
Shape correctness cannot prove semantic completeness. Coarser/longer segments
can cover a reference without proving the relation assigned to it. Repeated
fixed-setting runs measure repeatability on these controls, not generalization.
The updated segmentation may change available pieces; report those changes
without moving references or labels. Existing caps can still cause fallback.

## Implementation summary

Local implementation now changes the comparison owner, schema protocol identity,
verification dispatch and the adjacent tests. It is not runtime-qualified.

- `summary/comparisons.rs:87`: the existing sentence owner retains original bytes,
  tries the approved packing cuts and rolls back all tentative cuts if the whole
  original sentence is needed. The unsplittable-source rejection is removed.
- `summary/comparisons.rs:222`: one relation-shape table drives complete-object
  decoder alternatives and the parser. The permissive independent schema and
  handwritten parser shape match are removed.
- `summary/comparisons.rs:405`: the executor validates the complete dimension plan,
  preflights all requests, reserves shared ordinals and aggregates only complete
  groups. The old all-dimensions response parser/dispatch is removed.
- `summary/comparisons.rs:454`: the planner emits four owned dimension requests
  per claim under the unchanged 64-request cap. This permits at most 16 claims.
- The static coverage gate reports every original reference before runtime setup.
  Live reporting preserves individual responses and explicitly records unexecuted
  dimensions. A reporting adapter wraps exact response objects for the historical
  scorer without filling, repairing or combining their passages.
- An inherited scorer defect compared only admission booleans. The minimal
  regression reproduced a false parity pass for ambiguous instead of recorded
  unsupported; reporting now compares the exact recorded verdict as well.
- The protocol reserves verification 15 and schema v4. Historical persisted A/B
  views reopen unchanged. Original labels, passages, model and settings are fixed.

## Verification results and stop

Durable alias: `verification-dimension-contract-20261005`.

- The four declared origin probes failed before the implementation and pass after.
- Final comparison tests: 28 passed, 0 failed, 2 ignored.
- Coherent verification tests: 10 passed, 0 failed, 2 ignored.
- Copied historical A/B views: 1 test passed; both unchanged.
- Formatting and strict all-target/all-feature Clippy pass. After the fixed-array
  grouping lint correction, both affected executor/count tests pass again.
- Static coverage: all 32 recorded passages in four controls fit one owned piece.
  The cross-piece negative regression fails the gate without dropping references.
- The qualified native converter/grammar library accepts and rejects all 80
  independently specified shape cases correctly. The first probe export was
  invalid because converting the ordered decoder schema into a JSON value sorted
  its fields. That invalid result is retained separately. The corrected probe
  preserves the exact transport serialization; no production ordering changed.
- Installed gateway admission rejects the actual candidate schema before inference:
  `_passage_definitions` requires a root object while root alternatives require
  the root to contain only `anyOf`. Their composition is not supported. Its
  passage enum cap also admits a 240-character control but rejects a 241-character
  whole sentence. Isolating the branch-validation layer also reproduces its
  passage-budget rejection: it adds mutually exclusive alternatives to the same
  budget accumulator. Reproductions and the installed validator hash are retained.
- Planner-only sizing reaches the combined 4096-character cap for sparse, prose
  and dense fixtures on all four projections; 4097 rejects. These are not actual
  gateway/native runtime maxima. Runtime measurements and six-input B admission
  remain unrun because the gateway prerequisite failed.
- No live generation or repeated control run was performed. No semantic outcome
  or runtime fidelity claim is available. No model/settings/installed runtime,
  push, merge or review-thread state changed.

## Cold diff audit

The production surface is limited to the comparison owner, dispatch and protocol
identity. Existing extraction, synthesis prompts, model defaults and persisted
schemas are unchanged. Source catalogs and full contexts remain owned by the same
claim; parser and decoder consume the shared relation-shape table. Regression
expectations use independent shape rules. No position parser or free quote path
was introduced. Test-only historical wrappers are used solely for replay/scoring.

The installed gateway is the next origin that requires a bounded compatibility
contract. Do not inline duplicate enums, weaken relation constraints, restore the
240-character rejection or raise runtime caps to evade its rejection. This arc
stops at the required runtime gate; it is not a qualified release candidate.

## Gap audit

NOT DONE for runtime qualification. Static coverage and local origin fixes are
verified. Gateway schema admission is blocked; actual native/gateway generation
probes, runtime maximum measurements, six-input B admission, the frozen repeated
run driver, and the three repetitions of each control remain pending. PR116 stays
held. Full A/B worker proof, independent review and unseen qualification follow
only after those prerequisites pass.

## Accepted amendment during implementation

The operator requested a static recorded-passage coverage gate before the live
run and offered a narrower conjunction list. Both are incorporated above before
candidate freeze or inference. The conjunction vocabulary is now only and/or/
but/nor. This amendment does not relax labels, passage boundaries or live gates.
