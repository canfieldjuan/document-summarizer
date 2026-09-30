# F1: exclude page furniture before balanced summary selection

## Root cause

General synthesis builds quote candidates from the whole normalized page, then
spends first/middle/last slots on candidate positions without excluding running
page furniture. Mixed body/footer candidates can also carry publisher boilerplate.
The saved fidelity review found footer-only excerpts consuming the offered budget.

## Required change surface

- One source-range filter for recognized repeated marginal publisher notices,
  running header/footer labels and page-number-only lines. Never rewrite source
  text or infer a missing clause. Keep operative numbered headings and repeated
  prose; repetition alone is not grounds for removal.
- Apply that filter before quote packing and General page-balanced selection.
  The prompt, schema and parser use the same filtered catalog. Preserve original
  page/block provenance and deterministic identities.
- Version the synthesis policy; historical synthesis 10 and its stored results
  reconstruct the old catalog. Analysis behavior, Story and Contract stay unchanged.
- Preserve representation of every page with substantive quotable text. If all
  source on a text page is excluded, fail explicitly rather than silently treating
  the document as fully represented or inventing a source excerpt.

## Explicit non-scope

No structured extraction, clause regrouping, full-clause verifier change, semantic
condition guards, model/preset/settings changes, inference, new wire format or
storage migration. Those are separate F2/F3 work. PR111 remains draft on hold;
PR100 remains frozen. Private source text and customer names must stay local.

## Assumptions/blockers

Text order can place PDF footers before the body. Recognize bounded notice ranges,
not entire page tails. A line cannot be removed merely because it repeats; use
marginal position, label/notice shape, page identity and cross-page repetition.
This is not a universal PDF-layout classifier or removal of operative boilerplate.

## Verification plan

First reproduce footer admission with a public synthetic production-catalog test.
Prove positive/negative boundaries: multiple pages retain body content and offered
IDs; repeated labels and page numbers are excluded; operative prose, numbered
headings and unique copyright survive; a notice cannot consume the adjacent body.
Replay the saved A/B source catalogs and historical artifacts without inference.
Run adjacent summary tests, formatting and strict Clippy. CI owns broad duplicated
suites. Fresh A/B inference and private fidelity review follow F1-F3 together.

## Implementation summary

Pending.

## Cold diff audit

Pending.

## Gap audit

NOT DONE: implementation, regression proof and review pending.
