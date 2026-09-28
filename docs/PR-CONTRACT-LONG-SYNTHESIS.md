# Long Contract synthesis parity and unquoted-passage disclosure

Status: accepted at `cd373aa`; R1-R7 implemented in `19a5917` and `601e1df`.
Amendment 1 (R1 continuation, R6 opening-text cap, R8 Contract invalid-output
ledger fallback) is accepted and is not implemented.

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
     `3989`, `4023-4038`, `4083-4093`; `parse_window_repair_requirements`
     returns early for non-General at `6498`), unlike General after PR #63.
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
- What run B does and does not establish: B is a gate-only probe. It kept the
  same incomplete catalog (its Analyze warnings still report 18 omitted units).
  It establishes that clipping masks Contract's mixed-window handling. B's unit
  was both clipped and cross-window. A clipped Contract unit returns
  `MODEL_SUMMARY_RESPONSE_INVALID` inside the per-unit checks, before window
  ownership is examined (`summary/coherent.rs:6347-6371`), so the stage failed.
  It also shows that such responses occur on real input.
- B did not exercise Contract's existing mixed-window path. A complete
  mixed-window Contract unit returns `WINDOW_MIXED_RESPONSE_CODE` (`6401-6405`).
  The repair loop then makes one generic repair request (`3988-4067`), and the
  safe-sibling window fallback is already available to Contract
  (`parse_response_without_mixed_windows`, `6641-6642`). That repair is not
  bound to the rejected response (`3989`, `4030-4038`, `4083-4093`).
- B does not establish what a complete catalog would produce, because
  completing the catalog changes the candidate set, selection windows, prompt
  and response.
- Why quote segmentation is not this slice: segmentation remains an open,
  separate follow-up, not a rejected option. It cannot replace R2-R4 because R3
  and R4 govern model responses whatever the catalog contains, and because
  measured catalogs keep omissions that boundary splitting cannot remove. All 10
  page tails lack a safe terminal. In an approximate estimate over the 31
  over-limit units, 9 stayed over 600 characters after clause-number and
  enumerator splits. Any claim about segmentation's end-to-end effect requires a
  complete-catalog live probe in that slice.
- Separately, the user sees only a count of unquotable source units, never
  which passages they are.
- Amendment 1 evidence, live acceptance of `4b127fe` on 2026-09-23 (Contract
  profile, `qwen3-30b-a3b` with 49/49 layers on GPU):
  - The 10-page agreement ended `coherent` and verified, with 6 summary units.
    Its 18 unquoted passages agreed with the recorded warning.
  - The 15-page subcontract ended `Failed` with
    `MODEL_SUMMARY_RESPONSE_INVALID`. The final synthesis response had 8 units.
    Units 3-7 repeated identical text with identical source IDs, and
    `summary/coherent.rs:6466-6467` rejects a repeated (text, evidence) unit.
    The loop returned that error with no repair request (Synthesize ordinals
    0-9, zero Verify requests).
  - Under `9.0.0` the same document completed as `claimLedgerFallback`, because
    the incomplete-catalog gate stopped it before synthesis. R2 exposed it to a
    model-output failure that `9.0.0` never reached.
- Amendment 1, review finding F-A: `unquoted_opening_text` (`summary.rs`)
  returns 81 characters in two cases. A first word over 80 characters is split
  and then marked. An exactly-80-character first word followed by another word
  is also marked past the cap.
- Amendment 1, review finding F-B: `required_short_contract_evidence_ids` and
  `validate_verified_profile` (`summary/coherent.rs`) rebuild the Contract
  catalog with the current `VERSION`, while Contract evidence identities bind
  the synthesis version. A short Contract synthesized under `9.0.0` that
  continues into verification under this build therefore compares identities
  from different versions.

