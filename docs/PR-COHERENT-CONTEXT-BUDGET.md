# Coherent synthesis uses its effective context

## Root cause

The coherent synthesis producer and runtime validator use the analysis-stage
character helper, whose fixed 16,000-character cap survives a larger runtime
context. Saved production requests on real contracts A and B reproduce the
result: their complete retained catalogs fit 32,768 tokens with output and
framing reserves, but the application selects a smaller source catalog before
the exact runtime admission check. This is an admission defect, not evidence
that larger input alone improves summary quality. See issue #102.

## Required change surface

- `summary/coherent.rs`: one coherent input-budget owner, used by synthesis and
  runtime validation. At or below 8,192 context tokens preserve the existing
  conservative cap. Above that context use checked arithmetic for
  `(context - 2,048 output - 512 framing reserve) * 3` characters, including
  system prompt, user prompt and serialized response schema.
- Pass this same input limit through source reduction's stop condition and the
  existing bounded repair loop. Every admitted generation still passes exact
  runtime token preflight. Invalid budgets and non-context admission errors
  continue to fail; overflow still selects/reduces or falls back as before.
- `summary.rs` and coherent tests: reproduce a retained catalog unnecessarily
  reduced at 32k, prove full-source generation and runtime validation, and retain
  8k reduction, exact-token rejection, reserve and downstream verification gates.
- Amend the canonical synthesis budget paragraph in `docs/CONTRACTS.md`.

## Explicit non-scope

No preset/context default promotion, automatic profile routing, output-token
increase, source segmentation/omission change, prompt/schema wording change,
selection-window size change, analysis or verifier budget change, dependency,
public API, storage, artifact-format/version, UI or Connect change. Contract
incomplete-catalog admission remains separate draft #92. Completed artifacts
keep the same content validation; no stored catalog identity or text changes.

## Assumptions and blockers

The effective synthesis context comes from the existing runtime profile, not
ambient settings. The larger character allowance is a conservative proxy, not
token proof. Exact preflight is authoritative. Selection requests retain their
existing small window and budget. The private 32k test profile is experimental;
live evidence may still show fallback, poor coverage or model failure.

## Verification plan

1. Fail first through production synthesis with a synthetic retained catalog
   exceeding 16,000 characters that fits 32k: expect the old code to select
   sources, violating the no-selection assertion. Then prove full generation,
   validation and unchanged lower-context selection.
2. Probe output/reserve exhaustion, the exact character boundary, schema
   accounting, actual runtime context rejection, and unchanged analysis and
   verification limits. Reuse existing repair, grounding and coverage regressions.
3. Run affected summary tests, formatting and strict all-target/all-feature
   clippy. CI owns the duplicated full platform suites.
4. Reuse the saved real-contract diagnostic and pinned tokenizer to check both
   retained catalogs with the new budget. Run General A/B through the production
   pipeline at experimental 9B/32k with the existing inference lock, owned-server
   checks, private receipts and unchanged acceptance gates. Retain failures and
   report source admission separately from supported prose and final delivery.
5. Compare the final diff to this contract and request exact-head review.

## Implementation summary

The coherent-only helper now owns generation and runtime-validation admission.
The existing limit is passed into selection stop checks and bounded repairs.
Three regressions cover full-catalog admission, exact-runtime rejection and
reserve/legacy-stage boundaries. No runtime defaults or source text changed.

## Cold diff audit

- `summary/coherent.rs`: two callers now share the coherent budget owner; exact
  runtime checks and response schema are unchanged. Its boundary regression
  covers reserves, the request threshold and unchanged neighboring stage caps.
- `summary.rs`: test-only additions exercise real synthesis and validation,
  preserve 8k selection, and reject a 32k request when runtime admission fails.
- `docs/CONTRACTS.md`: records the new coherent budget at the canonical seam.
- Boundary-probe: the large valid catalog no longer selects at 32k; 8k still
  selects. Exact runtime rejection still produces no synthesis inference.
- Effect-trace: effective Synthesize context controls the shared input limit;
  the production-path no-selection assertion failed before and passed after.
- No shared mutable state or admission queue changed. Exact source identity,
  semantic verification and final delivery coverage remain enforced.

## Initial verification before recovery

The budget-only implementation was NOT DONE for merge. The admission fix and deterministic verification pass:
210 summary tests passed, 10 live tests ignored; formatting, frontend build and
strict all-target/all-feature clippy passed. The saved real-catalog diagnostic
passed and both full retained catalogs fit 32k, with the 8k behavior preserved.
The initial regression setup failures (missing frontend assets, then an invalid
synthetic response) preceded the valid fail-first assertion and are not its proof.

