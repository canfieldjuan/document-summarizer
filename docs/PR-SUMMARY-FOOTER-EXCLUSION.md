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

Implemented General synthesis 11 with a source-range filter before quote packing
and page-balanced selection. The shared catalog builder owns identities and
original provenance; the filtered catalog feeds the production prompt, strict
schema, parser, count disclosure and current-artifact reconstruction. Synthesis
10 keeps its former reconstruction. No extraction or verifier changes landed.

Fail-first `balanced_selection_excludes_running_furniture_and_keeps_each_page`
failed with `footer entered balanced excerpts`, then passed. Inverse tests retain
operative copyright conditions and governing headings, handle footer-before-body
PDF text and missing blank separators, preserve historical/Story/Contract behavior,
and reject a wholly excluded page instead of silently losing it.

Verification:
- `cargo test --offline --lib pipeline::summary::`: 264 passed, 13 opt-in ignored.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --offline --locked --all-targets --all-features -- -D warnings`: passed.
- `npm run build`: passed (unchanged frontend assets required for Tauri tests).
- `git diff --check`: passed.
- Saved A/B replay: historical artifacts validate; A offers 28 excerpts over 10/10
  text pages with zero detected footer excerpts. B offers 42 excerpts over 14/14
  text pages; footer-containing excerpts fall from 14 to zero. This metric counts
  mixed footer/body excerpts as well, unlike the earlier footer-only count of 13.
  The prompt and source-ID enum match the offered catalog. No inference occurred.

## Cold diff audit

- `summary/coherent/page_furniture.rs` (`Furniture::new`, `segment`): pure,
  per-document source ranges; repeated notice/label detection and exact page
  counters. No source mutation, shared mutable state or model instructions.
  Public positive/negative tests cover the actual catalog and disclosure boundary.
- `summary/coherent.rs` (`source_catalog_for_profile`, `validate_content`,
  `page_balanced_catalog`): versioned General dispatch, canonical evidence
  reconstruction, consistent count/prompt/schema/parser authority. Story and
  Contract keep their former source policy; saved A/B artifacts replay.
- `summary.rs` (`derive_quote_catalog_with_segmentation`, version dispatch):
  reuse the original identity/bounds owner with a source-segmentation callback;
  analysis callers retain their exact old segmentation. Version 10 stays admitted.
- `docs/CONTRACTS.md`: current source policy and synthesis version; no consumer,
  presentation, storage or runtime contract change.

boundary-probe: publisher footer excluded / operative copyright preserved;
numbered and substantial-completion headings retained; every saved A/B text page
represented; wholly furniture page rejected; historical artifacts accepted;
unoffered-citation regressions remain passing.

effect-trace: footer-free offered excerpts | source-range filtering before quote
packing and balanced selection | failing-before synthetic production-catalog test
and saved A/B replay show the changed excerpts without losing observed pages.

## Gap audit

DONE for local F1 implementation and deterministic proof. NOT DONE for merge:
required CI and independent review pending. F2 clause preservation, F3 verification
of the five failure classes, fresh A/B inference and private fidelity re-review
remain separate required work. PR111 stays draft and its hold unresolved. PR100
stays frozen. This filter does not claim to recognize every marginal layout or
improve model semantic accuracy by itself.
