# Page-balanced General synthesis sources

## Contract

Root cause: General synthesis offers a dense full source catalog whenever it
fits context. Source count is not page balance. The saved experiment selected
first/middle/last sources per page, preserving every offered page and source order.

Required change surface: version the General synthesis source selection policy;
select the first, lower-middle and last source per page, deduplicating positions
and retaining original bytes, request IDs, provenance and global source order.
Keep the complete canonical catalog for grounding; the selected subset belongs to
the synthesis request. Never discard a page to fit context: if the bounded subset
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

Pending implementation and evidence.

## Cold diff audit

Pending.

## Gap audit

NOT DONE: implementation and verification remain.
