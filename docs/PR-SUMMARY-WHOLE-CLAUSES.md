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
