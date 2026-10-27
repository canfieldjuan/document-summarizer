# Proposed native PDF reading-order repair

Status: CONTRACT DRAFT revision 2 — oversight A1 incorporated; not accepted, no production implementation. Original draft and manifest preserved unchanged.

## Root cause

The native Parse owner constructs pdf-extract PlainTextOutput and immediately writes each decoded glyph in PDF content-stream order (parser.rs:95–110 at 7911f437; pdf-extract 0.12.0 lib.rs:2161–2194). Its position checks insert whitespace but never reorder text. A PDF may paint a visually earlier table after later paragraphs. The resulting native text moves that table after the operative clauses and splits superscripts into address fragments. WholeDocument then sees false numbered/heading-like boundaries. The parser is the first owner that loses usable reading order.

The retained production-parser receipt has a text mismatch on every page of preproduction-2. The original page-one raster confirms the table precedes the clauses. The prior public diagnostic reproduces a numeric address becoming a clause and an address fragment becoming an uncertain boundary. That evidence characterizes failures; it is not a passing repair test. The prior annex/form-code example also remains a required integration check, not a reason to drop text.

The earliest available parser blame reaches the local history boundary 91dce3c. The true introducing commit is not established. No new source implementation was made during this investigation.

## Validated inventory first

* Exact original PDF SHA256: d9c46b09a1abc5916a9b0194cd82f350800b843de998928ff1ce35df8349c67a.
* Frozen preproduction text SHA256: c454dc8f2ad50b9e1490c500b866994dbe60d2b013b770c7433fc49c4e8e593a.
* Fresh offline reproduction: PDFPlumber 0.11.10, page.extract_text() defaults, ten pages. Joining with the original [PAGE n] wrappers reproduces the entire frozen file byte for byte. The exact historical extraction executable/version is UNKNOWN; this establishes a reproducible equivalent, not historical provenance.
* The original summary_run.py consumes prepared text and does not extract PDFs. Current reference extraction clusters characters by page position; default x/y tolerances are 3, use_text_flow is false, with direction-aware line and character ordering. Freeze the installed reference modules and dependency versions before the implementation comparison; do not tune these values against private inputs.
* Existing whole-writer inventory: whole-writer-qualification-preparation-20261019/inventory.json. Prior combined candidate 548ae05ed90a245293504039a90f4f86bb00a96d passed the historical offline owners. Frozen text dependency inventory reports zero blocked units for both inputs; the native PDF diagnostic reports two blocked units for preproduction-2. This is an input-representation difference, not a model-capacity finding.
* Model and runtime remain Qwen3.5-9B, model SHA cd76ec205963b3b33350093e6904d9de16c4e666fd104e1f632d25c7f15f2a13, server SHA 0ca399edd758decd825a71823b04ba7ddbc8b2e10d2309d8bf623ee3c2283099. Original baseline: context 32768, temperature 0, thinking off, chat framing, four slots, default seed 4294967295, output allowance 1500. The recorded framing-repair result at 456fa8aa reproduced both saved summaries exactly. It proves same-input parity, not whole-document fidelity.
* Production's existing one-slot native completion adapter, repaired framing, stage-specific schema/output bounds, seed owner, and writer semantics are unchanged here. This repair performs no inference and introduces no runtime/model departure.

## Required change surface

One native page text assembly owner in src-tauri/src/pipeline/parser.rs, optionally a private parser layout module, plus its public tests. Replace immediate PlainTextOutput assembly with position-aware assembly using the existing pdf-extract OutputDev glyph events. Port the recovered reference's ordering behavior; do not invent address, footer, contract-name, or benchmark-specific rules.

First demonstrate that the existing glyph events expose enough geometry to reproduce the reference. This is a hard prerequisite to adopting the implementation. If they do not, preserve the discrepancy and revise the contract before changing decoding libraries, adding a Python sidecar, or adding new production dependencies. Do not approximate silently.

Keep actual PDF pages and source provenance. [PAGE n] wrappers are comparison metadata only, never injected as invented PDF source. Retain the complete text, including addresses, form codes, page numbers, exclusions and duplicates. Do not remove text to make the dependency checker pass. OCR tagged-text parsing, admission, and profile inheritance remain unchanged.

No downstream special cases are proposed. The existing address/form-code diagnostic is evidence, not a patch. Existing source grammar and uncertainty rules remain unchanged while the parser repair is measured. If the corrected extraction still fails them, report the remaining first divergence before seeking a separate grammar amendment.

## A1 — recorded assembly identity and immutable lineage

Confirmed against 7911f437: PdfExtractParser::version() returns only "0.12.0" (parser.rs:45–46). parse_inner records it in ParsedDocument (parser.rs:145–146); db.rs:2953–2971 persists both the parser columns and complete JSON. Reload reads the saved JSON (db.rs:1319–1332). The current source validates newly produced artifact identity against the selected parser (parser.rs:1001–1008). These distinguish versions only if the assembly change has a new identity.

The repaired native parser keeps id "pdf-extract" and records version "0.12.0+reading-order.1": the decoding library version plus an explicit application assembly revision, through the existing version field. Define that identity once in the native parser owner. Every newly produced native artifact must use it, and persisted columns and serialized JSON must agree. Changing assembly behavior again requires another explicit assembly revision. Tagged OCR and corrected-source identities remain unchanged. No serialization-shape change or database migration is needed.

Existing stored content-stream-order artifacts retain their original version and exact bytes. Loading, reopening, recovery or Continue must not relabel them, re-parse them implicitly, or substitute new-layout normalized/chunk/citation artifacts into their saved run. Each run's derived artifacts remain bound to its actual persisted parse. An explicit existing retry creates a separate attempt and may use the current parser; its prior run and evidence remain immutable. This does not prohibit an explicit upgrade retry for the same input document: it prohibits mixing old/new parse content within a run or misrepresenting their provenance.