Required behavior:
- R1 Version. New runs use synthesis version `10.0.0`. Artifacts persisted
  under `9.0.0` or earlier validate and reopen under their recorded version's
  rules, including the `9.0.0` Contract incomplete-catalog fallback. Under
  `10.0.0`, General and Story behavior is identical to `9.0.0` except for the
  version identity bound into durable identifiers. This includes continuation:
  when a synthesis artifact was persisted under `9.0.0` and its verification
  runs under this build, the short-Contract clause and material-term checks
  rebuild the Contract catalog with the persisted synthesis version, not the
  current one.
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
- R4 Cross-window ownership, and binding the existing repair. Contract already
  detects a complete mixed-window unit, makes one generic window repair and has
  the safe-sibling window fallback. R4 hardens that existing path; it adds no
  new repair. Under `10.0.0`:
  - Contract determines cross-window ownership from validated response source
    IDs before per-unit clipping or completion validation can mask it.
  - The existing window repair is bound to the rejected response exactly as
    General's is after PR #63. The repair prompt carries the rejected response.
    The repair is accepted only when valid siblings are unchanged and every
    mixed unit's original evidence is covered exactly once by single-window
    replacement units.
  - A repair that violates the binding uses the existing safe-sibling fallback
    with `COHERENT_SUMMARY_CROSS_WINDOW_UNITS_WITHHELD`. The stage fails closed
    when no safe result exists.
  - Contract keeps its existing budget of two validation repairs. Window repair
    and clipped repair each remain one attempt.
- R5 Contract source selection is unchanged, including its identity/scope and
  risk/exit source requirements.
- R6 Unquoted-passage disclosure (operator decision, 2026-09-23, narrowed in
  review from clauses to source passages). For a
  Contract-profile run whose analysis version is `13.0.0`, the desktop summary
  view returned by `get_persisted_summary` carries `unquotedSourceUnits`. It
  has one entry per source unit that the analysis quote catalog omits, across
  every native-text page, in source order. Each entry contains:
  - `pageNumber`
  - `kind`: `overLimitSentence` for a safe-boundary unit over 600 characters,
    or `noSentenceBoundary` for a nonempty tail without a safe terminal
  - `characterCount`
  - `clauseReference`: the numbered clause reference that begins the passage,
    found by the existing contract clause detector, or `null`. This is a
    locator only, not a statement about that clause's coverage.
  - `openingText`: at most 80 characters in total, including a trailing `…`
    marker when shortened. It contains only whole words of the passage,
    joined by single spaces:
    - If the whitespace-normalized passage fits in 80 characters, it is
      returned whole, with no marker.
    - Otherwise it is the longest whole-word prefix of at most 79 characters,
      followed by `…`.
    - If the first word alone exceeds 79 characters, `openingText` is `…`
      only. The page, kind, character count and clause locator still identify
      the passage.

  The list is derived on read from the persisted normalized document and the
  run's recorded analysis version, through the same segmentation function that
  produces the omission counts. Nothing new is persisted. For other profiles,
  and for analysis versions that record no quote-boundary omissions, the list is
  present and empty. The desktop UI renders a "Passages that could not be
  quoted" section under both `coherent` and `claimLedgerFallback` presentations
  whenever the list is nonempty. It states only the fact the derivation proves:
  each listed source passage could not enter the quotation catalog, so no
  summary unit or claim cites that passage. It also states that other passages
  from the same clause or page may still be quoted and cited. It makes no claim
  that a clause as a whole is uncovered; clause-level coverage is not computed
  in this slice. `openingText` is display-only: it is never a
  citation, never sent to a model, and never part of summary text, integrity
  hashes, verification or Connect results.
