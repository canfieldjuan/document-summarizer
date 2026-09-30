# Complete source clauses and structured Contract extraction

## Root cause

The current source catalog packs sentence fragments into 600-character quotes,
including repeated page furniture. General selection chooses positions in that
catalog. Verification sees the selected quote rather than its enclosing clause.
The saved A/B read-through found repeated copyright footers and a payment quote
missing its governing substantial-completion lead-in. Other qualifications were
present but lost during model paraphrase. Contract mode still requests prose.

## Required change surface

- Add one deterministic source-clause owner beneath analysis, synthesis and
  verification. Preserve original source bytes and page/block provenance;
  exclude recognizably repeated marginal page furniture before selection.
- Keep numbered clauses, their printed headings, conditions and cross-page
  continuations together. Unnumbered text uses paragraph boundaries. Do not
  infer missing words. Context that cannot fit is unavailable, never shortened.
- Version the new source policy. Historical artifacts retain their former
  reconstruction and hashes. Synthesis schema and parser still admit only the
  offered identifiers. Verification receives complete governing source context.
- Contract mode (including Automatic resolving to Contract) produces structured
  clause extraction, not generated prose: ordered clause identifiers, printed
  headings where present, original provisions and page citations. Extraction is
  deterministic source copying; it does not claim normalized legal interpretation
  or fill unstated commercial terms. Use existing pipeline transactions and
  provenance validation. Persist typed output and render the same records in the
  desktop and the existing Connect text envelope; preserve old artifacts.
- Tests cover actual request construction, footer versus operative copyright
  clauses, headings/continuations, full verification context, unknown citations,
  structured output persistence, desktop projection and Connect delivery.

## Explicit non-scope

No model promotion, preset/runtime changes, additional model repair, installed
settings, dependency bumps, OCR backend changes, email classification, new
Connect capability, or changes to historical stored results. Do not claim
source extraction independently proves OCR transcription accuracy. Private
source files and business names remain local.

## Assumptions/blockers

The operator explicitly requested the Contract output change. Clause records
preserve contractual wording and avoid a speculative fixed legal-field taxonomy.
An unrecognized layout remains source text with an explicit missing heading;
source-boundary uncertainty must not silently create confident legal fields.
Saved pre-extraction Contract checkpoints require an explicit retry before new
synthesis; completed historical artifacts remain readable. The existing external
summary text envelope can carry a deterministic readable
rendering without changing Connect's closed wire schema. Exact machine-readable
clause records are retained by the application. #100 remains frozen.

## Verification plan

Declare and run failing-before regressions for repeated footer admission,
detached clause lead-in and missing fact-check context. Implement only after
observing these failures. Test both sides and mixed input, long/cross-page
clauses, output mutation and persisted historical replay. Replay saved A/B
sources/answers before any new inference. Run adjacent Rust tests, fmt, strict
Clippy and frontend build. Required CI owns duplicated broad suites.

## Implementation summary

Implemented one source-clause owner and a deterministic Contract route. Analysis
15 excludes repeated publisher notices before selection; synthesis 11 and its
verification request use full governing context. The PDF parser's real text order
puts B's visual footer before the body, so the filter removes only the repeated
notice range, never the rest of the page. Ordinary prose/form quotation behavior
is preserved. Unknown/ambiguous context is withheld with the existing omission
warning. Contract records retain source occurrences directly, including their
exact text and citations, without requesting model paraphrases.

Typed extraction is persisted and integrity-bound through synthesis, verification,
summary 9 and citation 5. The desktop renders those records; Connect projects the
same source clauses into its existing text envelope. Historical artifacts keep
old hashes/catalogs. A pre-extraction Contract analysis checkpoint requires retry.

Verification: the production-catalog test failed before implementation because a
repeated footer entered excerpts. It passes after, alongside inverse operative
copyright, cross-page context, clause size, real verification-request and output
mutation tests. A second failing-before regression proved that a notice without
a blank separator could consume a following numbered clause; the filter now
stops at that clause heading. The adjacent checks passed: summary 268, workspace 11, service 16,
Connect contracts 5, desktop 19. Frontend build and strict all-target/all-feature
Clippy passed. The Contract pipeline test also exercises the provider's actual
claim-line wire builder and checks its unchanged envelope and summary version.
Opt-in model/platform tests were not claimed as executed.

Saved-source replay (no inference): both old A/B artifacts still validate. New
General offers 26/42 excerpts, all with full context and no copyright-footer
excerpts. A/B disclose 3/1 omitted ambiguous units. B's payment excerpt has the
substantial-completion lead-in. Structured extraction returns 65/181 ordered
records from those sources. These are clause inventories, not a normalized legal
field taxonomy or semantic-quality sign-off.

## Cold diff audit

- `summary/clauses.rs`: exact source ranges, repeated notice exclusion before
  selection, numbered-clause continuation, context reconstruction and ambiguity/
  size handling. Original normalized text is never modified.
- `summary.rs`, `summary/pages.rs`, `summary/coherent.rs`: version dispatch,
  offered-catalog source authority, actual verifier request and preflight context,
  and the Contract route. Tests pin production wiring as well as helper behavior.
- `summary/contract_extraction.rs`: deterministic records are rebuilt from source
  at each validation boundary. Altered text, identifiers, metadata or provenance
  fails exact equality. Existing transactional stage transitions own concurrency;
  no new shared mutable state, queue or background service was introduced.
- `pipeline/contracts.rs`, `workspace.rs`, `src/main.ts`: typed records, versioned
  integrity binding, validated projection and text-safe rendering. Missing headings
  remain absent rather than invented. Connect's wire schema remains unchanged.
- Existing constructors in Connect tests, direct synthesis, trim and historical
  helpers receive `None` for the new optional extraction payload; their behavior
  is otherwise unchanged.

boundary-probe: repeated notice excluded / operative copyright retained; known
source accepted / unoffered source rejected; cross-page context retained; cap
ceiling-1/ceiling/ceiling+1 checked; mutated Contract record rejected; saved old artifacts
replayed. Structured output never routes back into generated prose.

effect-trace: footer-free complete source context | the shared source owner feeds
catalog and verifier construction | fail-first public regression, actual captured
verification requests and saved A/B replay demonstrate the changed input. Contract
output | profile dispatch plus exact-source reconstruction | persisted desktop and
Connect pipeline tests complete with a failing model runtime and zero generation
calls.

## Gap audit

DONE for local implementation and targeted deterministic verification.
NOT DONE for merge: required CI and independent exact-head review are pending.
No installed GUI/gateway run, new inference or model promotion was performed.
The existing Q/A-form and broader layout gaps in #105 remain separate. This
change does not claim all footer styles are recognized, that OCR is correct, or
that General model summaries now preserve every material qualification. #100
remains frozen.