The isolated production comparison on implementation `aa2def4` proves admission,
but fails the no-regression requirement. The private test composition uses the
same 9B/32k profile, framing and unchanged acceptance wrapper as the baseline;
input hashes match and both runs verify exclusive GPU and actual 32k context.

| General mode | Before | After |
|---|---|---|
| Real contract A | Disclosed fallback, 9/10 pages | Synthesis failure, no delivered summary |
| Real contract B | Disclosed fallback, 14/14 text pages | Same disclosed fallback, 14/14 |
| B supported coherent pages | 1/14 | 5/14, still below coverage gate |

A received all 64 retained sources; B received all 135. A returned incomplete
paragraphs, including two at the existing 1,200-character limit; the parser
rejected them as MODEL_SUMMARY_RESPONSE_INVALID. Its clipped-unit repair is
limited to selected/windowed General catalogs, so the newly admitted full
catalog does not enter that path. Another incomplete unit is below that exact
limit. No invalid output was delivered or validator relaxed.

B's first draft cited five pages; its correction cited four. The application
retained and verified the initial draft, then used the disclosed ledger fallback.
Single-run elapsed times were A 142.21 seconds and B 175.28 seconds; no latency
qualification is claimed. Source passage omissions remain unchanged.

Hold merge and the 32k preset promotion. Next define and replay the bounded
incomplete-output recovery contract at the coherent response owner; do not add
an ad hoc acceptance exception. This is separate from Contract admission #92.
The live failed attempt remains in the denominator. Exact-head review and CI
also remain required. Private raw source/model artifacts stay local; only
opaque labels, hashes and aggregate evidence belong in the public PR.

## Recovery contract revision

New evidence: the retained A response fails General synthesis before a coherent
artifact exists, although its unchanged per-page ledger previously delivered
9/10 pages after verification. Increasing admitted context must not discard that
independently verifiable fallback merely because the initial prose is malformed,
clipped or unfinished. The current repair classification depends on selection
windows and an exact decoder limit; changing those two predicates alone would
not handle every recorded unit and is not this fix.

Revised root cause: a rejected model-produced draft and an operational failure
leave synthesis through the same untyped error channel. The caller consequently
fails the entire job rather than preserving the independently verified ledger.

Revised required surface:
- Reuse the typed model-output versus runtime/invariant distinction already
  developed in draft #92 at the coherent generation owner. Port only that shared
  mechanism; do not import Contract admission, versioning or omission changes.
- For General prose generation, if bounded existing recovery produces no usable
  draft for a model-output reason, use the existing claim-ledger fallback with
  the explicit `COHERENT_SUMMARY_MODEL_OUTPUT_INVALID` warning. Full and selected
  catalogs receive the same delivery floor. Do not accept any rejected prose or
  citations, add a new repair loop, or treat error-code text alone as provenance.
- Preserve valid prior drafts and valid siblings under the existing recovery
  rules. Verify fallback ledger claims independently, apply existing semantic
  guards and require final coverage before completion. Inadequate or unsupported
  ledgers must still fail without final artifacts.
- Preserve operational failure behavior: transport, runtime response identity,
  cancellation, invalid budgets, artifact/invariant failures, and non-context
  admission failures must not become successful model-output fallbacks. Existing
  context-overflow selection/fallback behavior stays unchanged.
- The warning's exact code, message, stage and allowed presentation must agree
  across authoring, runtime validation and persisted-artifact validation.

Revised non-scope: no prompt or output-limit change, new clipping acceptance,
source segmentation, new retry budget, preset promotion, automatic profile
routing, storage migration, Contract/Story model-output admission, or semantic
verification relaxation. Source-selection response failures remain outside this
General prose-generation fix. Draft #92 remains separate.

Verification: first reproduce the initial-invalid-output failure in the real
pipeline with a synthetic response and replay A's retained response privately.
Then prove A restores the prior disclosed 9/10 fallback with the normal verifier
and coverage gate. Cover malformed, clipped and below-limit unfinished output;
reject operational failures even if their error text resembles a model-output
error. Test insufficient/unsupported fallback, preserved valid draft behavior,
unchanged Story/Contract failure behavior and warning validation after reopen.
Run focused regressions, the affected summary suite, formatting and strict
clippy; repeat live A/B only after replay passes and GPU exclusivity is available.

