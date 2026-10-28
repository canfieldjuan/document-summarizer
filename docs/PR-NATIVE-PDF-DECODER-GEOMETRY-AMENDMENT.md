# Proposed r4 amendment: preserve geometry at the decoder owner

Status: ACCEPTED revision 4. The operator-approved metadata-derived licence notice cleared A4. A local opt-in geometry candidate is implemented, but Phase A stopped at a newly proven lopdf decimal-precision gap. Application adoption and all holds remain blocked.

## Root cause and evidence

This amends accepted r2 SHA256 09e7b5a912cf29927a34c7678d341d2014cee80278a9a6fb2cec186ade9de20e only where its callback-only approach failed. All other r2 parity, identity, privacy, fidelity and model-dispatch holds remain.

Public proof at 7bf5921: the existing native parser emits drawing order rather than reference reading order. Two additional text-only PDFs produce identical OutputDev event streams while pinned PDFPlumber returns `A B` and `A\nB`. Oversight independently confirmed the STOP in 1ca6bc09. Manifest: geometry-stop-manifest.json SHA256 5bdaee18662d68871dffce5cd1d98b465b2d9b274335042342b72549e962cebc. LayoutEvents captures the callbacks exercised by these text-only twins; it does not implement stroke/fill for arbitrary PDFs.

At pdf-extract 0.12.0 lib.rs:1290–1314, the decoder has the resolved font and text state but emits only transform, width, spacing, font size and text. OutputDev's signature (:1882) discards font geometry. The reference's PDFMiner LTChar (layout.py:372–392) needs more information to form character boxes. Downstream sorting cannot recover it. The missing interface information is a defect in our proposed port boundary; the production library has not been modified.

## Inventory reused and new departure

The frozen reference remains PDFPlumber 0.11.10 / PDFMiner.six 20260107 with unchanged defaults and reference-runtime.json source hashes. It reproduces all ten pages and the entire frozen second-source text exactly. Existing model/server/prompt inventory, actual-page provenance, first-source OCR proof and all historical qualification evidence remain the r2 baseline. No model settings or source text are changed by this amendment.

The newly proposed departure is a narrowly patched, repository-local copy of the existing pdf-extract 0.12.0 dependency. Cargo.lock currently records registry checksum 417e8fdc940f1d5bc62c5f89864c3a2255f74f69aa353c98509213d67df61e73; installed lib.rs SHA256 is 90037ccde8781e158a0e60470467682975f81e0c8f3e17ac43d53596c2faab84. decoder-source-inventory.json records the available source and VCS metadata. The cached archive has now been hashed against that lock checksum: all 12 archive files match the extracted cache, with no mismatches (crate-integrity-review.json). No crate download is needed. The archive contains no LICENSE file. Its .cargo_vcs_info.json identifies upstream commit b95bf9f6268772d5088f09b0034e488e64294835. Acceptance of this amendment includes one narrowly scoped upstream retrieval of the MIT copyright/licence text from jrmuizel/pdf-extract at that exact commit, plus read-only tree/path discovery if needed; no code or release download is authorized by that step. Record the URL, commit, full text and SHA256, and retain it with the vendored copy. Do not fetch it before acceptance. If the exact-commit notice cannot be obtained or reconciled with the package metadata, stop vendoring rather than inventing attribution. Preserve reference-derived notices as specified below. No package-version upgrade or transitive dependency update is part of this scope.

## Required geometry: first-class data, one decoder owner

Add an opt-in enriched glyph interface at the decoder callsite, supplied by its resolved font owner. Keep the legacy OutputDev API and old consumers behaviorally unchanged. A default compatibility path must preserve their existing call sequence, text, coordinates and errors; do not change PlainTextOutput to repair the new consumer indirectly.

The enriched record must contain decoded text and source occurrence order, resolved glyph bounds, advance, size and upright/writing-direction information, with enough raw provenance to audit each derivation. Geometry and font metrics are resolved once inside the decoder; the app consumes the resulting record and must not re-read font dictionaries or infer font identity from characters.

Reference-required inputs and branches to account for explicitly:

