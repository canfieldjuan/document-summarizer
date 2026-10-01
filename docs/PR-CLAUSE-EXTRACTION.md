# Contract view: key terms first, then the full source-clause list

Status: revised contract for review; key-terms implementation has not started.
Operator direction: [PR111 comment 5938758874](https://github.com/canfieldjuan/document-summarizer/pull/111#issuecomment-5938758874).
This document supersedes the earlier combined source-repair/extraction plan.
Commit this revision on its own and stop for contract review before implementing
key terms. PR111 remains a draft with its blocking threads open.

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
extraction-record enumerator. Reuse and, where needed, expose its authoritative
ordered source ranges rather than retaining a second parser in PR111.

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

For this slice, each selected entry quotes a **whole extracted clause**. The
operator also permits exact spans, but span selection is not needed here and
will not be introduced implicitly. No paraphrases, synthesized names, inferred
dates, amounts, obligations or legal conclusions are added. Section/category
labels and the fixed missing-match message are application text.

Store a typed, versioned selection of clause references alongside the existing
clause inventory. Its authoritative selection data is category plus clause ID;
text and page citations resolve from the validated clause/evidence records.
Do not create an independently editable key-term text copy. The validated view
and canonical text renderer copy the resolved source wording exactly.

Every displayed entry includes its **clause ID and source page citation(s)**.
Cross-page clauses cite all their authoritative source spans. The key-term entry
and its copy in the full list resolve to the same record. Adding key terms must
not remove, reorder, shorten or mutate that full inventory. "Unchanged" here
means unchanged by the key-term layer over the shared source owner; the old
PR111 parser's output is not a second authority.

### Deterministic selection policy for review

The initial selector matches **printed headings only**. It uses the heading
identified by the shared source owner; it does not search body text, filenames,
model summaries or arbitrary substrings. For matching only, remove a recognized
printed clause-number prefix, trim whitespace and terminal colon/period, fold
ASCII case, collapse whitespace, and normalize `&` to `and`. These operations
never modify displayed source text. Match the entire normalized heading against
this versioned, closed table:

| Category | Accepted normalized headings |
| --- | --- |
| parties | parties; parties to the agreement; contracting parties |
| payment | payment; payments; payment terms; fees; compensation; fees and payment; contract price |
| term/renewal | term; duration; renewal; term and renewal; term/renewal; commencement and duration |
| termination | termination; termination of agreement; termination of the agreement; cancellation |
| insurance | insurance; insurance requirements; property insurance |
| liability/indemnity | liability; limitation of liability; indemnity; indemnification; liability and indemnity; liability and indemnification |

Keep **all** matching clauses in canonical document order within a category;
remove duplicate references to the same clause ID, not distinct occurrences
with equal wording. Do not choose by HashMap iteration, model ranking, amount,
recency or apparent importance. A matching parent heading does not automatically
classify separately extracted child clauses; retain the complete governing
context supplied by the shared owner. A combined heading not in the table is
unmatched. The table and ordering are policy-versioned and frozen before A/B
replay; do not tune aliases silently after observing that replay.

Known misses: unheaded party introductions and inline defined terms, synonyms
outside the table, unsupported combined headings, non-English headings,
OCR-corrupted headings, and layouts whose heading boundaries the shared owner
cannot establish. Their text stays available in the full clause list and
existing source warnings remain visible. A defined-term recognizer is not part
of this initial rule. These misses must appear in the replay report. Any later
model ranking may select only extracted clause IDs and needs a separate contract.

### Validation, persistence and downstream presentation

- Rebuild the expected clause inventory and deterministic category selections
  from authoritative normalized sources at the existing validation boundaries.
  Reject unknown/duplicate category entries, unknown or cross-document clause
  IDs, mismatched pages, reordered/missing expected selections and altered text.
  A recomputed artifact hash alone is not proof of source fidelity.
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
  prefix mechanism to the key-terms-first rendering; never cut a quoted clause
  or its citation mid-unit. Keep the persisted/full desktop inventory intact.
  Any absent category/list portion caused by the cap is truncation, not
  `not identified`. Preserve the source/page coverage checks on the actual
  delivered references. If no acceptable nonempty prefix fits or coverage
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

The operator approved the product direction: key terms first, then the full
clause list. The detailed matcher and data/delivery rules above are **proposed
for contract review**, not already implemented or accepted. In particular,
heading-only selection deliberately leaves unheaded party definitions unmatched.
A/B replay must report that honestly rather than infer a result.

PR111 still has the summary-first hold, duplicate-owner review thread and merge
conflicts. A contract-only commit does not resolve them. After contract review,
reconcile with the shared F1/F2 owners and complete the agreed summary-fidelity
review prerequisite before claiming extraction is ready to merge. No fresh A/B
inference is required for the deterministic extraction replay.

## Verification plan

Before implementation, use a public clause fixture to prove the old branch lacks
key-term-first typed output through persistence, desktop projection and Connect
rendering. Keep that expected failing test specific to this missing behavior.
After implementation, prove:

- A recognized heading selects the exact whole clause; a non-matching heading
  such as `Payment history example` is not selected. Case, number-prefix and
  permitted punctuation variants match without changing source bytes. A body
  mention of an accepted term beneath another heading does not select it.
- All six categories have stable order; empty categories show `not identified`.
  Multiple matches preserve source order, equal-text distinct occurrences stay
  distinct, and repeated references to one occurrence are not duplicated.
- Missing headings, mixed matched/unmatched clauses, cross-page clauses and
  governing openings retain the full source list and correct page references.
- Altered key-term text fails validation; also reject a valid clause ID paired
  with another clause's text/page, a cross-document ID, unknown category or
  silently omitted expected match. Recompute hashes in mutation tests so hash
  mismatch is not the only reason for rejection.
- Desktop and the actual provider text path both deliver key terms first and
  the full clause list below. Category/list duplication cannot inflate coverage
  counts. At the byte cap, cap-minus-one/cap/cap-plus-one and multibyte source
  text prove whole-unit delivery, explicit truncation and failure when nothing
  acceptable fits. Ordinary non-Contract output remains unchanged.
- The production Contract pipeline completes with a runtime that fails if
  invoked, persists/reopens validated records, and rejects modified persisted
  output. Historical artifacts stay readable. Interleaved independent runs
  cannot mix clause IDs, source text or pages through selection state.
- Replay the saved real contracts **A and B** through the reconciled production
  extraction/validation/rendering path without model calls. For each category,
  record selected clause ID(s) and page(s), or `not identified`, plus the exact
  matching heading/rule locally. Record document/source hashes, app commit and
  policy version; verify every selected quote and citation against source and
  compare the complete inventory before/after adding key terms. Report known
  misses and any source-owner changes separately. Publish no private wording.

Run focused source/extraction/validation/workspace/Connect tests and the frontend
build, Rust fmt and strict Clippy. Required CI owns duplicated broad suites.
Do not substitute prior branch test results or a helper-only replay for the new
production path. Independent review checks the A/B selections and both negative
regressions before the implementation can be called complete.

## Implementation summary

This revision changes **only this contract**. It specifies the approved view
order, fixed categories, exact-source references, proposed heading matcher,
validation/delivery behavior, shared ownership and settling evidence. The
existing implementation at `847ca3e` has no key-terms feature. No branch
reconciliation, source deletion, tests, inference or product changes were made
for this contract revision.

## Cold diff audit

`docs/PR-CLAUSE-EXTRACTION.md` replaces the stale combined F1/F2/F3 implementation
claims with the current extraction scope and review gate. Source citations name
the inspected PR head and current-main owner APIs. The proposed feature and its
verification obligations are distinguished from behavior present in code.
The contract-only diff must leave all source, fixtures and approved model case
files unchanged; check that plus Markdown whitespace before committing.

## Gap audit

NOT DONE for implementation or merge. This contract revision is ready for review.
Pending: contract acceptance, main reconciliation and duplicate-owner removal,
key-term implementation, failing-before/passing-after regressions, real A/B
extraction replay and independent review, required exact-head CI, and release of
the existing summary-first hold. Stop after publishing this separate contract
commit; do not implement key terms while review is pending.
