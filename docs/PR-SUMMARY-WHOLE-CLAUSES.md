# F2: whole contract clauses in General summary sources

## Contract

### Root cause

General synthesis reuses the analysis quote packer, which splits long source
units into 600-character sentence groups. Page-balanced selection can offer a
later group without the governing heading or opening words. A payment statement
can consequently lose the stage that governs it before drafting starts.

### Required change surface

- Add one source-owned clause reconstruction helper under summary/coherent.
  Read normalized page/block order and original byte ranges retained by F1.
  Preserve numbered clauses, including lettered lists within them, through the
  next printed clause boundary. Carry parent numbered headings and their opening
  text into child-clause context. Carry continuations across blocks/pages.
- For current General synthesis, offer each clause fragment intact as an exact
  source quote and its complete governing clause as application-derived context.
  Keep citation quotes within their original block; never fabricate a joined
  quote spanning blocks. Ordinary prose/forms retain existing segmentation.
- Apply page-balanced selection only after reconstruction. Include all supplied
  context in the existing serialized-request budget. If it cannot fit, use the
  existing disclosed fallback; do not truncate clauses or invent a new cap.
- Version synthesis as 12.0.0. Preserve the version 11 footer/balancing policy and
  earlier saved artifact validation. Analysis and verification versions stay put.
- Tests cover the actual serialized drafting request, exact-source provenance,
  parent lead-ins, cross-page continuation, footer exclusion, ordinary text,
  request-budget fallback, historical replay, and deterministic source order.

### Explicit non-scope

No structured extraction (#111), model/settings changes, new inference, output
schema/API/storage/UI changes, analysis rewrites, or verifier behavior changes.
F3 will use the same clause owner to supply verifier context and test the five
reported fidelity failure classes. This PR does not claim an accuracy gain.
The existing Contract and Story profiles retain their source-selection policy.

### Assumptions/blockers

Normalized source order and printed clause markers are the available evidence;
this does not reconstruct clauses from layout geometry or infer missing text.
Ambiguous unnumbered prose retains the existing policy. User review/CI precede
merge. F3 starts from merged main, then fresh A/B General accuracy runs precede
review of #111; #100 remains frozen.

### Verification plan

1. Fail first: a long numbered payment clause must appear whole in the General
   request, including its heading and opening words. Current sentence packing
   must fail that assertion.
2. Exercise both sides: whole clause and faithful continuation preserved;
   unrelated following clause/footer excluded from its context. Parent and child
   headings, multi-block order, empty/ordinary source, and oversized request.
3. Replay saved private A/B normalized sources and historical artifacts locally;
   report source/context coverage without publishing private text. No inference.
4. Run adjacent summary tests, cargo fmt, strict Clippy, and cold diff audit.
   CI owns duplicated platform suites.

## Implementation summary

General synthesis 12 builds numbered clauses from F1's retained source ranges,
then applies the existing page-balanced selection. Exact quotes stay page/block
local; parent lead-ins and cross-page continuations are supplied as `full_clause`
source data. Both prompt serialization paths carry that context, and request
budget accounting includes it. Version 11 reconstruction and disclosures remain
accepted. No verifier, output schema, runtime or extraction behavior was changed.

The fail-first request test failed with `whole clause missing from drafting
request`, then passed. Six focused tests cover the whole long clause, cross-page
parent context with lettered conditions, exclusion of the next clause/footer,
ordinary forms and historical catalogs, complete serialized context budgeting,
same-page block order, and version-11 disclosure reload/tampering.

Verification performed before publication:
- Adjacent summary suite: 272 passed, 13 opt-in tests ignored.
- Six focused whole-clause regressions passed.
- Saved private A/B replay passed; historical artifacts remain valid. A offers
  26 excerpts across 10 text pages; B offers 42 across 14. No detected footer
  excerpts remain. The B payment-stage context check passes.
- Rebuilt A/B requests contain 47,579 / 47,713 characters including framing and
  schema, within the existing 32k profile character budget. This is not a live
  tokenizer admission or inference result.
- Formatting, strict all-target/all-feature Clippy and diff checks passed.

## Cold diff audit

- `summary/coherent/whole_clauses.rs`: one per-document clause owner, preserving
  normalized block order and original fragments. No shared mutable state or I/O.
  Numbered units, parent prefixes and cross-page controls exercise this path.
- `summary/coherent/page_furniture.rs`: expose retained ranges from the existing
  filter; no second footer policy. Existing footer/operative-text tests pass.
- `summary/coherent.rs`: current General catalog and serialized source context,
  historical-version admission and regression/replay tests. The offered catalog
  still controls the source-ID enum and response parser. No new source authority
  comes from model output.
- `summary.rs`: synthesis version and historical dispatch compatibility, plus
  the exported-version expectation. Analysis and verification versions unchanged.
- `docs/CONTRACTS.md`: current version, full-clause source semantics and F3 boundary.
  This document records the contract and evidence; no other module changes.

boundary-probe: long clause and child/cross-page lead-ins survive; unrelated next
clause/footer excluded; original-block quote provenance retained; ordinary forms
unchanged; over-budget context triggers the existing fallback decision; historical
disclosure accepted and forged disclosure rejected. Existing unoffered-source
rejection tests remain passing.

effect-trace: retain governing words in the drafting request | source segmentation
before balanced selection and prompt serialization | failing-before whole-clause
request regression passes, and saved B payment context now includes its stage.

## Gap audit

NOT DONE for merge until exact-head CI and independent review pass. Local F2
implementation and planned offline verification are complete. F3 must still add
full-clause verification and fidelity rejection tests; fresh A/B accuracy remains
unmeasured. Unnumbered/ambiguous prose retains its prior segmentation. #111 stays
held and #100 stays frozen.

## Contract revision: one context per governing clause

New evidence: #71 comment 5921835832 and #114 thread 4150488007 require one
serialized context per distinct governing clause. At c913fd2, both source prompt
builders clone `full_clause` into every offered fragment. Several fragments of
one clause therefore pay the whole-context request cost repeatedly.

Revised root cause: context belongs to a request-level clause table, while the
current serializer treats it as independent per-segment text.

Required surface: the two prompt builders in `summary/coherent.rs` must share
one deterministic context-interning helper. Serialize distinct full contexts once
in `clause_contexts`; each applicable offered segment references its
`clause_context_id`. Context IDs are not citable source IDs. Request budgeting
uses the resulting full serialization as before. Empty context tables are omitted.
Update the General instruction and canonical source contract for this input shape.

Non-scope: clause reconstruction/selection, citation provenance, response schemas,
verifier behavior, model settings, persisted artifacts, and F3 remain unchanged.
Identical context strings may share a table row; differing conditions must not.
Tables are local to each request, including source-selection windows.

Verification: fail first with three actual clause fragments and one expected
serialized context. Prove all fragments resolve to the complete context, distinct
contexts stay distinct, unused contexts are not emitted, and context IDs fail the
existing response parser. Exercise drafting and selection builders, no-context
requests, and the existing over-budget test. Replay A/B with total references,
distinct emitted contexts, request size and 32k budget fit. Run adjacent summary
tests, formatting, strict Clippy, and a cold diff audit. No inference.
