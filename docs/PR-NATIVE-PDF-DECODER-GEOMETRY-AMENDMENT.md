# Proposed r3 amendment: preserve geometry at the decoder owner

Status: PROPOSED; requires independent contract review and operator acceptance. No dependency or production edit is authorized yet.

## Root cause and evidence

This amends accepted r2 SHA256 09e7b5a912cf29927a34c7678d341d2014cee80278a9a6fb2cec186ade9de20e only where its callback-only approach failed. All other r2 parity, identity, privacy, fidelity and model-dispatch holds remain.

Public proof at 7bf5921: the existing native parser emits drawing order rather than reference reading order. Two additional text-only PDFs produce identical OutputDev event streams while pinned PDFPlumber returns `A B` and `A\nB`. Oversight independently confirmed the STOP in 1ca6bc09. Manifest: geometry-stop-manifest.json SHA256 5bdaee18662d68871dffce5cd1d98b465b2d9b274335042342b72549e962cebc. LayoutEvents captures the callbacks exercised by these text-only twins; it does not implement stroke/fill for arbitrary PDFs.

At pdf-extract 0.12.0 lib.rs:1290–1314, the decoder has the resolved font and text state but emits only transform, width, spacing, font size and text. OutputDev's signature (:1882) discards font geometry. The reference's PDFMiner LTChar (layout.py:372–392) needs more information to form character boxes. Downstream sorting cannot recover it. The missing interface information is a defect in our proposed port boundary; the production library has not been modified.

## Inventory reused and new departure

The frozen reference remains PDFPlumber 0.11.10 / PDFMiner.six 20260107 with unchanged defaults and reference-runtime.json source hashes. It reproduces all ten pages and the entire frozen second-source text exactly. Existing model/server/prompt inventory, actual-page provenance, first-source OCR proof and all historical qualification evidence remain the r2 baseline. No model settings or source text are changed by this amendment.

The newly proposed departure is a narrowly patched, repository-local copy of the existing pdf-extract 0.12.0 dependency. Cargo.lock currently records registry checksum 417e8fdc940f1d5bc62c5f89864c3a2255f74f69aa353c98509213d67df61e73; installed lib.rs SHA256 is 90037ccde8781e158a0e60470467682975f81e0c8f3e17ac43d53596c2faab84. decoder-source-inventory.json records the available source and VCS metadata. Before modifying a copy, obtain and verify the crate archive against the lock checksum, then compare its sources with the inventory. The current cache alone is not archive-integrity proof. A mismatch stops preparation. Preserve the upstream MIT attribution and any notices for reference-derived metrics/code; record their provenance. No package-version upgrade or transitive dependency update is part of this scope.

## Required geometry: first-class data, one decoder owner

Add an opt-in enriched glyph interface at the decoder callsite, supplied by its resolved font owner. Keep the legacy OutputDev API and old consumers behaviorally unchanged. A default compatibility path must preserve their existing call sequence, text, coordinates and errors; do not change PlainTextOutput to repair the new consumer indirectly.

The enriched record must contain decoded text and source occurrence order, resolved glyph bounds, advance, size and upright/writing-direction information, with enough raw provenance to audit each derivation. Geometry and font metrics are resolved once inside the decoder; the app consumes the resulting record and must not re-read font dictionaries or infer font identity from characters.

Reference-required inputs and branches to account for explicitly:

| Input | Source semantics to port and test |
|---|---|
| Simple-font descent | PDFMiner pdffont.py:905–941: descriptor/default resolution, default 0, positive descent sign normalization, vertical font scale. |
| Base-14 metrics | pdffont.py:1055: FontMetricsDB lookup precedes the descriptor/width fallback. Test precedence as implemented by the reference, not merely absence of a descriptor. |
| Type3 metrics | pdffont.py:1092–1095: FontBBox descent/ascent and FontMatrix horizontal/vertical scales. |
| Horizontal advance | Resolved character width, font size and horizontal scaling, with character/word spacing and TJ adjustments reflected in subsequent glyph positions exactly once. |
| Vertical writing | pdffont.py:1163–1180 and :1236–1238: writing mode, W2/DW2 displacement, default vx/vy, vertical advance. Exposing a fabricated horizontal rectangle is not support. |
| Transforms and orientation | Text matrix, page/content transforms, rise, scaling, rotation, normalized bounds and the reference upright test. Current trm already includes rise/scaling: do not apply either twice. |
| Page coordinates | Actual page index, dimensions/origin, rotation and the same coordinate convention used by the reference line grouping. Preserve page provenance and duplicates. |