- R8 Contract invalid-output ledger fallback (amendment 1, operator decision
  2026-09-23). R8 applies only to `10.0.0` Contract runs whose catalog R2
  admitted: the catalog has quote-boundary omissions and at least one
  candidate. For such a run, synthesis may stop because it rejected a model
  response and has neither a remaining permitted repair nor a safe coherent
  result. In that case it persists the verified-ledger fallback instead of
  failing the stage.
  - Complete Contract catalogs (no omitted source units) are outside R8,
    including complete short Contracts and complete long Contracts. They keep
    failing closed exactly as today. This includes short-Contract
    coverage-repair exhaustion (`summary/coherent.rs:4151-4163`).
  - Eligibility depends on where the failure originates, not on its error code.
    Eligible exits:
    - the repair loop rejects a response after its permitted repairs and
      `take_generated_fallback` yields no safe coherent result. This covers
      the loop's `Err(failure)` arm, its window-repair and clipped-unit
      integrity exits, and Contract validation-repair exhaustion on an
      R2-admitted catalog;
    - a repair request cannot fit the synthesis context after a rejected
      response (`SYNTHESIS_REPAIR_INPUT_TOO_LARGE`);
    - a source-selection response is invalid
      (`MODEL_SOURCE_SELECTION_RESPONSE_INVALID`). Selection has no repair.
  - These keep failing exactly as today:
    - cancellation;
    - runtime failures: model transport, runtime health, runtime metadata and
      context-preflight errors;
    - corrupt or invalid source artifacts;
    - application invariants, including those that reuse the
      `MODEL_SUMMARY_RESPONSE_INVALID` code (for example the repair schema's
      `maxItems` pointer), plus `SOURCE_SELECTION_PLAN_INVALID`, invalid
      budgets and `INVALID_SYNTHESIZED_DOCUMENT`;
    - every failure in the verification stage.
  - The fallback is `fallback_document` with a new fallback reason:
    presentation `claimLedgerFallback`, and summary text rendered only from the
    source-ordered claim ledger. It has no summary units and no synthesis
    evidence, so no rejected unit reaches the summary. The claim ledger then
    goes through normal verification.
  - The warning has its own code, `COHERENT_SUMMARY_MODEL_OUTPUT_INVALID`,
    at the Synthesize stage, with the message "The model's source selection
    or summary output was invalid; showing verified source claims instead".
    The message covers selection and synthesis and does not claim a repair
    ran. It does not reuse `COHERENT_SUMMARY_SOURCE_CONTEXT_TOO_LARGE`
    (`summary/coherent.rs:11`), because Connect delivers warning codes to
    consumers verbatim (`connect/contracts.rs:461-466`). A distinct fallback
    code already exists for a different fallback,
    `SUMMARY_DELIVERY_COVERAGE_FALLBACK` (`summary.rs:114`).
  - A `claimLedgerFallback` synthesis artifact carries exactly one fallback
    warning at the Synthesize stage. It is either the source-context code with
    its reason message, or the invalid-output code (R8 only), never both. A
    `coherent` artifact carries neither. `validate_content`
    (`summary/coherent.rs:7443-7446`, `7492`) enforces this.
  - `validate_for_runtime` accepts the invalid-output fallback only for
    Contract at `10.0.0`, and only when the rebuilt catalog shows the R2 path:
    omitted source units present, at least one candidate, and no
    catalog-determined fallback. It rejects the fallback when labeled
    General, Story or `9.0.0`, or when the Contract catalog is complete.
  - The desktop's static fallback note (`index.html:148-150`) currently
    attributes every ledger fallback to "the bounded source-context rules". It
    becomes cause-neutral: "A coherent summary could not be produced.
    Verified source claims are shown instead." The processing notes carry the
    reason.
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
- Per-page agreement: for every page in the analyzed artifact's
  `inspected_pages`, the number of derived entries on that page equals the
  omitted-unit count that `versioned_page_scope` computes for the page. That is
  the same per-page value `validate_plan` already recomputes
  (`summary/pages.rs:1235-1251`).
- Warning agreement: the `ANALYSIS_QUOTE_BOUNDARY_OMITTED` warning carries only
  two counts and no page set (`summary/pages.rs:1053-1070`), so both are checked
  separately. The sum of derived entries over inspected pages equals the
  warning's omitted-unit count, and the number of inspected pages with at least
  one entry equals its affected-page count. With no entries on inspected pages,
  the warning is absent.
- The list may also include pages analysis did not inspect, because synthesis
  catalogs every chunk. Those entries are not compared with the warning.
- A coherent Contract summary built from an incomplete catalog still carries
  the Analyze quote-boundary warning. Disclosure never replaces warnings.
- Durable identifiers created under `10.0.0` bind `10.0.0`. A persisted `9.0.0`
  artifact is never re-derived or relabeled under `10.0.0`.

Failure cases:
- Model output that is invalid and not repairable under R3/R4 fails the stage
  as recoverable, as today, for General, Story and complete Contract catalogs.
  For an R2-admitted `10.0.0` Contract catalog it selects the R8 fallback,
  which is disclosed by its own warning code. No failure outside R8's eligible
  exits and R2 boundary selects that fallback.
