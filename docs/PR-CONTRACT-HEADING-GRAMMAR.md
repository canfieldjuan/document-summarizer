# PR-B: Contract heading grammar at the clause reader

## Root cause

`summary/coherent/whole_clauses.rs` already admits decimal markers with an
optional section sign. It does not strip the explicit Section/SECTION keyword,
and its ARTICLE grammar requires the ordinal and title on one line. These
layouts reach the existing heading-like uncertainty path, causing abstention.
The term aliases are not the cause and will not change.

## Required change surface

- Extend the Contract line reader with Section/SECTION followed by the existing
  decimal marker grammar. Preserve section-sign support, decimal children,
  original byte ranges, parent/peer scope, and tab-as-whitespace recognition.
- Admit uppercase ARTICLE plus a canonical ordinal on one line, followed by an
  uppercase title on the next nonempty line of the same page. Lines may occupy
  separate blocks. Both lines belong to one article source heading. Cross-page,
  missing-title, mixed-case-title and malformed-ordinal cases remain uncertain.
- Use the existing inline ARTICLE ordinal grammar once for inline, split, and
  bare-Roman recognition. Remove the artificial ARTICLE/TITLE string round-trip
  used to validate bare Roman ordinals. Do not introduce a parallel number parser.
- Keep the existing three-way line classification, unit-boundary checks,
  TOC handling, parent tracking and boundary_uncertain signal. Selection consumes
  that signal; no text re-scan or alias changes downstream.
- Version Contract extraction3.3.0 with the expanded grammar. Saved3.2.0 uses the
  old grammar with new furniture; saved3.1.2 uses the old grammar and old furniture.
  Preserve the existing source-ID recipe. General and the PR-A owner stay frozen.

## Earlier rules and cleanup

Retain the single decimal-marker parser, component-length admission, ordinal
canonicalization, whole-unit admission, leading-title handling and uncertainty
propagation. Remove the synthetic ARTICLE/TITLE call for Roman validation.
Section/ARTICLE look-alikes remain the fallback for unsupported forms, not an
extra filter over new recognized headings. No category-specific patch is added.

## Non-scope

No new term aliases, inline lower-case prose inference, heading reconstruction
across page breaks, TOC redesign, full-list suppression, model/UI/schema change,
furniture-rule revision or v5 tuning.

## Assumptions and blockers

Authorized by PR111 comments5964081366/5964091129. This branch depends on frozen
PR118 at1a0eaabae330ffee73067e69ff7240b13c83804d and targets main. Its unique first
commit is contract-only. Review the PR118 dependency and this grammar increment
separately; after PR118 merges, the main diff contracts to this increment.

## Verification plan

1. Fail-first public Section parent/child and split-ARTICLE fixtures, checking
   actual selected source extents and complete retained source lines.
2. Negatives for malformed/oversized numbers, prose references, missing/mixed-case
   titles, TOC entries, split blocks/pages, continuation boundaries and nearby
   peers. Clean existing decimal/section-sign/inline ARTICLE forms stay correct.
3. Old3.1.2 and3.2.0 artifacts retain their exact behavior; unknown or forged
   versions are rejected. No alias or furniture-fingerprint change.
4. A-AH saved-source regression against both prior3.1.2 and frozen PR-A3.2.0;
   explain every inventory/selection change and review all newly selected extents.
5. Adjacent tests, strict Clippy and formatting. Freeze the combined candidate
   for independent review and the operator-supplied fresh v5; zero wrong labels.

## Implementation summary

Implemented in `eef572d`, following contract-only `8797aad` on frozen PR118.
Only `whole_clauses.rs` and `contract_extraction.rs` change in this increment.

- `whole_clauses.rs:237` admits the explicit Section/SECTION prefix through the
  existing decimal parser. `ReadLine::read` records recognition once.
- `whole_clauses.rs:328` owns the existing canonical ordinal grammar shared by
  inline/split ARTICLE and bare-Roman recognition. The synthetic ARTICLE/TITLE
  parsing call is removed.
- Adjacent same-page ARTICLE/title fragments become one heading. Their original
  bytes and fragment order are retained, including cross-block headings.
- `whole_clauses.rs:654` distinguishes wrapped sentence text from an uncertain
  named-heading shape before allowing the new forms to open source sections.
- `whole_clauses.rs:767` marks uncertainty when bare numbering would close an
  explicitly marked section: those numbers may be an internal list. Inventory
  stays complete; selection consumes the existing uncertainty flag.
- Contract3.3.0 uses the expanded grammar. Versions3.2.0 and3.1.2 reconstruct
  their respective historical grammar/furniture combinations. Source-ID recipe
  remains3.0.2. No aliases, selection algorithm or General/furniture code changed.

## Reproduce, isolate, explain, fix, prove, prevent regression

1. Reproduce: at contract-only8797aadda3863140efe3827a123c4f32fb7e072e,
   the two `heading_origin_reproduction` tests failed with empty payment
   selections. Both public layouts have explicit complete source sections.
2. Isolate: `decimal_line` lacks the Section keyword; `article_title` requires
   the ordinal/title on one line. Existing uncertainty therefore prevents labels.
3. Explain: heading admission, not aliases, loses the structure. Expanding that
   admission also requires preserving sentence and numbering-scope certainty.
4. Fix: extend the reader, retain original fragments, carry its uncertainty to
   the unchanged selector, and preserve historical policies by version.
5. Prove: both originals pass. Contract tests43passed/0failed/1ignored; adjacent
   General tests7passed/0failed; strict all-target/all-feature Clippy, format and
   diff checks pass. Saved-source A-AH replay passes, including68 historical
   analysis/synthesis/verification artifact sets across both earlier versions.
