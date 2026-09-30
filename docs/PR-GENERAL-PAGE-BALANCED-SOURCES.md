# Page-balanced General synthesis sources

## Contract

Root cause: General synthesis offers a dense full source catalog whenever it
fits context. Source count is not page balance. The saved experiment selected
first/middle/last sources per page, preserving every offered page and source order.

Required change surface: version the General synthesis source selection policy;
select the first, lower-middle and last source per page, deduplicating positions
and retaining original bytes, request IDs, provenance and global source order.
The General synthesis request, response parser and verification grounding all use
the offered subset rebuilt by `source_catalog_for_profile`. Never discard a page
to fit context: if the bounded subset
cannot fit, use the existing fallback. Disclose selection using the existing
warning. Stored older artifacts reconstruct with their original policy/version.
Tests cover uneven page density, empty/single/multiple sources, exact identity,
determinism, real request construction and historical artifact replay.

Issue #71 clarification (comment 5913148917): synthesis citations are limited to
the offered subset. Both the schema's `source_ids` enum and the parser catalog
contain only selected IDs. Correction/repair prompts preserve that subset. A real
but unoffered source ID must fail `MODEL_SUMMARY_RESPONSE_INVALID`; offered IDs
must remain accepted. The existing implementation already enforces this boundary;
add an explicit two-sided regression before publication.

PR #109 review correction: the original test constructed the subset itself and
did not pin production's use of `source_catalog_for_profile`. Add a regression
that obtains the catalog through that production function, then exercises the
generation/parser path with a fake response citing an unoffered but real ID.
The offered-only response must pass. Replacing the function's final return with
`Ok(catalog)` must make the negative test fail. This is a test/documentation fix;
the production boundary is already correct and does not change.

Coverage denominators remain based on all textual pages in the normalized
document, with the existing non-substantive-page adjustment. They are not derived
from the selected source count. The new regression also checks that selection
preserves those pages and the existing coverage threshold.

Explicit non-scope: no short-unit paragraph or model-dependent repair; no prompt
wording, verifier, coverage threshold, model, runtime or other profile changes.
A short-unit prompt is a separate follow-up only if later evidence justifies it.

Assumptions/blockers: independent branch from current main. The deterministic
trim is separate. Any combined live proof identifies both source commits.

Verification plan: contract commit first, fail-first page-distribution/request
regression, focused version/replay tests, adjacent Rust tests, formatting and
strict Clippy. Live A/B and public control with unchanged qualification gates;
report delivered coherent/fallback counts, not only offered-page coverage.

## Implementation summary

Implemented as synthesis version 10.0.0. The shared catalog builder and prompt
builder select first/lower-middle/last per page and preserve exact identity/order.
The schema and response parser use the offered subset. A balanced catalog that
cannot fit goes to the existing verified fallback instead of model source selection.
Stored version 9.0.0 and earlier artifacts retain their original catalog policy.

Fail-first request regression showed all nine sources before and the expected five
sources across uneven pages after. Adjacent suite: 252 passed with one remaining
old preflight expectation; that focused test passed after correction. Historical
replay validated all 18 saved synthesis and verification artifacts. The explicit
unoffered-source/correction regression passes. Strict Clippy and formatting also pass after the final test-only addition.

## Cold diff audit

- `summary/coherent.rs::page_balanced_catalog`: deterministic per-page positions,
  deduplicated then filtered in original order. Source bytes, IDs and provenance
  are cloned without rewriting. Empty/single/sparse/dense and historical tests
  cover both selection directions.
- `source_catalog_for_profile` and `prompt_and_schema_for_version`: shared offered
  subset drives the actual request, schema and response parser. The negative test
  uses an omitted ID that is valid in the complete catalog; the positive side uses
  offered IDs. Correction builders keep the same segments and schema enum.
- `synthesize_with_delivery_coverage`: retain all offered pages, disclose reduced
  source count and use existing fallback on context overflow. No page is silently
  removed by the model selector.
- `summary.rs` version dispatch and runtime validation: accept current version and
  preserve the old policy for stored artifacts. The 18-artifact replay validates
  this boundary using saved data, without inference.

Effect trace: canonical catalog -> page-balanced offered subset -> matching prompt
and schema -> parser against that subset -> unchanged grounding and coverage. This
bounds density; it does not prove that a model will summarize every offered page.

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