- A runtime context rejection keeps the existing ledger fallback and its
  boundary-specific message.
- If the normalized document or analyzed artifact is missing or invalid on
  read, or the derived entries violate the per-page or warning agreement above,
  the summary view fails with the existing workspace integrity error. An error
  is never presented as an empty list.

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
    `parse_window_repair_requirements` (`6498`), the General-only branches of
    the repair loop (`3856`, `3989`, `4023-4038`), and the rejected-response
    acceptance check that binds the existing repair (`4083-4093`);
  - leave General-only source framing
    (`parse_response_without_mixed_source_framing_units`, `6580`) and all Story
    branches untouched.
- `src-tauri/src/pipeline/workspace.rs`: `SummaryView` gains
  `unquoted_source_units`, derived in `get_persisted_summary`.
- Amendment 1:
  - `summary.rs`: `unquoted_opening_text` (F-A), and the verification-stage
    call site that passes the persisted synthesis version (F-B).
  - `summary/coherent.rs`:
    - `required_short_contract_evidence_ids` and `validate_verified_profile`
      take the synthesis version (F-B);
    - the invalid-output fallback and its distinct warning code;
    - an origin-classified conversion to the R8 fallback in
      `synthesize_with_delivery_coverage`, for R2-admitted Contract catalogs
      at `10.0.0`;
    - the matching `validate_for_runtime` arm, and the `validate_content`
      fallback-warning rule.
  - `index.html`: the cause-neutral fallback note.
- `src/main.ts` and `index.html`: the view type and the "Passages that could not be quoted"
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
  citations, and the Connect result schema and its fields. R8 can change a
  delivered Contract result's warnings and presentation mode.
- Persisted schemas and migrations (database schema stays at version 20),
  gateway, runtime and model selection, and dependencies.
- No whole-run retry, no repair attempts beyond General's, no silent source
  dropping and no deterministic prose.
- R8 changes only what a failed Contract synthesis presents. It does not claim
  that coherent summaries cover material terms. In live acceptance, the
  10-page agreement's coherent summary cited pages 1, 4 and 5 only. It did not
  state the contract term, insurance or governing law, and it mentioned
  indemnification only as a reference to "the indemnification section".
  Material-term coverage and source-selection quality for long Contract
  summaries need their own review. Per the operator, that material-term
  coverage gap is a separate merge blocker, and amendment 1 alone does not
  make #92 mergeable.

Assumptions/blockers:
- Operator decision (2026-09-23): a Contract summary may be produced from an
  incomplete catalog when the unquotable source passages are listed explicitly.
- The two real contracts contain private contact data and are never committed.
  Deterministic fixtures reproduce their shapes synthetically.
- Parity alone may yield thin long-contract summaries. Run C's General summary
  cited 3 of 10 pages and did not cover the termination, term,
  limitation-of-liability or insurance clauses.
  Long-contract materiality is measured and reported by live acceptance, not
  guaranteed by this slice.
- Operator decision (2026-09-23): the Contract-only R8 fallback, limited to
  R2-admitted `10.0.0` catalogs and the eligible exits above, with its own
  warning code.
- Amendment 1 decision for F-A: the `…` marker counts inside the 80-character
  cap, and a first word that cannot fit yields the marker alone rather than a
  split word.
- F-A shipped because the R6 test allowed `UNQUOTED_OPENING_CHARACTERS + 1`
  characters and never probed a long first word.

