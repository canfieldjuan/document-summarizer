# Contract view: key terms first, then the full source-clause list

Current status (2026-10-02): operator-directed origin uncertainty revision is
previously published at selection policy 3.1.0, source identity 3.0.2. That lock
is invalidated by confirmed classifier defects below. A-O are
regression inputs now. All nine historical wrong slots abstain. Full inventory
and provenance are unchanged. New unseen EDGAR qualification and independent
review remain pending; PR111 remains draft. Stop before the fresh unseen batch.

## Required classifier correction (review findings, operator approved)

Root cause: my 3.1.0 classifier equates failed title admission with an unknown
section boundary. That promotes wrapped sentence tails, long body sentences,
field labels and opening titles to uncertainty. It also never upgrades an
admitted Roman inventory root to Heading. Existing tests encode that defect.
The published 6e6b2b6 lock is invalidated for unseen qualification.

Contract revision from reproduced body-context failures: marker words inside a
wrapped sentence and long mixed-case list-item bodies are continuations, not
new section openers. Unknown structural/ordinal lines must start a source unit
and have a plausible heading remainder (title shape or unpunctuated all-caps
heading). TOC trailing-number recognition requires a numbered heading; bare
trailing numbers require actual TOC context or dot leaders. This avoids amount
fields, dated footers and document identifiers. The public body-context probe
failed on all eight classes before this correction. The canonical Roman ordinal
is cached once and reused by inventory admission rather than reparsed.

Further origin correction: my previous-line inheritance on a newly recognized
numbered clause marks uncertainty even when the unknown line is outside its
selected extent. Public recognized_section_starts_after_unknown_boundary fails
on that exact defect. Remove this special case. Keep the signal on the preceding
section that retained the unknown line; a recognized new start is reliable.

Final reproduced classifier refinement: a short mixed-case remainder does not
make a Roman/letter or structural marker a section heading. The public short
list-body probe failed for insurance, liability and Roman-body cases. Remove
that shortcut; require a title-shaped remainder or an unpunctuated all-caps
heading. Canonical ordinals are case-neutral; long mixed-case enumerated body
and wrapped references stay Text. Short generic caps remain limited to eight
words and source-unit starts outside the opening.

Required change surface: ReadLine and cached classification finalization in
whole_clauses.rs; public regression fixtures and policy identity in
contract_extraction.rs. Admitted Roman roots are reliable Heading boundaries.
Explicit unrecognized Roman/letter and ARTICLE/SECTION/EXHIBIT/APPENDIX/
SCHEDULE/ATTACHMENT/ANNEX markers and TOC evidence remain uncertain. Generic
heading-like detection requires a source-unit start, at most eight all-caps
words, no terminal sentence punctuation, no label colon, and a position after
the opening block. Titles rejected as wrapped body remain Text. Inventory
admission and bytes stay 3.0.2; selection policy becomes 3.1.1.

Explicit non-scope: full-list grouping/text/IDs/ranges/provenance, General,
furniture, topic aliases, rendering, wire/schema/storage and dependencies.
No downstream text rescan. Shared cached metadata controls certainty only.

Assumptions/blockers: F payment/termination were historical wrong slots at
3.0.1 but were corrected by admitted Roman boundaries in 3.0.2. Preserve those
correct selections rather than forcing historical slots to abstain forever.
New unseen qualification and independent review stay pending.

Verification plan: declare public fail-first probes for admitted matching and
sequence Roman roots, wrapped tail, long all-caps body, label and opening title.
Retain negative unsupported/tab/title-case/prose Roman, split ARTICLE, appendix
and TOC tests and clean controls. Replay the frozen A-O inputs, account for all
25 additional withdrawals individually, preserve full inventory on identical
normalized IDs, then lock and stop. Record reproduce/isolate/explain/fix/prove/
prevent-regression per affected slot using public fixtures per source class.

## Origin regression lock receipt

