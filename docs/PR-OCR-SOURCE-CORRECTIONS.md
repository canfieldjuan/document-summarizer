# OCR source correction in the existing desktop

## Root cause

DocSum displays OCR text as citation evidence but has no correction command.
Exact-substring grounding validates against the OCR transcription, so a misread
name can survive verification. A warning or confidence cutoff does not repair
it. The existing parser checkpoint, local SQLite database, recent-run view and
Continue action provide the required execution path without another service.

## Required change surface

- Add a desktop OCR review panel for a completed or failed OCR run with a parsed
  artifact. Show page text, allow explicit page-text corrections and open the
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

Pending implementation.

## Cold diff audit

Pending implementation and verification.

## Gap audit

NOT DONE. Implementation and proof remain.