| Input | Source semantics to port and test |
|---|---|
| Simple-font descent | PDFMiner pdffont.py:905–941: descriptor/default resolution, default 0, positive descent sign normalization, vertical font scale. |
| Base-14 metrics | pdffont.py:1054–1055: FontMetricsDB lookup precedes descriptor/width fallback. Existing core_fonts.rs provides widths, not Ascent/Descent/FontBBox. Add the missing descriptor data from the pinned reference AFM data, retaining its full Adobe notice and marking modifications. Test precedence even when a PDF supplies conflicting descriptor/Widths values. |
| Type3 metrics | pdffont.py:1092–1095: FontBBox descent/ascent and FontMatrix horizontal/vertical scales. |
| Horizontal advance | Reference-resolved character width, font size and horizontal scaling, with character/word spacing and TJ adjustments reflected in subsequent glyph positions exactly once. Non-base-14 fonts without Widths must follow the pinned descriptor/Widths/MissingWidth resolution; see A2 below. Legacy advance stays unchanged. |
| Vertical writing | pdffont.py:1163–1180 and :1236–1238: writing mode, W2/DW2 displacement, default vx/vy, vertical advance. Exposing a fabricated horizontal rectangle is not support. |
| Transforms and orientation | Text matrix, page/content transforms, rise, scaling, rotation, normalized bounds and the reference upright test. Current trm already includes rise/scaling: do not apply either twice. |
| Page coordinates | Resolve inherited MediaBox and Rotate at the decoder page owner, respecting a leaf override. Apply reference rotation 0/90/180/270, normalized MediaBox origin/extent and coordinate convention; use MediaBox rather than substituting CropBox. Preserve actual page provenance and duplicates. |

Do not claim a descent field alone completes this interface. Trace the reference character fields used by line/word grouping, then demonstrate them on public fixtures. If any needed field or decoder behavior is still missing, stop and record it; do not silently use zeros, horizontal mode or fallback text. This is a qualification stop, not authorization to add new application rejection policies or page limits.

## Review corrections A1–A4

**A1: missing metric data and notices.** Add the base-14 Ascent, Descent and FontBBox descriptors to the enriched font owner; changing precedence cannot create missing data. Source them from pinned pdfminer/fontmetrics.py. Keep the entire Adobe AFM licence/copyright notice verbatim with the derived data and mark derivation/modifications prominently. Freeze the source file and extracted metric table hashes. Keep existing width data and legacy lookup behavior intact. Public tests cover all base-14 descriptor values and conflicting PDF-descriptor precedence, not only Helvetica.

**A2: correct the reviewer premise, preserve the real legacy path.** pdf-extract lib.rs:590–607 creates a local name set to Helvetica for non-base-14 fonts without widths, but the actual table comparison at :608 uses base_name, not that local name. Therefore the code does not implement the claimed Helvetica substitution for an unknown name. It leaves the width map empty and reads MissingWidth from the font dictionary at :657; PdfSimpleFont::get_width uses that value when the map has no entry. The pinned PDFMiner path at pdffont.py:1056–1058 / :907–909 instead resolves descriptor/Widths/MissingWidth defaults. This is a real owner/resolution difference, but its legacy behavior must not be described or tested as a working Helvetica fallback. No unrelated fix to that old path is authorized.

For the enriched record only, implement the reference resolution and resulting advances/positions. Freeze fixtures with a non-base-14 font lacking Widths: absent MissingWidth, descriptor MissingWidth only, font-dictionary MissingWidth only, and conflicting values in both locations. Assert raw advances, subsequent glyph positions, final exact reference text, and independently preserved legacy outputs. Merely changing the reported advance while continuing to position later enriched glyphs using legacy advances is insufficient. Keep the distinct resolution explicit at the decoder owner; the app must not compensate.

**A3: page geometry.** pdf-extract has no Rotate handling in lib.rs, and its current begin_page receives only MediaBox/ArtBox. The enriched page path must resolve inherited attributes and apply rotation before producing reference glyph coordinates. PDFMiner pdfpage.py:83–114 supplies inheritance and pdfinterp.py:1352–1362 supplies the glyph page transform; PDFPlumber page.py:211–230 normalizes page boxes. The rotate_point code near page.py:284 handles annotation rectangles and is not the glyph-transform owner. Add all four rotations, parent-only attributes, leaf overrides, nonzero box origins, and a differing CropBox fixture. Test that legacy callbacks remain unchanged and the enriched result matches the reference. Do not infer rotation from text orientation downstream.

**A4: local provenance and limited licence fetch.** Archive integrity is already proven locally; retain the receipt and recheck before copying. Only the missing exact-commit MIT notice requires the scoped upstream retrieval above. The Adobe AFM notice is already available in the pinned local reference, so preserve it locally without a separate download. Neither step authorizes upgrading dependencies or fetching an unpinned release.

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

A compatible geometry extension is a proposed implementation, not proven sufficient. Maintaining a vendored patch is a real cost. The existing library also has differences outside the exposed callback; Phase A must identify any such difference needed for the enumerated public cases rather than silently broadening this patch. Archive provenance is now verified; the missing exact-commit MIT notice and retained Adobe AFM notice must be established before writing the dependency delta. The r2 STOP remains in force until this amendment is independently reviewed and explicitly accepted.