## Recovery implementation and verification

Implemented in `7f387b5` after the contract-only `a0e6009` commit. The generation
owner now returns an explicit model-output rejection separately from operational
or application failures. Only General prose rejection selects the existing
ledger fallback. The warning is authored and checked through the same reason
mapping. Verification also enforces the General-only profile boundary.

Fail-first: `general_rejected_prose_requires_independently_verified_adequate_ledger`
failed with `MODEL_SUMMARY_RESPONSE_INVALID` before implementation. It now passes
for full and selected catalogs and still rejects an inadequate verified ledger.
Malformed, empty, foreign-reference, exact-limit and below-limit unfinished
responses are covered. Operational errors and application-owned schema corruption
remain fatal even when their codes resemble model-output errors. Story/Contract
behavior and prior valid draft retention are unchanged.

The affected summary suite passed: 214 tests, 10 live tests ignored. Formatting
and strict all-target/all-feature clippy passed. Test-only setup errors (an error
accessor, a draft-only fixture helper and private replay serialization) were
corrected; none is counted as the fail-first behavior proof.

A's private production replay reused its 19 retained pre-verification responses
and nine recorded real verifier verdicts, mapping verdict IDs only after exact
claim-text and quote equality. It restored the disclosed 9/10 fallback through
the existing verifier and coverage gate. Persisted artifacts matched after
independent database reopen. Request comparisons exclude per-run block IDs and
seeds; this replay is separate from fresh model verification.

| General document | Budget-only result | Fresh recovery result |
|---|---|---|
| A | Failed; no delivered summary | Disclosed verified fallback, 9/10 pages |
| B | Disclosed verified fallback, 14/14 pages | Same delivered fallback, 14/14 pages |

Fresh A returned identical text for all 19 pre-verification responses, including
the invalid draft; delivery changed because of the application fix. It made
20 total requests. Fresh B made 30 requests and also returned unfinished prose,
so this run used invalid-output fallback instead of the earlier run's
valid-but-undercovered draft. Neither fresh run delivered coherent prose.

Both fresh runs verified exclusive GPU ownership, actual 32k context and unchanged
input hashes, with the same 9B Q4_K_M, template, thinking setting, output budget
and acceptance wrapper as before. A took 150.61 seconds, B 163.90 seconds including
startup; no timing qualification is claimed. The original failed A attempt stays
in the evidence. No installed defaults or settings changed.

Recovery aggregate SHA-256: `dd0efba3a28a7bab5f22b81bcf38068eb300cf1264bb60f50570504d6dea1766`.
Private raw artifacts remain local; public evidence uses opaque document labels.

## Recovery cold diff audit

- `summary/coherent.rs:2941`: typed generation rejection chooses the existing
  fallback only for General. Runtime/invariant errors keep their original path.
- `summary/coherent.rs:3857`: rejection is tagged at response validation, not
  inferred from a caller-wide error-code match; budgets and operational failures
  default to the fatal variant. Existing bounded repairs are unchanged.
- `summary/coherent.rs:6360` and `:7651`: producer/runtime/persisted warning
  validation agrees on code, exact message, stage, version and presentation.
- `summary.rs:1207`: verification checks the profile boundary, then uses the
  existing independent ledger verification, semantic guards and final coverage.
- Regression tests at `summary.rs:11104`, `:11200`, `:11213` and
  `summary/coherent.rs:8729` cover both successful recovery and fatal boundaries.
- `docs/CONTRACTS.md`: the recovery invariant was committed before implementation.
- boundary-probe: valid prior drafts still pass; malformed/unfinished/foreign
  output recovers only through adequate verified claims; inadequate ledgers,
  operational failures, corrupted warning metadata and non-General admission fail.
- effect-trace: the typed result at the generation caller controls recovery;
  fail-first, saved-response replay and identical fresh A response text demonstrate
  that the edit changes delivery without accepting the rejected model prose.
- Concurrency: no shared state, queue, retry budget or model-runtime ownership
  changed. Local artifacts continue through the existing atomic completion path.

## Gap audit

NOT DONE for merge: implementation, deterministic checks, private replay and fresh
A/B production proof are complete; fresh CI and exact-head review are pending.
The observed delivery regression is fixed. A's missing page and source passage
omissions remain; this is fallback delivery proof, not coherent-prose quality
qualification. Contract admission, clipping repair improvements and preset/context
promotion remain separate. No production code changed after the tested commit;
this evidence update does not require repeating code tests.