Tested code commit: `ddcc0fddc31f4a987a10bb7d1411a0fd4a67882b`.
Public Contract tests: 25 passed, 0 failed, 1 ignored; format and strict Clippy
passed. Final fresh A-O and same-identity replay: 2 passed, 0 failed. Every case
has zero model calls, successful reload and complete delivery coverage. Clause
IDs, text, headings, evidence IDs, quote ranges and provenance exactly match the
frozen 3.0.2 baseline on identical normalized/chunk identities. Fresh inventories
also match; only selected-section delivery units are withdrawn.

Durable private artifacts (sha256, aliases only):

- `baseline-raw`: `7e5cffe30c9a53ba72c55f3d2e13b473c0cf0f555c8861daf116c52f5919d213`
- `new-raw`: `f2822a3c6e4e2c6771b68f51bbe2147c9358134ad59d52ee3ea9a74b75967573`
- `inventory-replay`: `1115bc8cb3940d16f599ae586ac22ed4d13f0c449d46315a981f69ee2956da17`
- `category-origin-comparison`: `e0d74670d745f260167a2692eebe211d7057974ce0479f815c6e054c8d8e5d04`
- `execution-receipt`: `e136e2c3a3d0f7d29ea842d963a65c871f4e6e5909fe6586e0f23e3b0915a638`

The changed slots below all become `not identified`. All other categories are
unchanged, including 15 identified slots. The 25 additional withdrawals are the
conservative cost of refusing uncertified boundaries; this revision makes no
recall improvement. Each row's source event and uncertainty flag are retained
with the private comparison. Headings and source text remain in the full list.

| Alias | Category | Change | Origin cause |
|---|---|---|---|
| A | termination | additional withdrawal | unsupported standalone title |
| A | insurance | additional withdrawal | unsupported standalone title |
| A | liability/indemnity | additional withdrawal | unsupported standalone title |
| B | parties | additional withdrawal | unsupported standalone title |
| B | payment | additional withdrawal | unsupported standalone title |
| B | insurance | additional withdrawal | unsupported standalone title; unsupported/split ARTICLE boundary |
| C | parties | additional withdrawal | unsupported standalone title |
| D | parties | additional withdrawal | unsupported standalone title |
| D | termination | additional withdrawal | unsupported standalone title |
| E | parties | additional withdrawal | unsupported standalone title |
| E | term/renewal | additional withdrawal | unsupported standalone title |
| E | termination | additional withdrawal | unsupported standalone title |
| E | insurance | additional withdrawal | unsupported standalone title |
| F | parties | additional withdrawal | Roman heading with uncertified boundary; unsupported standalone title |
| F | payment | historical wrong | Roman heading with uncertified boundary; unsupported/split EXHIBIT boundary |
| F | term/renewal | additional withdrawal | Roman heading with uncertified boundary |
| F | termination | historical wrong | Roman heading with uncertified boundary |
| F | liability/indemnity | additional withdrawal | unsupported standalone title |
| G | liability/indemnity | additional withdrawal | unsupported standalone title |
| H | payment | historical wrong | Roman heading with uncertified boundary |
| H | term/renewal | historical wrong | Roman heading with uncertified boundary; unsupported standalone title |
| H | termination | additional withdrawal | unsupported standalone title |
| H | liability/indemnity | historical wrong | Roman heading with uncertified boundary; unsupported standalone title |
| I | parties | additional withdrawal | unsupported standalone title |
| I | payment | historical wrong | Roman heading with uncertified boundary; unsupported standalone title; unsupported/split APPENDIX boundary |
| L | parties | additional withdrawal | unsupported standalone title; unsupported/split EXHIBIT boundary |
| L | payment | additional withdrawal | unsupported/split EXHIBIT boundary |
| L | liability/indemnity | additional withdrawal | unsupported standalone title |
| M | parties | additional withdrawal | unsupported standalone title |
| M | termination | additional withdrawal | unsupported standalone title |
| N | parties | historical wrong | TOC run; unsupported standalone title; unsupported/split ARTICLE boundary |
| N | payment | historical wrong | TOC run; unsupported/split ARTICLE boundary |
| N | termination | historical wrong | TOC run; unsupported/split ARTICLE boundary |
| O | parties | additional withdrawal | TOC run; unsupported standalone title; unsupported/split EXHIBIT boundary |

