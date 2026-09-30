# General deterministic clipped-section trim

## Contract

Root cause: General synthesis rejects ceiling-clipped prose before semantic
verification. The existing repair asks the same model to rewrite the answer,
including complete siblings. It cannot guarantee preservation. The approved
policy is deterministic truncation at a complete sentence, not model rewriting.

Required change surface:
- `summary/coherent.rs` and a focused trim module: intercept General outputs at
  the admitted character ceiling, including trailing whitespace. Validate raw
  shape and source membership before recovery. Reuse the existing sentence
  boundary and qualification owners. Retain an exact prefix ending at the last
  complete sentence; never invent punctuation or reword text. A cut qualifier
  (unless, except, provided, subject to, notwithstanding, however, but, etc.)
  withholds its whole unit. A unit with no complete sentence is dropped.
- Preserve every valid complete sibling in application code, for windowed and
  unwindowed catalogs. No synthesis repair request for a clipped answer, even
  when nothing survives; use the existing safe fallback. Other profiles and
  non-clipping repairs are unchanged.
- Record durable trimming/withholding warnings. Re-run ordinary grounding,
  semantic fidelity and final page coverage on retained text. For trimmed units,
  use the existing per-source material-coverage verification seam to check that
  every cited source still contributes a material fact. A non-contributing or
  uncertain source withholds the unit; it never inflates delivered coverage.
  Complete siblings keep their text and sources. No generation prompt, model,
  1,200-character ceiling, runtime setting or public API changes.
- Tests cover complete/clipped/empty-prefix/qualifier/malformed cases, whitespace,
  abbreviation and decimal boundaries, mixed sources, preserved siblings, no
  synthesis retry, warnings, verifier rejection and delivered coverage.

Explicit non-scope: no replacement-only model repair, new model, prompt-length
experiment, invoice/email work, storage migration or relaxed qualification gate.
The abandoned ab7ee1f plan and its implementation remain local and unpublished.

Assumptions/blockers: saved verifier verdicts apply only to identical requests.
Replays cannot label a changed claim supported by reusing an old verdict. Offline
proof reports deterministic outcomes and missing semantic verdicts separately;
then one live confirmation supplies fresh verification. Private inputs stay local.

Verification plan: fail-first production regression; adjacent Rust tests;
formatting and strict Clippy. Replay saved A/B answers from page-balanced plus
short-unit and page-group experiments before fresh inference. Report counts for
trimmed, withheld, structurally accepted and actually delivered coherent/fallback
results, with unknown semantic outcomes explicit. Live confirmation follows the
existing exclusive-runtime harness; no installed settings change.

## Implementation summary

Pending implementation and evidence.

## Cold diff audit

Pending.

## Gap audit

NOT DONE: implementation, offline replay, and live confirmation remain.
