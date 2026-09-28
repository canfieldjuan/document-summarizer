# Final summary page disposition (#99)

## Root cause

Analysis omissions are source-bound stage decisions. Coherent synthesis builds
its own catalog from normalized source and can later recover a page. Current
coverage rejects any overlap with historical analysis omissions; GUI completion
does not use that coverage gate, while Connect can discard supported coherent
content. The office acceptance test also treats historical analysis omissions
as final omissions. No final reconciliation owner exists.

## Required change surface

- pipeline/summary.rs: one remaining-omissions calculation subtracts only pages
  actually referenced by the presented claims. Use it in existing page coverage
  gates, including delivered-prefix validation. Recovered material pages return
  to the adjusted denominator; technical omissions never reduce it.
- Keep analysis checkpoints, fingerprints, versions and inspection history
  unchanged. Do not erase old judgments or fabricate new analysis evidence.
- Final summary completion in both GUI and Connect computes presented citation
  pages, discloses recovered analysis omissions using the existing warning
  channel, then hashes/persists the resulting summary and citations normally.
  Evidence retained solely for an undisplayed ledger cannot recover a page.
- office_acceptance.rs: retain historical omission metrics and add final
  remaining/recovered metrics; evaluate disjointness and adjusted coverage on
  final disposition. Keep source, prompt, claim/evidence, raw coverage and
  adjusted coverage thresholds unchanged. This is a stage-accounting correction,
  not permission to lower a threshold or ignore a failed run.
- docs/CONTRACTS.md: clarify analysis versus final omission semantics before code.

## Explicit non-scope

No model/default/framing, prompt, semantic verifier, source-catalog selection,
analysis omission admission, schema/storage migration, new retry, background
queue, OCR, or model-host changes. No historical checkpoint rewrite. No lower
coverage threshold or treating unverified claims as recovery.

## Assumptions and blockers

Only already validated exact-source evidence can enter the final calculation.
A rejected/undelivered claim cannot clear an omission. Existing production
validation and transactional state-version fencing remain authoritative.
Live inference requires the existing harness lock and an otherwise empty GPU.
The 9B preset remains in separate PR100 and waits for this fix plus live evidence.

## Verification plan

Reproduce the recorded first-page no_substantive_content followed by source-
bound coherent recovery with an offline production-pipeline fixture. Before fix,
assert the final recovery disclosure / coverage behavior fails. After fix prove
GUI and Connect completion, preserved analysis bytes, valid artifact hashes,
remaining omissions on unrecovered/withheld pages, and no recovery from
undisplayed evidence. Boundary checks: empty, complete, partial recovery;
unknown pages; unchanged 50 percent raw / 60 percent adjusted limits; delivered
prefix excluding a recovered page. Run affected summary/Connect tests and lint.
CI owns broad platform suites. Then run the unchanged model prompts and fixtures
through the 9B acceptance lanes, reporting the corrected final accounting and
all failures. Do not claim the previously failed run passed retroactively.

## Implementation summary

The existing finalization path now adds recovery disclosure before calculating
summary and citation hashes. Presentation selection, cited-page collection and
remaining omissions have shared owners in summary.rs; coherent admission,
verification fallback, final completion and Connect delivered-prefix checks use
the same coverage rule. Only presented supported claims recover pages. Original
analysis, verification, source fingerprints and stage versions are preserved.

The office acceptance report keeps historical omission metrics, adds remaining
and recovered pages, and counts citations from presented claims rather than all
retained evidence. The original failed live run remains a failure. Its incorrect
historical-omission disjointness assertion is replaced by final-disposition
accounting; no prompt, fixture, claim/evidence or coverage threshold changes.

## Cold diff audit

- Required production edits are confined to summary.rs. No model, runtime,
  verifier, normalized-source admission, storage schema or dependency changes.
- Boundary-probe: recovered material pages reenter the denominator; incomplete
  recovery cannot inflate it. Empty, overlong, truncated and unknown-reference
  claim sets fail coverage. Technical omissions remain in the denominator.
  Undisplayed ledger evidence cannot count as a presented citation.
- Effect-trace: a source page omitted during analysis can later appear in the
  coherent summary; the shared final accounting recognizes only verified
  presented citations and emits recovery disclosure before integrity hashing.
  Offline production-stage fixtures prove General GUI and Story Connect
  recovery, General Connect non-recovery, and rejected recovery with no final
  artifact. Analysis survives byte-for-byte; persisted final hashes match.
- Concurrency/data integrity: existing state-version fencing and atomic final
  persistence are unchanged. The affected suite includes rollback of summary,
  citations, state and events, plus concurrent Connect admission and ownership.
- Security: model proposals still pass source and semantic validation before
  final accounting. This change does not give model output new authority.
- Verification: two declared fail-first regressions failed on current main;
  both passed after the fix. The affected summary/Connect provider suite passed
  (223 passed, 10 live tests ignored). Offline office checks passed (3 passed,
  3 ignored). Formatting and all-target/all-feature clippy passed. CI owns the
  broad platform suites; live inference is recorded separately.

## Gap audit

DONE: source fix and deterministic reproduction/verification.
NOT DONE for merge: exact-head review, required CI and live evidence pending.
PR100 remains separate and still requires its preset/profile/real-document gates.