## Accepted origin uncertainty revision (operator direction)

Root cause: the shared Contract reader treats unrecognized structural lines as
ordinary continuation. It loses boundary confidence before key-term selection.
The bare-Roman patch introduced a literal period-space recognizer, missing tabs;
its separate decimal lookahead parser duplicated grammar with different guards.

Required surface: classify each retained line once at the shared source owner.
Use whitespace-neutral recognition (tabs are spaces) while retaining original
bytes. Cache decimal, ARTICLE, Roman, title and source-unit classifications.
Distinguish reliable headings, unsupported heading-like boundaries, and ordinary
text. Record uncertainty on affected source clauses at this origin; selection
consumes this signal and never re-parses selected text. If any candidate section
for a category crosses uncertainty, the entire category is not identified,
including opening parties. TOC runs, split ARTICLE lines, unsupported Roman or
letter headings, and appendix transitions produce uncertainty. Existing Roman
inventory roots are conservative heading-like boundaries for key terms.

Full inventory is frozen: clause order, text, headings, source ranges, provenance
and IDs retain policy 3.0.2 behavior. This requires retaining inventory admission
rules inside the single reader, not a second parser. Selection policy changes to
3.1.0; source identity remains 3.0.2. No General, furniture, alias, rendering,
wire/schema, dependency or storage changes. No recall improvements.

Special-case inventory across prior revisions, with disposition:

- Independent Contract segmentation/furniture owner (initial revision): already
  removed by the shared source revision; do not restore it.
- Decimal components, optional section symbol/trailing period/parenthesis,
  three-digit limit and uppercase remainder: retained inventory admission;
  consolidate into one cached decimal reading.
- Wrapped-number guard and wrapped sentence-tail/preceding-unit guard: preserve
  inventory behavior, remove repeated previous/next line reparsing.
- Leading title plus decimal child, synthesized missing numeric parent,
  strict-child ancestry and repeated numbers under ARTICLE: retained inventory
  semantics, consume cached classifications.
- ARTICLE separators, uppercase whitespace ARTICLE, canonical Roman ordinal,
  numeric ordinal limits and terminal punctuation: retained inventory grammar;
  unsupported/split forms mark uncertainty rather than silently certifying terms.
- Bare Roman uppercase/canonical/matching-decimal/consecutive-ordinal routes:
  retain existing inventory shape (including the prior lookahead exclusion of
  section-symbol-prefixed markers), remove the duplicate decimal parser and
  literal-whitespace confidence assumption; these roots/crossings are uncertain
  for key terms pending a later inventory revision.
- Opening-between fallback, first decimal 1/ARTICLE I/Roman I: retain eligibility,
  but require an uncertainty-free opening range.
- Exact heading aliases including pricing/initial term/hold harmless/plural
  liability and normalized ampersand: retained topic grammar, not boundary logic.
- Conjunction/comma/semicolon/or matching and shared multiword final-head guard:
  retained topic grammar; no selected-text rescan or extra category denylist.
- Furniture disclosure, atomic selected-section delivery and byte/page coverage
  guards: retain delivery behavior; they do not decide boundary confidence.

Verification: public fail-first fixtures for tab/space Roman crossings, split
ARTICLE, TOC entries and appendix transitions must become not identified. Clean
numeric/ARTICLE controls must retain selections. Full inventory equality before
and after is required, including IDs on the same normalized document. Run A-O
fresh as regression only, identify the A/B copies against the old policy, show
all nine historical wrong slots abstain, explain every other category change,
then commit/hash the rules and stop before the operator's new unseen batch.

## Accepted bare-Roman boundary revision

The C-G run exposed a source-boundary failure under policy 3.0.1: a bare Roman
section heading can be appended to the preceding numbered clause. Exact source
reconstruction then preserves that wrong extent in a selected key term. This
revision changes the shared Contract source view, not the category aliases or
rendering. Operator acceptance: Continue in this session. Implementation requires a new policy identity.

