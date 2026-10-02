# Contract view: key terms first, then the full source-clause list

Status: accepted contract implemented at `6e3c375`; independent review and the
existing summary-first merge hold remain open.
Operator direction: [PR111 comment 5938758874](https://github.com/canfieldjuan/document-summarizer/pull/111#issuecomment-5938758874).
This document supersedes the earlier combined source-repair/extraction plan.
Acceptance: [PR111 comment 5943290721](https://github.com/canfieldjuan/document-summarizer/pull/111#issuecomment-5943290721).
The two required amendments were committed separately in `281301f`, before
implementation. PR111 remains a draft; acceptance authorized implementation,
not release of the summary-first merge hold.

## Root cause

At PR111 head `847ca3e68c35aaee1a69e634d40a198e3aa1b758`,
`ContractExtraction` in `pipeline/contracts.rs` contains only `clauses`.
`summary/contract_extraction.rs::records` copies source clauses, and
`src/main.ts::renderClaims` renders that inventory directly. There is no typed,
validated category selection or key-terms-first presentation. Merely adding
headings in the frontend would leave persistence and Connect inconsistent.

The branch also owns source splitting and furniture removal in
`summary/clauses.rs`. On refreshed main
`e0cb818d57a1c9e51125540b9a2c4c5e1f377516`, F1 and F2 already own those decisions
in `summary/coherent/page_furniture.rs` and `summary/coherent/whole_clauses.rs`.
The latter exposes `Clauses::new`, `segment` and `context`; it is not yet a typed
extraction-record enumerator and has no heading field. Its `number` function
recognizes decimal clause markers; its `leading_title` rule only admits an
all-caps prefix immediately before one. It does not recognize ARTICLE starts,
separate an inline title from its body, or attach a title-case leading title to
the following section. Therefore the heading/section rules below require a
versioned extension of this shared owner, not just a new matching table.
Reuse its authoritative source order and expose source ranges rather than
retaining a second parser in PR111.

The prior contract's statement that no fixed term taxonomy was wanted is
superseded by the operator's six categories below. Earlier local tests and A/B
clause counts were against the unreconciled branch and do not qualify this new
view or establish merge readiness.

## Required change surface

### Source ownership and scope of the branch

- Bring PR111 onto the current main baseline without rewriting published
  history. Remove the duplicate `summary/clauses.rs` owner and reuse F1's
  furniture filtering and F2's whole-clause boundaries and governing openings.
- If extraction needs ordered fragments, printed headings or provenance not
  exposed today, extend the shared owner with that view. Do not copy its
  parsing/filtering logic into extraction or a key-terms module. Preserve its
  existing General behavior and historical version dispatch.
- Keep PR111 focused on deterministic Contract extraction and presentation.
  The General verification change belongs to F3/PR116, not this PR.
- Contract mode, including Automatic resolving to Contract, makes no model
  call for clause extraction, term selection, rewriting or validation.

### Product and data contract

Both desktop and the existing Connect text envelope present:

1. **Key terms**, in the fixed category order below.
2. **Full clause list**, retaining every extracted record in canonical source
   order with its original text, heading when present, and citations.

The categories are **parties, payment, term/renewal, termination, insurance,
liability/indemnity**. Every category is present exactly once. A category with
no selected clause displays exactly **not identified**. This means the declared
selector found no match; it does not assert that the document lacks that term.

Each selected entry quotes a **whole source section**, represented by its ordered
whole-clause references, including the heading, governing opening and all
subclauses. A leaf section can contain just one clause. The opening-parties
fallback below similarly retains its entire source prefix. No sentence picking,
paraphrases, synthesized names, inferred dates, amounts, obligations or legal
conclusions are added. Section/category labels and the fixed missing-match
message are application text.

Store a typed, versioned selection alongside the existing clause inventory:
category, selection rule, source-section/root reference and the complete ordered
clause-ID list. Opening-party selections identify the opening source unit(s).
These references, not copied free text, are authoritative; text and page
citations resolve from validated clause/evidence records. Do not create an
independently editable key-term text copy. Validation rebuilds the selection
including its section extent, and the renderer copies source wording exactly.

Every displayed clause includes its **clause ID and source page citation(s)**.
Cross-page clauses cite all their authoritative source spans. Each key-term
reference and its copy in the full list resolve to the same record. Adding key
terms must not remove, reorder, shorten or mutate that full inventory.
"Unchanged" here means unchanged by the key-term layer over the shared source
owner; the old PR111 parser's output is not a second authority.

### Accepted deterministic selection policy

These Contract extraction rules extend F2 through the shared source owner with
heading ranges, section ancestry and ordered source fragments under a new
extraction policy version.
Do not change General's current segmentation or historical artifact dispatch.
Category matching consumes this view; it must not become another parser.

#### What counts as a heading

Use canonical retained source lines after F1 furniture filtering, across block
and page boundaries. A line boundary means a newline or the end of a normalized
source block, not a visual line inferred from a model. Ignore blank lines for
adjacency, but do not skip intervening nonblank text. Retain original bytes and
provenance for display; normalization below is for recognition only.

The initial policy recognizes these three layouts. Resolve an ARTICLE start
first, then a numeric start, then a standalone leading title, so the same line
cannot acquire two owners:

1. **Numbered title, standalone or inline.** At the start of a retained line,
   accept a decimal marker such as `5`, `5.`, `5)`, `5.1` or `5.1.`, optionally
   preceded by the section sign. Each decimal component has one to three ASCII
   digits with literal periods between components and at most one trailing
   period or closing parenthesis. The marker must be followed by whitespace;
   its remainder must be nonempty. After the marker, the candidate ends at
   the first period or colon followed by whitespace/end of line; with no such
   delimiter, use the entire remaining line. Text following the delimiter is clause
   body and must stay in the same source clause. Thus `5. Payment. Client shall
   pay...` has heading `Payment`, but quotes the whole clause including `Client`.
   A numeric start remains a clause/section boundary even when its candidate
   does not match any category. Do not search later sentences for a heading.
2. **Standalone title before a numbered clause.** A complete retained line is
   a title when, after removing one optional terminal period/colon, it consists
   of alphabetic words and spaces, with `&`, `/`, hyphen and apostrophe allowed
   inside/between words, and is either all caps or title case. For title case,
   alphabetic words are uppercase or have an uppercase first letter and
   lowercase remaining letters, except the lowercase connectors `and`, `or`,
   `of`, `the`, `to`, `for`, `in`, `with`, `a`, `an`; at least one word starts
   uppercase. The next nonblank retained line must begin with a numeric marker
   from rule 1. The title belongs
   to the following section, never the preceding clause. For a dotted marker
   such as `5.1`, give the title the implicit parent `5` when that parent is not
   already open in the current article/document; otherwise attach it to the
   incoming clause `5.1` without renaming the existing parent. For a plain `5`,
   attach the title to section `5`. This supports `Payment Terms` followed by
   `5.1 ...` even across blocks/pages. If the numbered line also has a title,
   retain and match both separately at their assigned level(s); never
   concatenate them into a made-up heading or duplicate the source section.
3. **Standalone ARTICLE title.** A complete retained line of the form
   `ARTICLE <ordinal> <separator> <title>` opens an article. Match ARTICLE
   case-insensitively; the ordinal is a positive decimal integer of up to three
   digits or a canonical uppercase Roman numeral I through MMMCMXCIX. The
   separator is a hyphen, U+2013, U+2014 or colon, with optional surrounding
   whitespace; require whitespace between ARTICLE and its ordinal. The title
   is the entire remaining line, with only an optional terminal period/colon
   removed. Thus `ARTICLE V - PAYMENT` yields `PAYMENT`. Inline body on an
   ARTICLE heading line is not supported. An unmatched ARTICLE title still
   establishes the next article boundary.

These grammar rules establish candidates and boundaries **independently of the
category table**. For example, an unlisted `6. Notices` must end section 5.
No font-size inference, all-body keyword search or filename matching is allowed.

#### Section extent and ordering

A match selects the **complete section** rooted at its heading. A numeric
section `5` contains its opening and every contiguous descendant (`5.1`,
`5.1.1`, `5.2`, etc.), including unnumbered continuations and cross-page text.
It ends before the next numeric marker that is not a strict component-prefix
descendant, the next ARTICLE, or a standalone title introducing such a new
section, whichever comes first. Components, not string prefixes, determine
ancestry: `50.1` is not a child of `5`. A `5.1` match similarly includes `5.1.1`
but stops before `5.2`. With no following boundary, end at document end.
An implicit parent from rule 2 follows the same rule. Section identity includes
its source occurrence and article scope, not just its printed number; a new
ARTICLE resets numeric ancestry.

An ARTICLE section includes all subsequent clauses until the next ARTICLE or
end of document; numeric sections nested within it do not end the article.
Do not infer an article's end from numbering style changes or an unsupported
appendix heading. This limitation is reported with the known misses below.

For `5. PAYMENT / 5.1 ... / 5.2 ... / 6. NOTICES`, the payment entry must quote
5, 5.1 and 5.2, including the opening words and citations, and exclude 6. It must
never present `5. PAYMENT` alone when child text exists. Unknown child headings
do not drop their text from the selected parent. A matched section whose entire
retained text is just its own heading is not evidence of terms: select no
key-term entry for it and report that heading-only miss; keep the heading in
the full list. Do not require each child to have its own recognized heading.

Keep all matching sections in canonical document order within a category.
When a matching ancestor already includes a matching descendant in the same
category, keep the ancestor selection once; do not repeat its child references.
Otherwise deduplicate only repeated references to the same source occurrence,
not distinct occurrences with equal wording. Matches in different categories
may refer to the same source clause without creating extra inventory records.
Do not choose by HashMap iteration, model ranking, amount or apparent importance.

#### Frozen heading table and opening-parties fallback

For matching only, trim the recognized heading, fold ASCII case, replace `&`
with the word `and` surrounded by spaces, then collapse whitespace. Markers and
heading delimiters have already been separated by the rules above. Never change displayed source text.
Match the **entire** normalized heading against this closed table:

| Category | Accepted normalized headings |
| --- | --- |
| parties | parties; parties to the agreement; contracting parties |
| payment | payment; payments; payment terms; fees; compensation; fees and payment; contract price; pricing |
| term/renewal | term; duration; renewal; term and renewal; term/renewal; commencement and duration; initial term; renewal term; term and termination |
| termination | termination; termination of agreement; termination of the agreement; cancellation; term and termination |
| insurance | insurance; insurance requirements; property insurance |
| liability/indemnity | liability; limitation of liability; indemnity; indemnification; liability and indemnity; liability and indemnification; hold harmless; limitations of liability |

`term and termination` selects the same complete section under both term/renewal
and termination, without adding inventory records. These additions, plus
`limitations of liability` and `renewal term`, come from general drafting
conventions in the accepted review, before the A/B freeze.

If no headed parties section was selected, apply just this fallback: take all
retained opening source text before the first section, **only if** that section
is root numeric `1` (including an implicit `1` before `1.1`) or ARTICLE I/1,
and the opening contains the standalone word `between`, case-insensitively.
Word boundaries exclude adjacent letters, digits and underscores. Quote the
entire prefix using its source-unit IDs and pages; do not extract or infer party
names. A leading title assigned to clause 1 belongs to that section, not the
opening prefix. No fallback applies when the prefix is empty, the first section
starts elsewhere, or `between` occurs only inside a numbered/article body. This
is a positional selector, not a semantic guarantee that every use of `between`
names parties. Record `opening-between` as the rule in replay evidence.

The grammar, section extents, table, fallback and ordering are policy-versioned
and frozen **before** A/B replay. The additions `pricing`, `initial term` and
`hold harmless` come from this review, not from inspecting A/B selections. Do
not silently tune any rule after observing the replay; seek contract review
for a rule change first.

Known misses and limits: headings outside the table, unsupported combined or
non-English headings, OCR-corrupted headings, inline titles without a period or
colon plus whitespace separating their body, lowercase unnumbered titles,
standalone titles not followed by a recognized numeric marker, `Section 5`
markers, letter-only `(a)` numbering, split-line
ARTICLE titles and unsupported article endings (e.g. an unmarked appendix).
Unheaded parties without the narrow opening rule remain `not identified`.
The rules cannot distinguish a similarly formatted table of contents from
operative sections or establish that a matching heading covers every relevant
obligation elsewhere. This can produce a wrong label, not just a missing match:
a numbered TOC's last entry runs until the body's `1.` and absorbs the preamble;
if that entry matches a category, its key term quotes non-matching text. The
last body section similarly absorbs trailing unnumbered material such as a
signature block or exhibit. Ambiguous/misordered normalized layouts are not repaired
by inference. Keep their source in the full list and existing warnings visible;
report these limits in the replay. No defined-term recognizer is added. Any later
model ranking may select only extracted source IDs under a separate contract.

### Validation, persistence and downstream presentation

- Rebuild the expected clause inventory and deterministic category selections
  from authoritative normalized sources at the existing validation boundaries.
  Reject unknown/duplicate category entries, unknown or cross-document clause
  IDs, mismatched pages, reordered/missing expected selections and altered text.
  Also reject a selected parent with missing children, a section extended into
  its peer, and a fallback that does not satisfy the opening rule. A recomputed
  artifact hash alone is not proof of source fidelity.
- Validate the typed selection and the canonical rendered text together before
  persistence/delivery and again on existing persisted-artifact read paths.
  Unknown free-text fields in reference records are rejected. Mutation of a
  rendered key-term value must fail source reconstruction even with refreshed
  hashes. A valid empty category is distinct from invalid extraction; do not
  turn a validation failure into `not identified`.
- Update the internal extraction/output policy version and integrity binding;
  completed historical artifacts retain their previous interpretation and
  hashes. Do not mutate saved results in place. An incompatible resumable
  checkpoint requires the existing explicit retry path.
- Use the same validated selection and ordering for desktop and Connect.
  Desktop renders quoted text as text, with existing citation actions. The
  Connect text envelope retains its closed schema, capability and media type;
  its text carries key terms first, followed by the source-clause list.
- Adapt the existing canonical render-unit builder and its consumers together.
  `render_citation_claim_lines` currently requires claim lines to reproduce
  canonical text; adding a frontend-only prefix would violate that invariant.
  Category labels, `not identified` rows and repeated source references are
  presentation units, not new extracted clauses. Do not duplicate claim IDs in
  the authoritative ledger or count a label as cited page coverage.
- Preserve the **1 MiB UTF-8 text cap**, existing artifact-size cap, and
  `SUMMARY_TRUNCATED_FOR_DELIVERY` disclosure. Apply the existing whole-unit
  prefix mechanism to the key-terms-first rendering. A selected key-term section
  (all its clauses and citations) is one indivisible delivery group; a prefix
  cannot leave a heading stub or omit its qualifying subclauses. Full-list
  clauses remain individually indivisible. Keep the persisted/full desktop
  inventory intact. Do not skip an oversized section and deliver later units.
  Any absent category/list portion caused by the cap is truncation, not
  `not identified`; a labels-only prefix is not an acceptable result. Preserve
  the source/page coverage checks on actual delivered references. If no acceptable nonempty prefix fits or coverage
  fails, use the existing explicit delivery failure, never silent clipping.
  The current extraction route's final delivery-size check and provider prefix
  path must be exercised together, not assumed to compose from helper tests.
- Selection is pure per-document/per-policy data. Reuse existing run identity,
  stage transactions, state-version checks and checkpoint ownership. Add no
  global document cache, scheduler, queue, model server or background service.

Likely implementation files after approval: `pipeline/contracts.rs`,
`summary/contract_extraction.rs`, the shared source modules where an extraction
view is required, `summary.rs` validation/rendering/version dispatch,
`pipeline/workspace.rs`, `src/main.ts`, and the existing Connect text builder
and provider where presentation-prefix accounting changes. Update the canonical
version/behavior documentation in `docs/CONTRACTS.md` before implementation.
Keep regression tests with these existing owners.

## Explicit non-scope

No new model inference, model ranking, semantic rewrite, model promotion,
preset/runtime/settings change, OCR backend change, email classification,
new Connect capability or wire fields, dependency bump, database migration,
queue infrastructure, General-summary redesign, or historical result rewrite.
No independent source parser. No claim that copied OCR text is independently
accurate. Private source files, business names and quoted contract text stay
local; use A/B aliases and public fixtures in published evidence. #100 remains
frozen and PR116's hold is independent of this contract.

## Assumptions/blockers

The accepted view is key terms first, then the full clause list. The amendments
and canonical contract were committed before implementation. The shared-source
view and all public grammar/table fixtures were frozen at `6e3c375` before A/B
replay. No rule was changed after observing real selections.

The summary-first hold and independent review of this implementation remain
merge gates. General/F3 verification is outside this PR. No fresh model inference
is required for this deterministic extraction replay.

## Verification plan

Before implementation, use a public clause fixture to prove the old branch lacks
key-term-first typed output through persistence, desktop projection and Connect
rendering. Keep that expected failing test specific to this missing behavior.
After implementation, prove:

- Public fixtures cover each declared layout before real replay: `5. Payment.
  Client shall pay...`; title-case `Payment Terms` before `5.1`; and ARTICLE V
  with each supported separator before PAYMENT. Prove the correct heading and
  exact whole-source selection, including opening words, through the shared
  owner and production extractor. An unlisted heading such as `Payment history
  example` and an accepted word appearing only in body text select nothing.
- `5. PAYMENT`, `5.1`, nested `5.1.1`, `5.2`, `6. NOTICES` prove complete parent
  selection and exclusion of the next peer, including a cross-page child and
  an unknown child heading. `50.1` is not a descendant of `5`. A leaf match stops
  at its peer; an ARTICLE stops at the next ARTICLE even if its title is
  unmatched. A title-case leading heading is absent from the preceding clause
  and present exactly once in its own section. Cover both implicit and already
  open parents, and repeated numbers in different articles. A real heading-only
  section is not presented as a populated key term. Matching parent plus matching child
  produces one complete selection, while distinct same-text sections survive.
- The opening fallback accepts a whole prefix containing `between` before
  section 1, including ARTICLE I and implicit root 1, and preserves every source
  reference. Reject body-only `between`, `inbetween`, empty opening, a first
  section other than 1/I, and a prefix without the word. An existing headed
  parties selection suppresses the fallback. Report the positional rule's
  semantic limit rather than calling it party-name extraction.
- The added aliases `pricing`, `initial term` and `hold harmless` have positive
  public fixtures. Mixed matched/unmatched sections and unsupported layouts
  retain the complete source inventory. Negative fixtures cover `Section 5.
  Payment`, `(a) Payment`, a lowercase unnumbered title, an undelimited inline
  heading and a split-line ARTICLE title; none gains a heading match by body
  keyword search. For each accepted marker/title form, also use an unlisted
  title to prove that a structural boundary is not itself a category match.
  Grammar and table fixtures are frozen before looking at real A/B selections.
- Positive public fixtures cover `term and termination`, `limitations of
  liability` and `renewal term`. The combined heading selects one whole section
  under both term/renewal and termination, with no extra inventory records.
- All six categories have stable order; empty categories show `not identified`.
  Multiple matches preserve source order, equal-text distinct occurrences stay
  distinct, and repeated references to one occurrence are not duplicated.
- Missing headings, mixed matched/unmatched clauses, cross-page clauses and
  governing openings retain the full source list and correct page references.
- Altered key-term text fails validation; also reject a valid clause ID paired
  with another clause's text/page, a cross-document ID, unknown category or
  silently omitted expected match. Omit a parent's child or append the next
  section and require rejection even when every individual ID is valid.
  Recompute hashes in mutation tests so hash mismatch is not the only reason
  for rejection.
- Desktop and the actual provider text path both deliver key terms first and
  the full clause list below. Category/list duplication cannot inflate coverage
  counts. At the byte cap, cap-minus-one/cap/cap-plus-one and multibyte source
  text prove whole-unit delivery, explicit truncation and failure when nothing
  acceptable fits. Include a parent whose heading fits but complete section
  does not: no partial section or labels-only success may be delivered, and no
  later unit may leapfrog it. Ordinary non-Contract output remains unchanged.
- The production Contract pipeline completes with a runtime that fails if
  invoked, persists/reopens validated records, and rejects modified persisted
  output. Historical artifacts stay readable. Interleaved independent runs
  cannot mix clause IDs, source text or pages through selection state.
- Replay the saved real contracts **A and B** through the reconciled production
  extraction/validation/rendering path without model calls. For each category,
  record section boundaries, every selected clause ID and page, or
  `not identified`, plus the exact matching heading/fallback rule locally.
  Record document/source hashes, app commit and policy version; verify every selected quote and citation against source and
  compare the complete inventory before/after adding key terms. Report known
  misses and any source-owner changes separately. Locally report whether A or B
  has a numbered TOC or trailing unnumbered material and which key terms that
  affects, including any non-matching text absorbed into a selected section.
  Publish no private wording.

Run focused source/extraction/validation/workspace/Connect tests and the frontend
build, Rust fmt and strict Clippy. Required CI owns duplicated broad suites.
Do not substitute prior branch test results or a helper-only replay for the new
production path. Independent review checks the A/B selections and both negative
regressions before the implementation can be called complete.

## Implementation summary

Implemented at `6e3c375709689130c707aca8eb727a50835b6120`, after the contract-only
amendment `281301f`, main reconciliation `ff877cc` and canonical contract `83bae74`.
The former `summary/clauses.rs` is deleted. Contract policy v2 uses F1 retained
source ranges and F2's shared module, preserving General's existing policy.
The typed six-category view, persisted validation, desktop projection and
Connect text all resolve the same source records. Selected sections are atomic
for bounded delivery; full-list clauses remain individual units. Existing
furniture-only-page disclosure is propagated to coverage checks.

### Verification receipt

- Declared fail-first `contract_key_terms_persist_and_deliver_before_full_clause_list`
  failed because the saved output had no key-terms prefix; it now passes,
  including reload and rejection of changed text with recomputed summary and
  citation hashes/metadata.
- Declared fail-first `contract_source_furniture_disclosure_preserves_delivery_coverage`
  failed on missing existing-source warning; it now passes after propagation.
- `cargo test --quiet --locked --lib contract_`: 31 passed, 0 failed, 7 ignored.
  This includes public grammar/alias/hierarchy negatives, independent document
  identities, mutation rejection, byte-cap boundaries and the production-stage
  provider completion test with both ordinary and oversized selected sections.
- Adjacent filters: `whole_clause` 8 passed; `furniture_` 8 passed, 1 ignored;
  `pipeline::workspace::tests` 11 passed; `connect::contracts::tests` 5 passed.
  Each of the bounded-prefix and below-page-coverage provider regressions passed.
- `npm run build`, `cargo fmt --all --check` and
  `cargo clippy --locked --all-targets --all-features -- -D warnings`: passed.
- Private saved-source replay (ignored test explicitly enabled): 1 passed.
  No LLM, model server or inference configuration was used or changed.

### A/B replay after freeze

| Alias | Source clauses | Delivered render units | Text bytes | Model calls |
| --- | ---: | ---: | ---: | ---: |
| A | 67 | 71/71 | 57,368 | 0 |
| B | 183 | 186/186 | 92,266 | 0 |

All current shared-source clauses equal the final inventory texts. Adding key
terms adds only references, not inventory records. Source reconstruction,
citation validation and actual delivered-reference page coverage passed.
The earlier prototype receipt records 65/181 clauses but contains counts only;
that is not a per-record equivalence claim across the changed source policies.

| Category | A: sections / clause references | B: sections / clause references |
| --- | --- | --- |
| parties | 1 / 3 | 1 / 1 |
| payment | not identified | not identified |
| term/renewal | not identified | not identified |
| termination | not identified | not identified |
| insurance | 1 / 4 | 1 / 4 |
| liability/indemnity | 2 / 8 | 1 / 3 |

These misses are reported, not tuned away. The corresponding document text
remains in the full inventory; this is not complete key-term identification or
semantic/legal qualification.

The required extent assessment found no numbered TOC in A. A's final unmatched
section absorbs signature/attachment material; none enters a selected key term.
B contains a numbered TOC. Its final item absorbs an unsupported ARTICLE heading,
not a preamble in these normalized sources, and is not selected. The heading-only
payment TOC entry is not selected. B's ending is a numbered document-enumeration
clause with subitems, not a trailing unnumbered signature/appendix block. These
observations do not remove the general TOC/trailing-material risks in the policy.

This replay starts at saved normalized/chunked sources, not fresh PDF ingestion
or installed GUI operation. Its Connect input descriptor is a public fixture;
it does not establish original PDF-byte provenance. The separate public provider
test covers production stage persistence, workspace reload, completion and
oversized-section failure. Full requests are inapplicable: there are no model
calls. Source hashes, exact quotes, IDs, pages, rules, raw rendered results and
extent annotations remain in the ignored local evidence packet for review.

## Cold diff audit

| File / owner | Change and contract trace | Verification |
| --- | --- | --- |
| `docs/CONTRACTS.md` | Canonical typed policy, versions, scope and delivery invariants | Contract committed before feature code; cold diff |
| `docs/PR-CLAUSE-EXTRACTION.md` | Accepted amendments, unchanged frozen rules and this receipt | Acceptance and pre-replay freeze hashes |
| `pipeline/contracts.rs::ContractExtraction` | Strict typed source records/selections, optional artifact fields, integrity binding | Mutation, JSON/persistence and historical workspace tests |
| `summary/contract_extraction.rs` | Deterministic extraction, selection, authoritative rebuild, canonical atomic rendering | Public fixtures, mutations, cap boundaries, A/B replay |
| `summary/coherent/whole_clauses.rs::contract_sources` | Versioned source view and section ancestry within shared owner | Grammar, cross-page/parent/article tests; existing General whole-clause tests |
| `summary/coherent/page_furniture.rs` | Exposes existing F1 range/warning API within summary module; no detector change | Existing furniture tests and Contract disclosure regression |
| `summary.rs` | Contract dispatch, versions, validation, canonical rendering and unique-source prefix coverage | Persist/reload regression, provider size path, replay |
| `pipeline/workspace.rs::SummaryView` | Typed desktop view and binding to validated citation artifact | Workspace tests and modified-output rejection |
| `src/main.ts::renderClaims` | Key terms then full list, source IDs/citations, text-only DOM insertion | TypeScript/build plus persisted projection fixture; installed GUI not exercised |
| `connect/provider.rs` | Public fixture for real production stage completion and oversize failure; existing runtime logic unchanged | Ordinary completion and first-section-too-large case |
| `connect/contracts.rs`, `connect/v2.rs` | Existing test initializers explicitly have no extraction | Connect tests, strict all-target Clippy |
| `summary/coherent.rs`, `coherent/trim.rs`, `direct.rs`, `legacy_generation.rs` | Optional-field and closed-enum plumbing; existing General algorithms unchanged | Whole-clause/furniture/workspace tests and strict Clippy |
| Former `summary/clauses.rs` | Removed duplicate source owner | No module reference; all extraction uses shared owner |

boundary-probe: accepted and unsupported layouts, heading-only section, peer
exclusion, complete parent with nested/cross-page children, repeated numbers in
different articles, dual-category references, changed/omitted/cross-document
selections and checksum-consistent text tampering. UTF-8 delivery tests cover
cap-minus-one, cap and cap-plus-one; the actual provider rejects an oversized
first selected section without delivering a heading stub or later unit.

effect-trace: key terms precede the full inventory | typed source references and
`render_units` control persisted/Connect order; `renderClaims` consumes the same
validated view | fail-first persisted regression, provider completion and frozen
A/B replay demonstrate the order and source-bound content.

Concurrency remains per-run existing transactions/state-version checks. Selection
has no shared mutable document state. Interleaved public document runs prove
identical text cannot reuse another document's IDs; this is not an endurance test.

## Gap audit

NOT DONE for merge. Implementation and local verification are complete. Required
exact-head CI, independent review of the selections/negative regressions, and
release of the existing summary-first hold remain. The selector's reported
layout/topic limits are unchanged. No fresh PDF ingestion, installed GUI proof,
new model qualification or broad duplicate CI suite is claimed.
