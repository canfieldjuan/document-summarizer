# PR-A: versioned page furniture at the source owner

## Root cause

`summary/coherent/page_furniture.rs` already excludes exact page counters,
several running labels, and repeated copyright notices. Issue117's claim that
it handles only notices is incomplete. Admission currently inspects only the
first/last nonempty line in the first/last block. Filing labels, document/date
footers, combined page labels, and adjacent repeated footer components remain
source text. Repeated keys also erase all digits rather than just page counters.
The owner does not distinguish historical furniture policies.

## Required change surface

- Keep the owner in `page_furniture.rs`, returning retained original byte ranges.
  Read page lines with their actual page-edge positions across blocks.
- Inspect a bounded four-nonempty-line band at each page edge. A repeated label
  must occur at the same edge position on at least two and a strict majority of
  text pages. Recognize filing labels, short document/page/date running labels,
  and repeated short footer components adjoining a recognized document/date
  footer. Repetition alone never deletes arbitrary body text.
- Normalize only admitted page-counter fields for repetition; retain filing IDs,
  dates and other numbers as content. Preserve operative lines, numbered clause
  headings, unmarked governing titles, and unique ordinary lines.
- A syntactically explicit EDGAR document-header record at the start of the
  document is filing metadata and may be excluded even when unique. This is the
  narrow exception to retaining unique ordinary source lines; document titles,
  party openings and ordinary exhibit references remain subject to safeguards.
- Preserve copyright-notice detection and all historical policy behavior.
- Use the existing furniture warning code for source-backed disclosure. Under
  the new policy it may disclose removals even when no whole page is excluded;
  coverage subtracts only actually furniture-only pages. Historical warnings
  retain their exact text and validation behavior.
- Version General synthesis and deterministic Contract extraction independently
  from their source-ID recipe. Historical artifacts reconstruct using the old
  owner policy; unchanged source ranges retain their identity.

## Earlier special cases and cleanup

The old notice detector, exact page counters, confidential/draft labels, pipe
page-suffix labels, uppercase document-title labels, digit-erasing repeat keys,
and first/last-line admission are inventoried here. The new edge reader owns
current line/position admission and page-counter canonicalization; remove the
redundant current-path checks instead of stacking downstream filters. Retain
legacy behavior only behind the explicit historical policy. No Contract-level
text re-scan or post-selection furniture filter is added.

## Non-scope

No heading grammar/alias change (PR-B), model prompt or generation change,
verifier redesign, schema/wire/storage migration, frontend change or dependency.
Do not fix unrelated known full-clause numbering ambiguities. Do not tune v5.

## Assumptions and blockers

Policy decisions in PR111 comments5964081366 and5964091129 request separate
follow-ups against main after PR111 merge. Recognition stays conservative where
metadata cannot be distinguished from operative text. A gain is accepted only
when the resulting source extent is correct; unexplained changes block lock.

## Verification plan

1. Fail-first public cases: repeated filing labels, combined page/date header or
   footer, multi-line margins, and EDGAR metadata. Assert removal by original
   byte ranges and complete retained operative text.
2. Negatives: repeated operative prose, body exhibit references, numbered and
   governing headings, unique labels/titles/dates, minority/position-changing
   repeats, one-page inputs, and counter/ID distinctions.
3. Exercise General catalogs and Contract selection plus warning/coverage;
   historical General and Contract artifacts reconstruct with the old policy.
4. Run adjacent tests, format/Clippy; full library suite for version dispatch.
5. Replay A-AH from saved normalized/chunked evidence, reporting every selection
   and source-range change with source adjudication. Lock only after explanation.
6. Fresh v5 remains held back until both PR-A and PR-B locks are verified.

## Implementation summary

Pending. First commit contains only this contract.

## Cold diff audit

Pending implementation and proof.

## Gap audit

NOT DONE: public reproductions, implementation, historical/current regression,
A-AH reconciliation, independent review and lock remain.