Verification plan:
- Fail-first deterministic regressions, using a scripted runtime and synthetic
  contract fixtures:
  1. A long Contract catalog with one sentence unit over 600 characters.
     Before: ledger fallback with zero Synthesize requests. After: source
     selection and synthesis requests are issued and the result is `coherent`.
  2a. A replay of run B's response shape: a windowed Contract unit citing two
     windows and ending at exactly 1,200 characters mid-sentence. Before:
     `Failed` with `MODEL_SUMMARY_RESPONSE_INVALID`, because clipping masks
     ownership. After: window ownership is detected first and one bound repair
     is made. A valid repair is accepted; an invalid repair yields the
     safe-sibling fallback with its warning; with no safe unit the stage fails
     closed.
  2b. A non-clipped mixed-window regression, which preserves the existing path
     while enforcing the new binding: a complete (not clipped) windowed
     Contract unit citing two windows.
     - Both before and after: the parser returns `WINDOW_MIXED_RESPONSE_CODE`,
       not `MODEL_SUMMARY_RESPONSE_INVALID`, and exactly one window repair
       request is made.
     - Before (fail-first): a repair that drops one of the mixed unit's
       original sources, or rewrites a valid sibling, is accepted.
     - After: the repair prompt carries the rejected response. Such a repair is
       rejected and yields the safe-sibling fallback with
       `COHERENT_SUMMARY_CROSS_WINDOW_UNITS_WITHHELD`. A repair that re-mixes
       windows is rejected the same way. A valid single-window split covering
       every original source exactly once is accepted.
  3. A single-window clipped Contract unit: 1,199 characters and incomplete is
     invalid; 1,200 characters and incomplete is clipped and repaired; 1,200
     characters and complete is valid.
  4. A persisted `9.0.0` Contract fallback artifact with omissions reopens and
     validates unchanged. A `10.0.0` artifact from the same input validates as
     coherent. A `9.0.0` fallback relabeled `10.0.0` is rejected.
  5. The existing General and Story suites pass unchanged apart from version
     identity.
  6. The unquoted list: per-page equality with `versioned_page_scope` for every
     inspected page; both warning counts (units and affected pages) equal the
     sums; warning absent when inspected pages have no entries; entries on
     uninspected pages excluded from those comparisons; an empty list with zero
     omissions; 600- versus 601-character units; a no-terminal
     tail; clause reference present and absent; the 80-character word-boundary
     cut; empty for non-Contract profiles and for version-12 analysis; an
     integrity error on contradiction; never present in summary text or
     integrity hashes.
  7. The existing version-13 quote tests pass unchanged, and the exposed spans
     reproduce the existing segments and counts on those fixtures.
  8. Restart: interrupting after a completed repair request resumes without
     resubmission, and a resumed request with a different semantic hash never
     reuses the ledger entry.
  9. Amendment 1, live-shape replay (fail-first): a Contract synthesis
     response whose units repeat identical text and source IDs. Before:
     `Failed` with `MODEL_SUMMARY_RESPONSE_INVALID`. After:
     `claimLedgerFallback` with the invalid-output warning and verified ledger
     claims, and no rejected unit text in summary text or claims.
  10. Amendment 1, eligibility boundaries:
      - For an R2-admitted Contract catalog, repair exhaustion, repair input
        too large and an invalid source-selection response each yield the R8
        fallback.
      - These keep today's outcome: runtime failure, cancellation, an invalid
        source artifact, an application invariant that reuses
        `MODEL_SUMMARY_RESPONSE_INVALID`, and a failed verification.
      - General and Story given the eligible responses still fail.
      - A complete short Contract whose coverage repairs are exhausted still
        fails closed with `MODEL_SUMMARY_RESPONSE_INVALID`.
      - A complete long Contract given the live duplicate-unit response still
        fails.
  11. Amendment 1, validation:
      - A `10.0.0` Contract invalid-output fallback over an R2-admitted
        catalog validates.
      - The same fallback is rejected when labeled General, Story or `9.0.0`,
        when the Contract catalog is complete, and when the catalog requires a
        catalog-determined fallback.
      - Also rejected: a `coherent` artifact carrying the invalid-output code,
        and a fallback carrying both fallback codes or neither.
  12. Amendment 1, F-B (fail-first): a short Contract synthesized under
      `9.0.0` continues through verification under this build and completes.
      Before the fix it is expected to fail with
      `CONTRACT_SUMMARY_INCOMPLETE_AFTER_VERIFICATION`. That expectation comes
      from the review trace, confirmed by reading the code, and is not yet
      executed.
  13. Amendment 1, F-A (fail-first): passages of exactly 80 and 81
      characters, an exactly-80-character first word followed by another
      word, and an overlong first word. Every `openingText` is at most 80
      characters and contains only whole words or the lone marker.
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
