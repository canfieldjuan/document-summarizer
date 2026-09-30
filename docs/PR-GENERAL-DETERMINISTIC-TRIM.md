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

Implemented in `summary/coherent/trim.rs`, called before General clipping repair.
Recovery preserves retained bytes and complete siblings, checks original source
metadata, and re-parses the retained answer. A qualifier tail drops its whole unit.
Prior valid coverage drafts and window/framing siblings retain precedence over an
invalid correction. Trim warnings persist with claims. Verification uses the existing
strict source-pair verdict parser and downgrade seam; it never rewrites prose.

Fail-first proof: the original trim regression rejected the answer before recovery;
it now passes with one synthesis call. A second regression exposed a correction
that lost the original sibling; it now retains the safe original draft. Adjacent
summary tests: 256 passed, 11 ignored. Formatting and all-target/all-feature strict
Clippy pass at source commit `1b4948d`.

Offline replay used 18 saved raw answers from both earlier experiment shapes.
Nine unchanged answers reproduced coherent delivery. Nine clipped answers now
parse, but their changed verification requests lack saved verdicts: their semantic
and delivered outcomes remain unknown until fresh verification. This is not a
claim of nine additional successful summaries. Private replay output SHA-256:
`4c284b794485a316a8c4dd832cbe2251ab652ede11e9e91e27a29dce8c92b014`.

## Cold diff audit

- `summary/coherent.rs::generate_summary_with_coverage_repair`: intercept exact-ceiling
  General output after transport completion checks; preserve prior good drafts and
  siblings, then return deterministic recovery without a rewrite call. Both
  regression failures above exercise this controlling path.
- `summary/coherent/trim.rs`: canonical sentence boundaries, exact prefix,
  conservative qualifier withholding, metadata validation, warnings and per-source
  contribution verification. Boundary tests cover accepted/rejected ceilings,
  trailing whitespace, abbreviations/decimals, unsafe fragments and citations that
  belong only to discarded text; unchanged siblings sharing a source stay intact.
- `summary.rs::verify`: run the retained text through normal semantic checks plus
  the source-contribution seam before unchanged delivery coverage.
- `summary/quote_segments.rs::qualification`: visibility only, making the existing
  qualifier policy available to the trim; no change to source segmentation.
- Offline replay is an explicitly ignored, local-input test. No private documents,
  answers, paths, runtime credentials or installed settings enter this diff.

Effect trace: clipped answer -> deterministic recovery -> retained claim -> fresh
semantic/source verdicts -> unchanged coverage gate. Structural acceptance alone
is not treated as delivered quality.

## Live confirmation

Six production-flow attempts used isolated Qwen3.5-9B Q4_K_M at 32,768 context,
thinking off, GPU offload and the existing checksum-verified qualification
runtime. No installed setting changed. The shared inference lock, owned-server
checks, source/input checksums and actual context checks passed. Production system
prompts and 1,200-character ceilings were unchanged; no short-unit hint was added.

| Configuration | Contract A | Contract B | Public control |
|---|---|---|---|
| Trim only (`1b4948d`) | claim-list fallback, 10/10 pages | claim-list fallback, 14/14 pages | coherent, 5 cited pages |
| Trim + page sources (`1b4948d` + `46af52c`) | coherent, 8/10 pages | coherent, 14/14 pages | coherent, same 5 pages |

Every attempt made one synthesis call. In trim-only, A kept four units (two
trimmed), B kept two (one trimmed); every retained unit was an exact prefix of raw
output. Complete siblings were unchanged. Re-verification left supported prose
citing only pages 1-2, so the existing coverage gate selected fallback. The combined
run offered 28/74 A sources and 42/146 B sources. Its answers were already complete,
so that run does not exercise trim. A's omitted pages 8-9 remain disclosed.

Production seeds are derived from run IDs and differ between attempts. This is
one confirmation per configuration/document, not a same-seed causal experiment,
multi-seed qualification or human-rated quality result. The combined flow delivered
coherent prose here; no claim is made that trimming caused the difference or that
all inputs/models qualify. Offline changed-answer semantic results remain unknown.

The private aggregate checksum is
`02c9c0745def1b26b328262513ecfabcb10e760d5424125a0de5c63dddb5d11b`.
The later page commit `d63d04c` only adds the offered-citation boundary test; its
production source matches the tested `46af52c` exactly. Raw evidence stays local.

## Gap audit

NOT DONE for merge: remote CI and independent PR review remain. Implementation,
local regression/lint checks, offline replay and the scoped live confirmation are
complete. No further model repair or prompt experiment is included. These results
do not complete model qualification; contract A still lacks two cited pages.
