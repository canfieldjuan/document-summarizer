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
The existing external summary text envelope can carry a deterministic readable
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

Pending implementation.

## Cold diff audit

Pending final diff.

## Gap audit

NOT DONE: contract recorded before implementation; tests and implementation pending.