Recognize a complete retained line of the form `<Roman>. <TITLE>` as a
top-level section boundary only when all of these hold: the Roman ordinal is
canonical uppercase I through MMMCMXCIX; the title has at least one alphabetic
character and every alphabetic character is uppercase; and the next nonblank
retained line starts with a decimal clause marker whose first component equals
the Roman ordinal. Alternatively, recognize a canonical uppercase Roman title
when its ordinal is exactly one more than the last recognized bare Roman
section. Roman I requires the matching-decimal route. Skipped or repeated
ordinals and sentence-case titles cannot use the sequence route.
The ordinal and title are used only for recognition. Keep
the exact source line, its page and its original bytes in the new section.
The new section resets numeric ancestry, contains its following numbered
descendants, and ends before the next recognized top-level section. An
unmatched Roman title still closes the preceding section. A recognized Roman I
also satisfies the existing opening-parties fallback's first-section rule.

Do not recognize noncanonical numerals, title-case or inline-body forms,
Roman-looking initials or sentences, or a heading whose following decimal
section number disagrees. Retain rejected lines in the full source inventory
under the existing continuation rule. General summary segmentation, furniture
filtering, alias matching, typed output, rendering, storage and Connect wire
shape stay unchanged. If accepted, bump the Contract extraction policy identity
from `contract-extraction-3.0.1` to `contract-extraction-3.0.2` for the
changed boundary grammar; old checkpoints retain their explicit retry path.

Verification: declare a fail-first public production-extraction test showing a
payment clause ending before `III. TERM AND TERMINATION`, a termination clause
ending before `IV. GENERAL PROVISIONS`, and all intervening source lines in the
full list. Probe matching and mismatching ordinals, canonical and invalid Roman
spellings, uppercase and sentence-case titles, page/block boundaries, and an
opening Roman I. Run the focused Contract tests, formatter and strict Clippy.
A/B/C-G are regression evidence only; qualification of the revised rule needs
a newly screened unseen set and independent review before merge.

## Accepted boundary revision (policy 3)