Required public regressions, before adoption:

* Assert the new native identity, persist/reopen it, and compare parser columns with serialized artifact identity. The old-version constant mutant must fail.
* Seed a legacy-version parse and its derived artifacts, reopen and exercise the actual applicable recovery/Continue path with the new parser available. Assert byte-identical old parse, identity, derived provenance, and no implicit parse invocation. Check an explicit separate retry retains the old run and records the new identity only on its own artifacts.
* Supply a mismatched artifact/parser identity and prove the existing validation rejects it; verify OCR identities remain unchanged.

Required test surface additionally includes adjacent persistence/recovery/service tests where those paths are owned (pipeline/parser.rs, db.rs, recovery.rs, service.rs). Production changes remain at the native parser identity/assembly owner; if a test exposes a separate lineage mutation defect, trace it and revise this contract before expanding production writes. The amendment adds identity proof, not a new resume policy or a bulk migration.

## Every difference from the reference

1. Rust production replaces the reference's Python PDF decoder/geometry objects with existing pdf-extract glyph events. Equivalence is unproven and must pass before adoption.
2. Production returns real per-page ParsedPage values; the summary baseline uses one flat file with explicit page wrappers. Compare each page exactly and reconstruct the frozen wrapper format only in the test. Then run the unchanged dependency/assembly owners on actual paginated output; flat-text success alone is insufficient.
3. The first frozen input now uses reviewed retained OCR output. Its existing complete-source proof remains the oracle for that path; do not substitute the old incomplete native source or re-run OCR.
4. Reference extraction defaults become explicit test pins. No private parameter tuning is allowed.
5. Native parser metadata gains the explicit assembly revision in its existing version value. Old artifacts keep their identity; no in-place source conversion is allowed (A1).

## Explicit non-scope

No model/OCR calls under this repair contract; no changes to prompts, sampling, model, context/call limits, writer/Verify semantics, full clause output, citation rules, dependency resolver, furniture filtering, database schema, packaging, installed apps, or private benchmark labels. No new blanket page/size limits. No modification or push of the held PR stack until integration has the required gates. Model-run approval already exists and remains recorded; dispatch waits for valid sources and a fresh request/budget freeze.

## Verification plan and stop rules

1. Before the fix, add a public synthetic PDF regression where visual row order differs from content-stream order; include a table followed by clauses and superscript text. Assert the complete expected page text. It must fail against the current native owner for the documented ordering class, not merely fail a downstream admission check.
2. Add faithful/unsafe sibling fixtures: already ordered text, a real numbered heading, repeated text that must not be deduplicated, page-end continuation, a footer before an annex, empty/mixed native pages, and rotated/multi-column content. Use the pinned reference and direct rendering to check ordering and character preservation; discrepancies remain gaps rather than waived successes. Exercise PDF glyph transforms and tolerance boundaries on both sides. No address lexicon.
3. After repair, the same public PDF reproduction must pass. Reverting only the assembly change must kill the regression. Run adjacent parser/normalizer tests and fmt/clippy; platform CI must cover Linux, Windows, macOS before merge.
4. On the frozen second PDF, require exact per-page reference equality and exact reconstruction of the frozen text hash, without normalization that hides deletions or order changes. Report complete differences if this fails. Do not proceed to model calls.
5. Using actual page provenance, rerun the existing source/dependency and writer preparation owners. Compare full source-unit, dependency, citation-anchor and assembly outputs; account for all differences from the flat historical representation. Preserve all text and require zero unresolved source blockers. Also replay the retained first-input OCR parse path unchanged. Reuse existing harnesses; no new model test harness.
6. Run the existing unchanged historical C9/SAME60 offline regression gates at the integration candidate. Freeze exact code, inputs, request bodies and code-derived Analyze/Synthesize bounds only after these pass. Historical flat-text budgets do not authorize real-PDF dispatch.

Stop before adopting a candidate with unexplained layout, text, provenance or dependency differences. Technical defects within this declared parser surface remain owned by this session. A dependency/library, grammar, product-behavior or scope expansion requires a reviewed amendment and operator acceptance. All production/merge/PR116 qualification holds remain.

## Assumptions and blockers

Reference bytes are recoverable; historical extractor provenance is incomplete. The current glyph interface's sufficiency and paginated dependency parity are not yet established. Independent contract review and operator acceptance are required by the accepted offline continuation contract before source changes. No additional blanket model-run permission is requested.

## Implementation summary / cold audit / gap audit

NOT DONE: contract and diagnostic evidence only. Production checkout remains unchanged. The reproduction script reads the bound source and emits private text plus alias/hash-only comparison receipts. No parser fix, regression execution against a candidate, integration gate or model run is claimed.

## Operator acceptance

Accepted verbatim: "i accepy". Accepted contract SHA256 09e7b5a912cf29927a34c7678d341d2014cee80278a9a6fb2cec186ade9de20e. Oversight contract PASS: ad932fd4. This acceptance does not change its geometry-sufficiency stop rule.

## Geometry-sufficiency gate result

STOP: public twin PDFs produce identical OutputDev events but different pinned-reference line grouping because resolved font descent is absent from the callback. See durable evidence alias c9-native-layout-parity-20261027, geometry-stop-manifest.json SHA256 5bdaee18662d68871dffce5cd1d98b465b2d9b274335042342b72549e962cebc. The public drawing-order regression fails as expected; the interface-collision characterization passes. Production remains unchanged. Contract r2 requires amended scope before a decoder/interface change; no model run or repair adoption is allowed by this result.
