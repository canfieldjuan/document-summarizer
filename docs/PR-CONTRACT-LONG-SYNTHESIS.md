# Long Contract synthesis parity and unquoted-clause disclosure

Status: proposed contract. Implementation has not started and must wait for
explicit operator acceptance of this document.

### Contract

Root cause:
- Observed on 2026-09-23 with the installed build of current `main`, the Local
  Inference Gateway and `qwen3-30b-a3b` (Instruct 2507) fully on GPU: two real
  multi-page contracts (a 10-page services agreement and a 15-page AIA-form
  subcontract) run under the Contract profile both end in `claimLedgerFallback`
  with `COHERENT_SUMMARY_SOURCE_CONTEXT_TOO_LARGE` ("A complete bounded source
  catalog could not be constructed") and make zero Synthesize requests.
- Broken assumption: long-document coherent synthesis was built and hardened
  for General only. PR #43 records "incomplete Story or Contract catalogs, still
  use the existing fallback" and "This slice does not change Story or Contract
  overflow behavior"; PR #63 bound cross-window repair for General only.
  Contract received long source selection (`supports_long_source_selection`)
  but not the rest of the long-document path:
  1. `incomplete_catalog_requires_fallback` (`summary/coherent.rs:6174-6177`)
     forces the ledger fallback for every non-General catalog with any
     quote-boundary omission. Real contracts almost always have one: running the
     production version-13 segmenter over the two contracts' persisted
     normalized blocks yields 41 omitted units (31 single sentence units over
     600 characters, 10 unterminated page tails).
  2. A summary unit that stops at the 1,200-character decoder limit is
     classified as repairable `MODEL_SUMMARY_RESPONSE_UNIT_CLIPPED` only for
     windowed General catalogs (`summary/coherent.rs:6347`, `6703-6705`). For
     Contract the same unit returns `MODEL_SUMMARY_RESPONSE_INVALID` and fails
     the stage.
  3. For Contract, per-unit completeness validation runs before cross-window
     ownership is determined, and the one window repair sends generic feedback
     without binding to the rejected response (`summary/coherent.rs:3856`,
     `3989`, `4023-4038`; `parse_window_repair_requirements` returns early for
     non-General at `6498`), unlike General after PR #63.
- Evidence from a controlled experiment (scratch worktree at `origin/main`,
  live gateway, fresh database, the 10-page agreement):
  - A. Unmodified, Contract profile: reproduces the installed result exactly
    (ledger fallback, 9 ledger claims, the same four warnings).
  - B. Contract profile with only the incomplete-catalog gate neutralized:
    Synthesize issues four source-selection requests and one synthesis request.
    The response's sixth unit cites eight sources from two selection windows and
    stops at exactly 1,200 characters mid-sentence. The run ends `Failed` with
    `MODEL_SUMMARY_RESPONSE_INVALID`.
  - C. Unmodified, General profile: `coherent` presentation, verified, four
    summary units, with `COHERENT_SUMMARY_SOURCE_SELECTION_APPLIED` and
    `COHERENT_SUMMARY_CROSS_WINDOW_UNITS_WITHHELD`.
- Consequence: making the catalog complete (for example by splitting long
  clauses) would convert today's fallback into a hard failure (run B). That
  symptom patch is rejected. Separately, the user sees only a count of
  unquotable source units, never which clauses they are.

Required behavior:
- R1 Version. New runs use synthesis version `10.0.0`. Artifacts persisted
  under `9.0.0` or earlier validate and reopen under their recorded version's
  rules, including the `9.0.0` Contract incomplete-catalog fallback. Under
  `10.0.0`, General and Story behavior is identical to `9.0.0` except for the
  version identity bound into durable identifiers.
- R2 Incomplete Contract catalog. Under `10.0.0`, a Contract catalog with
  quote-boundary omissions proceeds to synthesis when it has at least one
  admitted candidate, which is the rule General already uses. A catalog with no
  candidates keeps the existing verified-ledger fallback. The short-contract
  clause-coverage rules keep applying only to complete short catalogs.
- R3 Clipped units. Under `10.0.0`, a windowed Contract catalog classifies a
  unit whose text is exactly `MAX_UNIT_CHARACTERS` long, has valid unique
  source IDs and fails the completion check as
  `MODEL_SUMMARY_RESPONSE_UNIT_CLIPPED`. It then gets the same single
  clipped-unit repair and safe-sibling fallback as windowed General. A
  non-windowed Contract catalog keeps its current behavior, matching
  non-windowed General.
- R4 Cross-window ownership and repair. Under `10.0.0`, Contract determines
  cross-window ownership from validated response source IDs before per-unit
  clipping or completion validation can mask it. Its one window repair is bound
  to the rejected response exactly as General's is after PR #63: valid siblings
  unchanged, and every mixed unit's original evidence covered exactly once by
  single-window replacement units. Otherwise it uses the safe-sibling fallback
  with `COHERENT_SUMMARY_CROSS_WINDOW_UNITS_WITHHELD`, and it fails closed when
  no safe result exists. Contract keeps its existing budget of two validation
  repairs. Window repair and clipped repair each remain one attempt.
- R5 Contract source selection is unchanged, including its identity/scope and
  risk/exit source requirements.
- R6 Unquoted-clause disclosure (operator decision, 2026-09-23). For a
  Contract-profile run whose analysis version is `13.0.0`, the desktop summary
  view returned by `get_persisted_summary` carries `unquotedSourceUnits`. It
  has one entry per source unit that the analysis quote catalog omits, across
  every native-text page, in source order. Each entry contains:
  - `pageNumber`
  - `kind`: `overLimitSentence` for a safe-boundary unit over 600 characters,
    or `noSentenceBoundary` for a nonempty tail without a safe terminal
  - `characterCount`
  - `clauseReference`: the leading numbered clause reference found by the
    existing contract clause detector, or `null`
  - `openingText`: the unit's leading words, at most 80 characters, cut at a
    word boundary, marked as shortened when cut

  The list is derived on read from the persisted normalized document and the
  run's recorded analysis version, through the same segmentation function that
  produces the omission counts. Nothing new is persisted. For other profiles,
  and for analysis versions that record no quote-boundary omissions, the list is
  present and empty. The desktop UI renders a "Clauses not quoted" section under
  both `coherent` and `claimLedgerFallback` presentations whenever the list is
  nonempty. The section states that these clauses are not covered by the
  summary or its evidence. `openingText` is display-only: it is never a
  citation, never sent to a model, and never part of summary text, integrity
  hashes, verification or Connect results.
- R7 Connect. Connect-admitted Contract runs get the R2-R4 synthesis behavior,
  so a delivery that previously fell back may now be coherent. The Connect
  result schema and payload fields are unchanged, and the unquoted list is not
  delivered.

Invariants:
- The exact-quote, provenance, source-order and 600-character quote invariants
  are unchanged. The analysis version stays `13.0.0`, and the version-13
  segmenter's segments and omitted counts stay byte-identical. Exposing omitted
  spans derives the existing count from the same spans rather than computing a
  second count.
- For pages the Analyze stage inspected, the unquoted entries equal the count
  and page set reported by that run's `ANALYSIS_QUOTE_BOUNDARY_OMITTED` warning.
  The list may also include pages that analysis did not inspect, because
  synthesis catalogs every chunk.
- A coherent Contract summary built from an incomplete catalog still carries
  the Analyze quote-boundary warning. Disclosure never replaces warnings.
- Durable identifiers created under `10.0.0` bind `10.0.0`. A persisted `9.0.0`
  artifact is never re-derived or relabeled under `10.0.0`.

Failure cases:
- Model output that is invalid and not repairable under R3/R4 fails the stage
  as recoverable, as today. It never silently selects the ledger fallback.
- A runtime context rejection keeps the existing ledger fallback and its
  boundary-specific message.
- If the normalized document or analyzed artifact is missing or invalid on
  read, or the derived entries contradict the recorded warning for inspected
  pages, the summary view fails with the existing workspace integrity error. An
  error is never presented as an empty list.

Concurrency model:
- Synthesis stays one sequential background task per run. New repair requests
  reserve ordinals through the existing deterministic reservation, and
  cancellation checkpoints bracket every model request as they do today.
- A Synthesize stage interrupted before its artifact commits resumes with the
  same deterministic request sequence (seed, catalog, ordinals). It reconciles
  gateway ledger entries by owner, stage, ordinal and semantic request hash. An
  entry whose semantic hash differs from the resumed request is never reused,
  including across a `9.0.0` to `10.0.0` upgrade mid-stage.
- The unquoted list is a pure function of immutable persisted artifacts. Reads
  need nothing beyond the existing read path.

Required change surface:
- `src-tauri/src/pipeline/summary.rs`:
  - set `SYNTHESIS_VERSION` to `10.0.0`, keeping a named `9.0.0` constant and
    its validation routing;
  - expose the version-13 segmenter's omitted units (block, byte range, kind)
    as crate-visible data, with the existing omitted count derived from them.
- `src-tauri/src/pipeline/summary/coherent.rs`:
  - make `incomplete_catalog_requires_fallback` version-aware;
  - extend clipped classification, clipped repair and bound window repair to
    windowed Contract catalogs: the `is_windowed_general_catalog` predicate and
    its call sites (`6347`, `6704`), the General-only guard in
    `parse_window_repair_requirements` (`6498`), and the General-only branches of
    the repair loop (`3856`, `3989`, `4023-4038`);
  - leave General-only source framing
    (`parse_response_without_mixed_source_framing_units`, `6580`) and all Story
    branches untouched.
- `src-tauri/src/pipeline/workspace.rs`: `SummaryView` gains
  `unquoted_source_units`, derived in `get_persisted_summary`.
- `src/main.ts` and `index.html`: the view type and the "Clauses not quoted"
  section.
- Tests in the existing `summary.rs`, `summary/coherent.rs` and `workspace.rs`
  test modules, listed in the verification plan.
- In the implementation commit only: `docs/CONTRACTS.md` gets the canonical
  section and `docs/BUILD_LEDGER.md` gets the verified results.

Explicit non-scope:
- Analysis quote segmentation, the 600-character ceiling, the open-set
  abbreviation rule (`docs/CONTRACTS.md` boundary-safe catalogs) and the
  analysis version. Clause-aware splitting that preserves lead-in context, and
  joining sentences across page breaks, belong to a follow-up slice that reduces
  the 41 omitted units measured above.
- General and Story behavior, source framing, source-selection prompts and
  windows, synthesis prompts other than the repair instructions reused from
  General, `MAX_UNIT_CHARACTERS`, `MAX_SOURCES_PER_UNIT`, semantic verification,
  citations, Connect schemas and results.
- Persisted schemas and migrations (database schema stays at version 20),
  gateway, runtime and model selection, and dependencies.
- No whole-run retry, no repair attempts beyond General's, no silent source
  dropping and no deterministic prose.

Assumptions/blockers:
- Operator decision (2026-09-23): a Contract summary may be produced from an
  incomplete catalog when the unquotable clauses are listed explicitly.
- The two real contracts contain private contact data and are never committed.
  Deterministic fixtures reproduce their shapes synthetically.
- Parity alone may yield thin long-contract summaries. Run C's General summary
  cited 3 of 10 pages and did not cover the termination, term,
  limitation-of-liability or insurance clauses.
  Long-contract materiality is measured and reported by live acceptance, not
  guaranteed by this slice.
- No blocker is known.

Verification plan:
- Fail-first deterministic regressions, using a scripted runtime and synthetic
  contract fixtures:
  1. A long Contract catalog with one sentence unit over 600 characters.
     Before: ledger fallback with zero Synthesize requests. After: source
     selection and synthesis requests are issued and the result is `coherent`.
  2. A replay of run B's response shape: a windowed Contract unit citing two
     windows and ending at exactly 1,200 characters mid-sentence. Before:
     `Failed` with `MODEL_SUMMARY_RESPONSE_INVALID`. After: window ownership is
     detected first and one bound repair is made. A valid repair is accepted; an
     invalid repair yields the safe-sibling fallback with its warning; with no
     safe unit the stage fails closed.
  3. A single-window clipped Contract unit: 1,199 characters and incomplete is
     invalid; 1,200 characters and incomplete is clipped and repaired; 1,200
     characters and complete is valid.
  4. A persisted `9.0.0` Contract fallback artifact with omissions reopens and
     validates unchanged. A `10.0.0` artifact from the same input validates as
     coherent. A `9.0.0` fallback relabeled `10.0.0` is rejected.
  5. The existing General and Story suites pass unchanged apart from version
     identity.
  6. The unquoted list: equality with the warning for inspected pages; an empty
     list with zero omissions; 600- versus 601-character units; a no-terminal
     tail; clause reference present and absent; the 80-character word-boundary
     cut; empty for non-Contract profiles and for version-12 analysis; an
     integrity error on contradiction; never present in summary text or
     integrity hashes.
  7. The existing version-13 quote tests pass unchanged, and the exposed spans
     reproduce the existing segments and counts on those fixtures.
  8. Restart: interrupting after a completed repair request resumes without
     resubmission, and a resumed request with a different semantic hash never
     reuses the ledger entry.
- Gates: the locked all-target/all-feature Rust suite, strict Clippy with
  warnings denied for Linux and the Windows target as CI runs it,
  `cargo fmt --check`, `npm run build` and `git diff --check`.
- Opt-in live acceptance, reported and not committed: both private contracts on
  the installed-equivalent build with `qwen3-30b-a3b` at 100% GPU. Each ends
  `coherent` or with a named, disclosed fallback reason; verification passes;
  the unquoted list covers the recorded omissions (18 units on the agreement,
  23 on the subcontract, in the runs above). The report states which key terms
  appear: parties, term, payment, termination, liability, indemnity, insurance
  and governing law. Also re-run the public independent-contractor agreement
  packet from `docs/OFFICE_ACCEPTANCE.md`.