Do not claim a descent field alone completes this interface. Trace the reference character fields used by line/word grouping, then demonstrate them on public fixtures. If any needed field or decoder behavior is still missing, stop and record it; do not silently use zeros, horizontal mode or fallback text. This is a qualification stop, not authorization to add new application rejection policies or page limits.

## Change surface and sequencing

After acceptance, stage the verified dependency under src-tauri/vendor/pdf-extract and wire only that package through src-tauri/Cargo.toml/Cargo.lock. Record a pristine-source manifest plus a separate reviewed delta. Changed vendored code is restricted to resolved font metrics, enriched glyph geometry/emission, and their directly required helpers/tests/metric data. Do not include build products or the cache's machine metadata. Preserve attribution. The vendored source is retained maintenance responsibility and must be identified in future dependency updates.

Phase A is offline interface proof, before changing the application's native parsing selection. Existing native/PlainTextOutput behavior remains in place. Public fixtures must prove enriched geometry equals the pinned reference and the old consumer outputs remain unchanged. This phase may extend the existing parser_layout_tests.rs diagnostics and add focused vendor tests.

Only after Phase A passes, resume the already accepted r2 application assembly work: one reading-order owner, its explicit `0.12.0+reading-order.1` identity, existing-artifact immutability tests, exact private extraction parity, and unchanged dependency/writer/C9/SAME60 gates. No geometry recovery or document-specific exceptions downstream. Any extra normalization or text decoding discrepancy is recorded before proposing additional scope.

## Verification and stop rules

1. Reuse the preserved fail-first ordering fixture and descent twins. The twins must now yield distinct correct geometry and exact reference text. Removing the new metric propagation must make the twin test fail. The old callback still emits the same streams on the twins; compatibility and sufficiency are separate assertions.
2. Freeze public fixtures and their pinned-reference expected glyphs/text before implementation. Cover descriptor descent missing/zero/positive/negative, Base-14 precedence, Type3 non-default matrix, horizontal and vertical displacement/advance, transformed/rotated pages, superscripts, duplicates, mixed sizes, columns/tables and continuation across pages. Exercise both sides of reference tolerances without changing them. Add ordinary ordered-text controls and real numbered headings.
3. Compare raw geometry with a fixed floating-point comparison rule chosen from serialization precision before inspecting private text; exact final text is mandatory. Do not loosen numeric tolerances to hide changed clustering. Missing/nonfinite geometry cannot be treated as a successful comparison. Preserve every differing field and character index.
4. Capture old OutputDev/PlainTextOutput behavior on the same fixtures before the dependency patch; verify it stays unchanged, including error paths. Keep OCR/TaggedOcrParser unchanged. Run focused library/parser tests and fmt/clippy. Windows, Linux and macOS compile/test evidence is required before merge.
5. Only after public geometry proof, run the bound original second PDF against the richer decoder and r2 assembly. Require exact per-page extraction and reconstructed frozen-text hash. Keep real page provenance; never substitute the frozen flat text as production parser output.
6. Reuse r2's complete identity, persistence, actual-PDF dependency, writer planning and historical offline regression gates. Newly exposed discrepancies remain failures, not permission for grammar filters or reference tuning.

## Explicit non-scope

No full extraction-library replacement, Python runtime or sidecar, new service/capability, transitive upgrades, unrelated vendor fixes, global change to old extraction consumers, admission-policy change, database migration, OCR change, benchmark-label access, model/prompt/schema change, GPU/model/OCR calls, installation or held-stack merge. The earlier model-run approval persists; actual dispatch still requires successful source gates and a fresh code-derived request/budget freeze.

## Assumptions, costs and blockers

A compatible geometry extension is a proposed implementation, not proven sufficient. Maintaining a vendored patch is a real cost. The existing library also has differences outside the exposed callback; Phase A must identify any such difference needed for the enumerated public cases rather than silently broadening this patch. Archive provenance and licence notices must be established before writing the dependency delta. The r2 STOP remains in force until this amendment is independently reviewed and explicitly accepted.

## Implementation summary / cold diff / gap audit

NOT DONE. This amendment and source inventory are documentation only. No dependency, parser, runtime or test behavior changed. The preserved public evidence establishes why the earlier interface is insufficient, not that this extension works. The next implementation action after acceptance is fixture/reference freezing and verified vendor staging, followed by fail-first geometry tests.