6. Prevent regression: public tests cover clean decimal/section-sign forms,
   parent/child scope, tabs, malformed numbers, TOC entries, missing/mixed-case
   titles, cross-block/cross-page splits, unfinished prose, original byte ranges,
   mixed numbering, unknown versions and historical behavior.

### My regressions during implementation

- **Wrapped references:** the first draft promoted a wrapped Section reference
  before considering the preceding unfinished sentence. The adjacent test
  exposed a false withdrawal. A named-reference probe then exposed false
  termination labels for both Section and split ARTICLE. Its failure log names
  both inputs. The origin now uses three-way classification: sentence prose
  continues as text; a named-heading shape in that position stays uncertain.
- **Mixed numbering:** initial AC replay selected the fee introduction while
  bare1/2/3 list items with prices were detached as peers. The public
  `heading_grammar_mixed_numeric_scope_abstains` failed in exactly that way.
  The scope owner now marks the ambiguous boundary; the same public test passes
  and final AC/payment is not identified. Every price remains in the full list.

These were defects in my unpublished draft, not pre-existing behavior. Their
failed logs and patches are retained. Final proof uses the corrected replay,
not the earlier replay that exposed AC's unsafe selection.

## A-AH reconciliation

This is saved normalized/chunked-source regression, not fresh PDF ingestion or
an unseen test. All34 current outputs reconstruct from source and deliver all
render units with coverage; both saved versions replay exactly for all34.

Against frozen PR-A3.2.0 there are6 changed category slots:

| Alias | Category | Observed change and explanation |
| --- | --- | --- |
| N | Payment; termination | Both now abstain. New split ARTICLE headings in the TOC match these categories and are uncertain, activating the existing whole-category abstention rule. Valid body sections remain in the full list. |
| X | Parties | New first-article boundary enables the existing opening-between rule; named parties and complete introduction/recitals retained. |
| X | Insurance | Adds complete ARTICLE VIII and Section8.1, including approval/rate conditions for alternate coverage. Existing property-insurance selection retained. |
| X | Liability/indemnity | Adds complete ARTICLE IV with Sections4.1-4.3, reciprocal duties, exceptions, procedures and survival; ends before the next article. |
| AC | Liability/indemnity | Selects complete Section9, which incorporates ExhibitA. Cross-references are not resolved into additional selected text. |

AC/payment remains not identified after the mixed-numbering correction. All
other category slots match PR-A. Newly recognized boundaries change clause
counts only in N59to69, X42to87, AC21to33 and AD4to11. AD gains no label. There are
zero added or removed source lines versus PR-A.

Against PR1113.1.2, the combined candidate changes23 category slots: the17
explained PR-A changes plus these6. The123 removed metadata lines and2 restored
minority title lines are exactly PR-A's source change, independently reviewed
there. Heading work adds no source removal. Gains here are locally checked;
independent PR-B review and v5 qualification remain pending.

## Evidence aliases and SHA256

Durable evidence is outside worktrees. Public logs and private corpus artifacts
are cited by alias/hash only:

| Alias | SHA256 |
| --- | --- |
| B-origin-fail | fcff256805a338405bce5b30b41512b58fe630a623158df4903630acf9633444 |
| B-wrapped-reference-fail | aa0612e319cae9981fc058531dd59d3c4097160eea0911ccc919cda5965251e1 |
| B-numeric-scope-fail | 7c638ded4437c634bdcdaaa50439f167244f96e034baf7709558f26d3713bcf4 |
| B-final-Contract-tests | 2425e7ee7df7a3d01f9e253c4bb13c1279c4be46f7934592684542320dc49801 |
| B-strict-Clippy | e13175ca4d1500e059470d30785776b8e77b7244a16ef5918d9002ebca8681fb |
| B-regression-input | 4b13440014cf094a6303a283962f535a611698c51dcdcecec2ba09dad8889579 |
| B-final-replay | ec280381029d81e381ac333c752f2d8984e0fdaa1465832ff6696c0166df8f04 |
| B-final-diffs | 3b483b28b1225b8de4efad0806dcd39e8cbb75b67d56d3ec60d93799758473d8 |
| B-source-adjudication | 356908e883eacdbb24af2ba2ea2af7bb88f25db6057855f596663474edbca31e |
| Combined-implementation-fingerprint | b71ed257ab5d634f17d21d003086a7614d4121523053cbd74a504d1e8e5cab68 |

## Cold diff audit

`boundary-probe: accepted clean headings and rejected malformed, TOC, wrapped,
missing-title and cross-page forms; complete decimal children retained; mixed
numbering abstains; unknown versions reject; both saved grammars replay; selector
uses origin-produced boundary_uncertain with no downstream text re-scan.`

`effect-trace: recognize the two heading layouts without wrong extents | cached
line classification and source-scope construction in whole_clauses.rs | original
public failures now pass; my two regression classes fail before and pass after;
A-AH records every changed slot and proves source-line conservation.`

Aliases and selection code are byte-identical to PR-A (sha256
8fa9e5b9f131d7bdd1e53c79188b582b135b1c0d5684ca1b904a3e60d27a962e).
Furniture and General files are unchanged. The diff adds no I/O, shared mutable
state or privileged operation; new classification/scope state is document-local.
Customer-visible boundary errors are the blocking risk, covered by the
regression cases above and still subject to independent review.

## Gap audit and stop condition

DONE: contract-first commit, reproductions, origin implementation, cleanup,
public regression tests, historical replay, A-AH reconciliation and local checks.

NOT DONE: exact-head CI, independent PR-B review, operator verification of both
locks and the single fresh combined v5 batch. Keep draft. Freeze the combined
implementation fingerprint above; stop tuning and wait for the operator's
unseen release. PR118 also remains draft for that combined qualification gate.
