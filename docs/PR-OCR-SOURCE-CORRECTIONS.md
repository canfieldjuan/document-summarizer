# OCR source correction in the existing desktop

## Root cause

DocSum displays OCR text as citation evidence but has no correction command.
Exact-substring grounding validates against the OCR transcription, so a misread
name can survive verification. A warning or confidence cutoff does not repair
it. The existing parser checkpoint, local SQLite database, recent-run view and
Continue action provide the required execution path without another service.

## Required change surface

- Add a desktop OCR review panel for an idle completed, failed, or Parsed OCR
  run with a parsed artifact. Parsed eligibility allows revising a saved
  correction before Continue; active jobs remain unavailable. Show page text,
  allow explicit page-text corrections and open the
  retained source PDF for comparison. Render all document text as text.
- Save a separate document/run at the Parsed checkpoint. The PDF content hash
  continues to identify the unchanged PDF; a separate immutable correction
  record identifies the operator text, its hash, source run and source document.
  Keep the original parsed artifact, PDF, summary and Connect result unchanged.
- Preserve page numbering and unchanged pages. Reject native inputs, unknown or
  duplicate pages, empty edits, unchanged submissions, stale state/hash and
  over-limit text. Reuse the OCR admission bound of 262144 UTF-8 bytes across
  all corrected pages; never silently truncate.
- Admit the new document/run, parsed artifact, lineage, profiles and state
  events atomically in SQLite. Allow one corrected successor per source
  document. Identical request replay returns that successor; competing edits
  fail clearly. Later corrections use the successor, not the old document.
- Carry the existing summary/model profiles forward. Saving calls no model.
  Use the existing Continue action to regenerate and existing retry path after
  failure. Every retry of the corrected document must use its immutable
  corrected text rather than reintroducing the original OCR error.
- Keep OcrText provenance and propagate an OPERATOR_CORRECTED_OCR_TEXT warning
  through normalization, summaries and citation display. Operator text is not
  represented as an independently verified machine transcription.
- Surface lineage and the corrected successor in recent work. Source opening
  accepts a stored run identity, validates the retained PDF hash, and does not
  accept an arbitrary path or URL from the frontend.

Likely files: pipeline/corrections.rs (new), db.rs, schema.rs, parser.rs,
workspace.rs, mod.rs, lib.rs, src/main.ts, src/styles.css, index.html and focused
tests beside the relevant core code. One additive local schema migration is
required; no public Connect schema change.

## Explicit non-scope

No OCR model/runtime/dependency or prompt changes, confidence filtering,
automatic correction, invoice row/header editor changes, Connect redelivery or
mutation of completed jobs, new service/queue, final-prose editor, or model
promotion. This closes the local DocSum source correction path; it does not
qualify OCR accuracy or silently correct existing downstream consumer records.
PR document-ocr #12 remains separate; newer OCR models wait until its merge.

## Assumptions/blockers

The operator supplies corrections after comparing the PDF. The app cannot
determine the truth of handwriting or missing information. Existing retained
OCR PDFs preserve the original visual page. A missing/changed PDF must report
an error when opened. Completed Connect outputs are immutable; corrections
create a local result that is explicitly separate from already delivered data.

## Verification plan

1. Regression: save a correction, invoke the real parser/normalizer path for a
   retry, and require the corrected name instead of the old OCR name. Run it
   before the parser overlay integration and expect an assertion failure.
2. Prove immutable original artifacts, unchanged PDF bytes, atomic rollback,
   concurrent edit conflict, idempotent replay, restart and corrected retry.
3. Probe native rejection, mixed unchanged/edited pages, unknown/duplicate
   pages, no-op, empty text, byte-limit boundaries and stale version/hash.
4. Drive the existing summary pipeline with deterministic test runtime output;
   prove delivered citations quote corrected source and warnings disclose it.
5. Build frontend; focused Rust regressions/adjacent suites, cargo fmt and
   clippy. Full Rust tests before push because this changes persistent state.
   Exercise the desktop UI in an isolated browser fixture; distinguish this
   from a live-model or installed-app qualification.

## Implementation summary

The core owner is `pipeline/corrections.rs`. It creates a new Parsed run and
immutable correction record in one immediate SQLite transaction. Schema 21
adds the correction table and immutable triggers. Existing DB transition and
profile helpers create the new checkpoint; the parser checks for a correction
when retrying. The desktop exposes review, source opening, and save commands.
The shared review panel displays pages and lineage and opens the new run for
the existing Continue action. No inference runs during save.

Verification completed on Linux:

- Fail-first `corrected_document_reparse_uses_operator_text`: observed the old
  OCR text instead of the supplied correction before integrating the parser
  lookup. Passed after integration.
- Seven focused correction tests pass: retry, restart/replay, conflict,
  transactional rollback, page/UTF-8 boundaries, concurrent writers, completed
  summary/citations, and source PDF identity (some tests cover multiple cases).
- Full `cargo test --locked --all-targets --all-features`: library 560 passed,
  14 ignored; office acceptance 3 passed, 3 ignored; release contract 8 passed.
- `cargo fmt --all -- --check`, strict all-target/all-feature Clippy, and
  `npm run build` pass.
- Browser fixture using the production frontend and mocked desktop commands:
  no-op save rejected, changed page saved, other page preserved, separate
  Parsed run shown, Continue displays the corrected summary, original summary
  remains unchanged, source/successor navigation works. Console errors: none.
  Screenshot capture timed out, so visual verification is incomplete.

The browser fixture is not installed desktop or real-model qualification.
Source opening was clicked through mocked IPC; the real path/hash behavior is
covered by the Rust test, not a launched PDF viewer. No new inference was run.

## Cold diff audit

| Files | Contract responsibility |
| --- | --- |
| `corrections.rs` | Strict edit input, stable source/version/hash, bounded page edits, atomic lineage/replay, source PDF identity, and regression proofs |
| `db.rs`, `schema.rs` | Additive immutable storage and existing run/profile/state-event admission inside the caller transaction |
| `parser.rs`, `mod.rs` | Reuse saved correction on retry; native and uncorrected documents retain the parser path |
| `lib.rs` | Desktop commands, existing activity checks, and stored-identity source opening |
| `src/main.ts`, `index.html`, `src/styles.css` | Plain-text page review, save/error states, lineage navigation, existing Continue flow |
| `CONTRACTS.md`, this file | Implemented behavior, scope, and verification limits |

boundary-probe: valid edits and exact byte cap admitted; native, unknown or
duplicate pages, blank/no-op edits, stale version/hash, and over-cap UTF-8 text
rejected. A late persistence failure leaves no partial document/run; concurrent
connections admit only one successor. The pipeline consumes the stored
correction, as proved through retry and completed-summary citations.

effect-trace: correct OCR source before regeneration | immutable parsed
correction selected at the parser checkpoint | fail-first retry changed from
original to corrected text; completed pipeline cites corrected text while the
prior persisted summary remains identical.

No dependency, runtime/model, public Connect schema, prompt, or completed-job
mutation is in the diff. Native rejection and source-file integrity tests cover
the admission and PDF-opening boundaries. The frontend constructs text nodes
and textareas rather than interpreting document content as markup.

## Gap audit

DONE for local implementation and deterministic verification. NOT DONE for
merge: CI and independent PR review remain. Installed desktop/PDF-viewer proof,
live-model summary quality, Windows execution, and downstream correction
redelivery were not established by this work. External corpus/model tests stay
explicitly ignored. This does not qualify an OCR engine or close document-ocr
#12's operator quality decision.
