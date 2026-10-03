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

Implemented in `80613f4`, following contract-only `3bc8e6b`:

- Origin: `src-tauri/src/pipeline/summary/coherent/page_furniture.rs:256`.
  The page reader keeps original byte offsets across blocks, admits bounded
  metadata candidates by edge position, then counts distinct pages.
- Current recognition is centralized at `page_furniture.rs:172`; old
  first/last-line label recognition and digit-erasing keys run only under
  `Policy::Original`. Copyright recognition remains shared.
- General synthesis13.0.0 and Contract extraction3.2.0 use the new policy.
  General11/12 and Contract3.1.2 reconstruct the old policy and exact warning.
  Contract source-ID recipe3.0.2 remains unchanged.
- The existing disclosure code now reports partial-page removal under policy2.
  Coverage subtracts only the source-backed set of wholly excluded pages.

## Reproduce, isolate, explain, fix, prove, prevent regression

1. Reproduce: at contract-only `3bc8e6bee8dff9b3d186ea55ac64739993cc54f6`,
   `cargo test --manifest-path src-tauri/Cargo.toml --locked --lib
   furniture_origin_reproduction -- --nocapture` produced 0passed/5failed.
   All failures showed metadata still present in retained source bytes.
2. Isolate: public fixtures cover filing labels, combined running headers,
   date footers, multi-line margins, and typed EDGAR headers. Failure starts
   in furniture admission, before clause construction or category selection.
3. Explain: the old owner recognizes only selected metadata at the outermost
   lines; unsupported margins pass through as clause text or uncertain headings.
   This is an existing owner limitation, not a key-term alias defect.
4. Fix: recognize those forms at the owner and pass its retained byte ranges to
   both consumers. No Contract-level compensation filter or source re-scan.
5. Prove: those same5 tests pass. Final owner tests10passed/0failed; adjacent
   furniture tests20passed/0failed/1ignored; full library all-features714passed/
   0failed/20ignored. Strict all-target/all-feature Clippy, format and diff checks
   pass. The final edit moved the test module below runtime items for Clippy;
   final owner tests and Clippy were rerun after that move.
6. Prevent regression: committed negatives retain operative clauses, governing
   headings, unique/minority/position-changing metadata, distinct filing IDs,
   and numbers outside admitted counter fields. Tests cover edge-band limits,
   invalid counters, empty/single-page inputs, mixed content, cross-block UTF-8
   byte ranges, disclosure/coverage and historical policy dispatch.

Test scaffolding corrections during implementation: missing imports, CRLF
fixture construction, and Clippy's test-module placement rule. These were my
mistakes; none was a production fix prompted by a new corpus example.

## A-AH reconciliation

Saved normalized/chunked inputs from34 prior cases were replayed through current
analysis, synthesis, verification, citations and Connect delivery. All34 saved
Contract3.1.2 analysis/synthesis/verification artifacts validate under the
historical policy. Every new clause reconstructs from original source ranges;
all render units deliver and coverage passes. No model is invoked. This is a
saved-source regression, not fresh PDF ingestion or an unseen evaluation.

There are17 changed category slots, no lost selections, and26 changed source
inventories. Every change is recorded in the private source adjudication.

| Aliases | Category change | Explanation |
| --- | --- | --- |
| I, M, P, Q, S, T, W, Z, AB, AG | Parties | Typed EDGAR record removed; substantive opening unchanged. |
| L | Parties; payment | EDGAR opening cleanup; repeated filing label removed from payment continuation, both sides retained. |
| AE | Parties; liability/indemnity | EDGAR opening cleanup; complete sections4 (including4.1-4.3) and10 (including exception) become selectable after marked footer removal. |
| AF | Parties; insurance; liability/indemnity | EDGAR opening cleanup; date footer removed inside complete article24, preserving both sides. |

All other slots are unchanged. Source inventory changes consist of metadata
removal, plus two N title lines restored because they do not meet strict-majority
same-position admission. AF loses eight metadata-only numbered/date clauses,
changing46clauses to38. A-G source inventories and selections are unchanged.
The AE gain is locally source-checked; independent review and combined v5 remain
required. Remaining unsupported heading forms belong to PR-B.

## Evidence aliases and hashes

Evidence is durable outside worktrees. Only aliases and SHA256 appear here:

| Alias | SHA256 |
| --- | --- |
| A-fail-before | 039667828bbc6a055274100398ab7e65e6b1826ed0aa803816ec6d923133df5d |
| A-pass-after | 9d96eadf96b6005202f32dc3cfc5cc948208ef326c2bb2ee9d406040b7ebae7e |
| A-full-library | 5f8369450f5ca6fc2f2f1fae639867a7f8d6a78acb9051d6498f941167e1ee15 |
| A-strict-Clippy | cb9d57e86c6053b078d3dab27aa81d2ad7c58480cd8a355229bcb64f9fa86974 |
| A-regression-input | bc0c1c2aad2204caa5e3cf12e2ec2eac143c33cb6239e450604d8e754cef5a37 |
| A-regression-replay | b23b5535df15c7d4b0b43cfcc40026ced0209a23c855f179db4e96efafa103c4 |
| A-selection-diff | ab60b5cb646150964f66e48c6f569844aecc105c68f7cbfe92fc1f663eef5670 |
| A-source-adjudication | 0b7da8d7ecbf801600b56842b7505849f73bba4a0e99cf6192488b644de7a150 |
| A-implementation-fingerprint | ecf6216e52c4f1deb70988cfd2a680b15fb6693d3b3cdeee9fa17b51517dcaec |

## Cold diff audit

`boundary-probe: metadata removal and source preservation both exercised;
minority/unique/mixed inputs retained; page-band and numeric counter boundaries
passed; downstream General/Contract use owner-produced byte ranges; forged
warnings/version relabeling rejected; no new default fallback or shared state.`

`effect-trace: remove running metadata from source sections | Furniture retained
byte ranges control General catalogs and Contract records | five fail-before
fixtures pass after; A-AH inventory/selection diffs account for all changes.`

The diff adds version dispatch and historical reconstruction in addition to the
owner change. It does not add filesystem/network effects or shared mutable state;
existing concurrent pipeline tests remain green. Customer-visible source
removal is the relevant risk and is covered by the positive/negative probes and
source adjudication. Independent review must still challenge the policy.

## Gap audit and stop condition

DONE: public reproductions; owner fix; special-case inventory; historical/current
regression; A-AH source reconciliation; local tests/Clippy/format.

NOT DONE: exact-head CI, independent review, PR-B heading implementation, and the
single fresh combined v5 qualification. Keep this PR draft. Freeze this owner
policy at the recorded implementation fingerprint; do not tune it on v5.