Acceptance: [comment 5945128686](https://github.com/canfieldjuan/document-summarizer/pull/111#issuecomment-5945128686),
with execution order clarified in [comment 5945149973](https://github.com/canfieldjuan/document-summarizer/pull/111#issuecomment-5945149973).

Root cause: policy 2 treated a wrapped title-case sentence tail as a heading and
a wrapped numeric phrase as a new clause. Public production-path probes at
`f8fa549` reproduce both: `between the` / `Parties.` moves into the next clause;
`within` / `30 days of invoice.` breaks the payment parent. Source reconstruction
repeats these decisions, so it proves consistency, not correct clause boundaries.

Required change surface: the shared Contract source view in `whole_clauses.rs`,
the existing `heading_matches` owner, their public regression fixtures, and
policy identity `contract-extraction-3.0.1`. General's versioned behavior, the
alias table, output shapes, model/runtime settings and all other scope stay fixed.

The only policy amendments are the preceding-unit guard, uppercase numeric
remainder, combined-heading matching, uppercase whitespace-only ARTICLE form,
and the unseen-contract gate specified below. Do not tune these from A/B.

Execution: contract-only commit first; fail-first public regressions and
implementation next; focused tests/format/Clippy, then commit and hash the rules
and fixtures. A/B may be replayed only as regression evidence. **Stop before the
held-out run.** The operator-screened private manifest is not yet authorized for
access here; do not search the corpus or open candidate contracts.

Qualification replaces A/B with **3-5 operator-screened real contracts**, aliases
C onward, listed in a private local manifest supplied after operator confirmation.
Freeze before opening any of them. Run from fresh PDF ingestion and report each
category as correct, wrong label or missed, with cause. Pass requires **zero
wrong labels**; explained misses are allowed. Independent review must compare
every category against each source document. No qualification claim before this.

The new public negatives are the wrapped `between the` / `Parties.` and `within`
/ `30 days` cases, mixed-case `Article 5 shall apply`, and `payment history
example`. Positive controls retain real titles, numeric children, existing
aliases, each approved conjunction, and uppercase ARTICLE headings. Probe every
preceding-unit route (terminal punctuation, marker, heading, block/page start)
without changing General or adding a second parser.

### Combined-heading contract revision before qualification

Required review corrections: [comment 5945181625](https://github.com/canfieldjuan/document-summarizer/pull/111#issuecomment-5945181625)
and [comment 5945257806](https://github.com/canfieldjuan/document-summarizer/pull/111#issuecomment-5945257806).

Root cause: `heading_matches` treats every conjunct as an independent title.
Coordinated modifiers can instead share a final head noun: `Payment and
Performance Bonds` and `Duration and Frequency of Services` must not select
payment or term/renewal. The prior B regression's mixed-topic fees selection is
a wrong label, not an acceptable known limit.

Required change surface: narrow the existing matcher, add public negative and
positive production-extraction fixtures, and use policy identity 3.0.1 in code
and the canonical contract. Commit this design document alone first. Keep all
five accepted items, with this narrowing of item 3: always match the final
conjunct; match earlier conjuncts only when the final conjunct is a single
whitespace-delimited word. Preserve exact whole-heading aliases. An empty final
conjunct cannot enable earlier matches. No changes to the alias table, source
boundaries, General, storage/API shapes, renderer, model settings or dependencies.

Verification plan: fail-first public negatives for the two shared-head examples
and `Permits, Fees, Licenses, and Other Obligations`. Retain `Invoicing and
Payment`, `Survival and Termination`, `Insurance and Bonds`, `Term and
Termination` (both categories), and `Fees and Payment Terms` (final alias).
`Termination, Suspension or Assignment of the Subcontract` becomes an explained
miss. Assert rejected headings remain in the full source inventory. Run the
adjacent Contract tests, formatting and strict Clippy, then commit and hash the
rules and fixtures again before A/B regression. B's mixed fees payment selection
must disappear and its combined termination heading becomes an explained miss.

Assumptions/blockers: single-word final conjunct is the approved conservative
composition rule, not proof of semantic accuracy. No held-out source or manifest
access; publish the replacement freeze for confirmation before C-G qualification.
A/B remain regression-only. Implementation/cold diff/gap receipts follow the
tests; the PR is NOT DONE for merge until all qualification/review gates pass.

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
   its remainder must begin with an uppercase character, preserving F2's
   uppercase-first guard. Thus wrapped `30 days of invoice.` is continuation
   text, not a boundary. After the marker, the candidate ends at
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
   from rule 1. The previous retained nonblank line must also end a unit:
   terminal `.`, `:`, `;`, `?`, `!` or `)`, a standalone marker or heading line,
   or the candidate starts a source block/page. A marker/heading line here has
   no trailing clause body; a numbered sentence ending `between the` is not
   such a standalone line. Otherwise the candidate continues the prior clause.
   The title belongs to the following section, never the preceding clause. For a dotted marker
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
   establishes the next article boundary. A whitespace-only separator is also
   accepted when the keyword is exactly `ARTICLE` and every alphabetic character
   of the title is uppercase (with at least one such character). Thus
   `ARTICLE 10    PAYMENTS` is accepted, but `Article 5 shall apply` is not.
   Existing ordinal and explicit-separator rules are unchanged.

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
Keep the exact whole-heading match and also split normalized headings on
` and `, `,`, ` or `, `;` and `&` (already normalized to `and`). Trim each
conjunct and match the **entire conjunct** against the same closed table. Always
test the final conjunct. Test earlier conjuncts only when the final conjunct is
a single whitespace-delimited word; an empty final conjunct does not qualify.
This prevents shared multi-word heads from making a modifier look like a topic.
An admitted match selects that category once; one section can serve several
categories without adding inventory records. `payment history example` remains
unmatched. This changes composition, not the alias table:

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
and frozen **before** any unseen-contract access. A/B are regression cases only. The additions `pricing`, `initial term` and
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
and canonical contract were committed before implementation. Policy 2 was frozen at `6e3c375` before A/B replay. Independent review then
found incorrect boundaries and a wrong parties label. The accepted policy 3
revision above must be frozen again before unseen-contract access.

The summary-fidelity prerequisite and independent review of this implementation
remain merge gates; the original acceptance thread has been cleared. General/F3 verification is outside this PR. No fresh model inference
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

## Current policy-3.0.1 narrowing and regression receipt

Design-only commit: `c31f54d5d002509a58d2f5763f14167823083ee5`.
Implementation/freeze: `f2f26ae8a0d3ab910408f4f9bf91006c7d68467a`.
The rule/fixture fingerprint is the SHA256 of the sorted, compact JSON map of
file names to SHA256 values in the local composition-freeze manifest:
`76ea37062537f7736feaaf326898423208677f10f87dde1c8e37eb6781aa0862`.
The frozen files include the shared clause source, extraction implementation
and tests, provider fixtures, canonical contract and this design document at
the frozen commit. This later receipt edit does not change the frozen rules.

Five public tests failed first for the exact overmatching class, then passed.
They cover bonds, service frequency, mixed-topic lists, the explained termination
miss, positive category sets, an earlier match suppressed before a multi-word
final alias, empty final conjuncts and unchanged full source text. Adjacent
Contract tests: 40 passed, 0 failed, 7 ignored. Rust fmt and strict all-target,
all-feature Clippy passed. The alias table, shared source-boundary parser and
frontend are unchanged from `25f081f`. No frontend test/build was repeated for
this matcher-only revision; required CI remains independent.

Frozen A/B regression on identical saved inputs passed source reconstruction
and delivery coverage: A retains 64 clauses and 69/69 render units; B retains
191 clauses and delivers 195/195 units. All inventory text and headings are
unchanged. A's selections are unchanged. B's payment selections decrease from
2 to 1, removing the wrong mixed-topic fees label; termination decreases from
1 to 0, the required explained miss. Every other category is unchanged.
No new category selection is introduced; no source clause is lost. Model calls
remain zero. The local comparison script's initial field-name error was fixed
to read the existing serialized `clauseId`; production code was not changed.

Cold diff audit: `contract_extraction.rs::{VERSION,heading_matches,tests}` adds
only the approved eligibility gate and production-extraction fixtures;
`CONTRACTS.md` records the matching rule and new deterministic policy identity.
The design-only commit precedes both. No aliases, clause boundaries, renderer,
API/schema/storage shapes, runtime settings or dependencies change.

boundary-probe: shared-head and mixed-list negatives fail before/pass after;
final-alias and single-word-final controls still select exact expected categories,
dual-category terms appear once each, and excluded text stays in the inventory.
effect-trace: remove wrong topic labels | eligibility of earlier conjuncts in
the existing matcher | public fail-first tests and frozen A/B selection comparison.

NOT DONE for merge. Stop before C-G: replacement-freeze confirmation, unseen
qualification, independent review, exact-head CI and the fidelity prerequisite
remain. No private manifest or unseen source was opened. A/B are regressions
only; neither passing reconstruction nor this correction qualifies the rules.

## Historical policy-3.0.0 implementation and regression receipt

Contract-only revision: `642345937154293808159110ec10a027fe1e8b82`.
Implementation and freeze: `edb29fdbdd41ad564158f6fb8fe7cc9ab49eda6b`.
The source files, public fixtures and contract documents were hashed at that
commit before this A/B regression and before opening any unseen document.
The local mode-600 freeze manifest is `contract-policy3-freeze.json`.

All four `revised_policy_` regressions failed first for their intended class:
false parties selection, payment parent cut at wrapped `30 days`, missing
combined match, and missing uppercase whitespace ARTICLE. All four pass after
implementation, including valid-input controls. Rejected ARTICLE forms are
asserted not to create an article boundary, not merely to produce no match.

Validation: Contract filter 35 passed, 0 failed, 7 ignored; shared whole-clause
8 passed; furniture 8 passed with 1 ignored; workspace 11 passed. Frontend build,
Rust fmt and strict all-target/all-feature Clippy passed. The final additional
ARTICLE negative assertion passed in the focused 4-test rerun and strict Clippy.
The General source implementation and alias table remain byte-identical to
published baseline `f8fa549`. Existing ASCII and multibyte cap fixtures start with
uppercase text so they still exercise valid numbered clauses under the new rule.

Saved A/B inputs have identical source hashes to the policy-2 regression:

| Alias | Source clauses | Delivered render units | Regression result |
| --- | ---: | ---: | --- |
| A | 64 | 69/69 | Wrong parties selection removed; sentence tail remains with predecessor |
| B | 191 | 197/197 | Wrapped numeric cross-reference retained as continuation, not a clause start |

| Category | A selected sections | B selected sections |
| --- | ---: | ---: |
| parties | 0 | 1 |
| payment | 1 | 2 |
| term/renewal | 0 | 0 |
| termination | 1 | 1 |
| insurance | 1 | 1 |
| liability/indemnity | 2 | 1 |

These are selection counts, **not correctness ratings**. Reconstructed source
and citations plus delivery coverage passed, with zero model calls. The raw
local regression and checks are `policy3-ab-regression.json` and
`policy3-ab-regression-checks.json`, both mode 600. No private wording is published.

Known limits remain: uppercase number-led address/date/cross-reference lines can
still look like numeric clauses. Combined matching can select a whole mixed-topic
section; one B payment selection is a fees section mixed with other obligations.
Independent review must judge those labels and extents. This regression does not
establish zero wrong labels, complete section boundaries or full topic coverage.
The screened unseen set must supply qualification; no C-onward source or manifest
was opened or searched for, and the held-out run has not started.

Cold diff audit for this revision:

- `docs/PR-CLAUSE-EXTRACTION.md` and `docs/CONTRACTS.md`: accepted rule revision,
  policy identity and explicit unseen gate; committed before implementation.
- `summary/coherent/whole_clauses.rs::{contract_number,ends_source_unit,article_title}`:
  uppercase remainder, preceding-unit guard and uppercase whitespace ARTICLE
  recognition in the existing Contract view. General code is unchanged.
- `summary/contract_extraction.rs::{VERSION,heading_matches,tests}`: policy 3,
  whole-conjunct matching over the unchanged table, fail-first/both-side fixtures,
  and valid uppercase cap inputs.
- `connect/provider.rs` test fixture only: uppercase oversized child text retains
  the same production provider byte-limit failure test. Runtime code is unchanged.

boundary-probe: wrapped sentence tail and lowercase numeric continuation remain
in their original clauses; true titles/numeric children remain admitted. Terminal
punctuation, standalone structural lines, block/page starts, all approved heading
conjunctions and explicit/whitespace ARTICLE forms are covered. Mixed-case ARTICLE,
non-alias prose, empty/punctuation-only candidates and invalid ordinals stay out.

effect-trace: prevent wrong parties labels and severed payment sections | shared
source boundary admission before section ancestry/selection | declared fail-first
public tests, valid controls and frozen A/B regression demonstrate the two repairs.
Validation alone is not used as proof of semantic boundary accuracy.

## Historical policy-2 implementation summary

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

### Historical policy-2 A/B regression receipt (not qualification)

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

Correction from independent review: A's parties selection is a wrong label,
caused by a wrapped sentence tail. The table above records policy-2 output, not
semantic accuracy. Source validation did not detect this boundary error.

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

NOT DONE for merge. Policy 3.0.1 implementation and local regressions are complete.
The unseen-contract gate, required exact-head CI, independent review of the
selections/negative regressions and summary-fidelity review prerequisite remain.
Stop before the held-out run; the screened private manifest must be supplied
after operator confirmation. A/B are regression evidence only. The selector's reported
layout/topic limits are unchanged. No fresh PDF ingestion, installed GUI proof,
new model qualification or broad duplicate CI suite is claimed.