## Contract-time implementation summary / cold diff / gap audit (historical)

NOT DONE. This amendment and source inventory are documentation only. No dependency, parser, runtime or test behavior changed. A1–A4 are contract corrections only; archive integrity was checked without extracting or changing source files. The preserved public evidence establishes why the earlier interface is insufficient, not that this extension works. The next implementation action after acceptance is fixture/reference freezing and verified vendor staging, followed by fail-first geometry tests.

## Operator acceptance

Accepted verbatim: "I accept revision 4, including the pinned licence-text fetch. Let’s implement it and run the offline parity tests.". Reviewed r4 SHA256 e0fe7e52069dd32de11dbf4755f055977866fb179aff336f7c98864583322ab7. Oversight CONTRACT PASS 540120ba.

## Accepted-scope execution: licence prerequisite STOP

The exact upstream commit tree is untruncated and has no licence/notice path. The repository licence endpoint with that exact ref returns HTTP 404. The verified cached package declares MIT but contains no full MIT notice. Per A4, vendoring stopped; no copyright attribution was invented. Durable evidence alias `c9-native-layout-parity-20261027/licence-stop-r4.json`, SHA256 7b825d22a2daf839658023820c6fc743b699a08374da6365c6777cc95040f9bc. The missing input is an authoritative copyright/licence notice reconciled to the pinned upstream commit. A changed provenance procedure requires a separately reviewed scope decision.

Independent offline preparation: `r4-public-reference/manifest.json` SHA256 5e008b5d79ccb8f3190adfa652f8bda9aec9edf913f3bffef93db25d24024b2c freezes 16 public A2/A3 cases after checking 52 reference source hashes. It records exact text and raw glyphs for MissingWidth defaults and page transforms. This is a partial reference fixture batch, not native parity or the completed Phase A. Further public cases, legacy compatibility, decoder implementation and all application/private parity gates remain NOT DONE. No production code, dependency, parser identity or model setting changed; no model/OCR calls ran.

## A4 operator-authorized resolution

Operator approved the metadata-derived notice: "Approve the license notice." Verified against the human message at 2026-10-28T18:57:04.041Z; relayed in oversight77c90c7a. The vendored package will carry canonical MIT text with `Copyright (c) Jeff Muizelaar and the pdf-extract contributors`, with no invented year. PROVENANCE.md records its derivation from the package MIT declaration/authors and the missing upstream notice, plus the exact VCS/archive/source pins. Preserve the complete pinned Adobe AFM notice. This replaces only A4's unavailable-upstream-notice stop. Other r4 gates remain. No upstream issue is authorized.

## Phase A candidate result

NOT DONE. The local dependency candidate adds resolved font metrics, raw glyph bounds,
vertical displacement, text advances and inherited page rotation behind an opt-in path.
Default native extraction still uses legacy callbacks; the application has not adopted
reading-order assembly or a new parser identity. The 90-PDF serialized legacy baseline
is byte-identical. The base-font test and original descent-twin test pass; zeroing descent
propagation kills the latter, and restoring the exact source makes it pass again.

The full 87-case public geometry comparison fails: 30 numeric fields differ across
Type3 scaling and decimal-coordinate cases. Reference text assembly on those candidate
glyphs matches the original matrix, but a separate public boundary pair proves a real
text-changing collision: 717.00000 and 716.99999 become the same f32 coordinate while
reference text is `A B` versus `A\nB`. The explicit fail-first regression confirms
identical complete enriched glyph records. Origin: lopdf parser/mod.rs:99–110 and
object.rs:42, before this interface. No tolerance was loosened. R4 requires stopping
before expanding to that second library. The r5 numeric-precision proposal is unaccepted.

Two defects in this unpublished candidate were fixed locally: reusing the legacy glyph-name
width keys mishandled ZapfDingbats (corrected to the pinned reference Unicode keys), and
re-parsing the legacy JSON snapshot rounded a float in the comparator (corrected to exact
serialized-byte comparison). These were implementation/test rework, not model findings.
No downstream special-case filters were added. Legacy lookup data remains unchanged.

Focused regression evidence, format and Clippy completed; Clippy still reports inherited
vendor warnings. Phase A is not a PASS. Remaining public coverage, application assembly,
identity/persistence, original-PDF parity, dependency/writer/C9/SAME60 and platform CI are
not complete. No push, installation, model/OCR run or held-stack merge occurred.
