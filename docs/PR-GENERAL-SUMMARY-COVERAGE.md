# General summary coverage (#102)

## Root cause

Recorded production General summaries retained supported analysis evidence but
presented too few source pages. Both runs selected 16 source segments; generation
cited six and one respectively. Multiple source-selection requests are windows,
not proof of repeated catalog reduction. The selected page sets must be measured.
The desktop route disables the coverage policy used by Connect, so supported but
incomplete coherent output can be persisted as a completed result.

## Required change surface

- Reproduce the real failures offline through the production PDF and summary
  stages with recorded responses. Retain private inputs, response text, request
  comparisons and source-to-page mapping locally; publish only opaque labels and
  aggregate evidence. Do not put corpus names or identifying paths in Git.
- Isolate catalog availability, selected pages, generated citations and verified
  presented pages before choosing the repair. Write the evidence-based contract
  revision before production edits.
- Implement completeness handling at the existing shared summary seam, using
  the existing raw and omission-adjusted page accounting. Add synthetic
  regressions that fail before the fix, plus adequate-coverage and withheld-
  evidence controls. Preserve source/semantic validation and transactional final
  persistence. Reuse bounded repair or verified fallback rather than new workers.
- Rerun the same private documents through the production 9B profile after the
  fix with unchanged model, runtime, context, thinking policy and source bytes.
  Distinguish coherent-summary recovery, disclosed verified fallback, and failure.

## Explicit non-scope

No new infrastructure, model change, context increase, weaker acceptance gate,
unsupported citation, fake source coverage, hidden-ledger credit, schema/storage
migration, OCR/invoice/email work, or installed settings change. Story/Contract
profile behavior must not change without new evidence and a contract revision.

## Assumptions and blockers

A model can omit supported material; support alone does not prove completeness.
Raw coverage stays at 50 percent and adjusted coverage at 60 percent; technical
omissions remain in both denominators. Real document text and identities are
private. Live comparison requires the existing inference lock and an idle GPU.
The 9B preset remains unqualified until its acceptance gates actually pass.

## Verification plan

Record the original failing output, map selected IDs to pages with a no-inference
replay, then declare and run fail-first synthetic regressions at the controlling
seam. Prove good summaries remain coherent, undercoverage is handled explicitly,
semantic rejection cannot be undone, and insufficient verified fallback cannot
complete. Run focused summary tests, adjacent consumer tests, formatting and
strict clippy; CI owns duplicate broad suites. Retain before/after live receipts.

## Implementation summary

Investigation and regression plan only; no production change yet.

## Cold diff audit

Only this contract is introduced. Production prompts, routes and gates unchanged.

## Gap audit

NOT DONE: selected-page mapping, deterministic reproduction, source fix,
regressions, live comparison, current-head review and CI remain.

## Evidence-based contract revision

Offline replay through the production parser and summary stages reproduced both
failures with every request schema/system length/user length matching the saved
run. Real contract A selected six native pages and delivered four; real contract
B selected ten and delivered one. Both had sufficient selected coverage before
generation. No inference was run for this reproduction.

The fix is confined to summary.rs and summary/coherent.rs:
- General generation checks cited pages with the existing coverage owner. If a
  structurally valid draft is undercovered, use the existing single validation
  repair budget with explicit coverage feedback and available uncited pages.
  Initial prompts, selection, context and output ceilings remain unchanged.
- If coverage correction is exhausted, reuse disclosed claim-ledger fallback.
  That fallback must pass existing source and semantic checks. Other runtime,
  cancellation and invalid-output failures keep their existing behavior.
- General verification applies the existing verified coverage fallback when
  semantic withholding reduces coverage. Final General completion requires the
  same coverage gate as Connect, including fallback results; insufficient verified
  evidence cannot persist a completed summary. Historical artifacts stay intact.
- Story and Contract defaults remain unchanged. Connect retains its current
  routing, byte cap and coverage requirements.

Regression cases: adequate first draft (no repair), inadequate draft corrected
(one repair), repeated inadequate draft (disclosed fallback), verification loss,
and insufficient verified fallback (no completed artifacts). Reuse existing
coverage boundary tests and semantic guard tests. Retain source receipts locally.
