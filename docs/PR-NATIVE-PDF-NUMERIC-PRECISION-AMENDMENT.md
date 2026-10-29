# Accepted r5: retain decimal precision at the PDF object reader

Status: ACCEPTED and implemented locally. The numeric parity repair passes;
overall Phase A remains stopped on a separately reproduced Form-transform gap.
The frozen accepted proposal and its hash are preserved in the acceptance record.

## Inventory and root cause

Reuse accepted r2 and r4's pinned PDFPlumber 0.11.10 / PDFMiner.six 20260107
reference, source hashes, original inputs, defaults, source completeness,
model/runtime inventory and all fidelity/qualification holds. No model calls.
The public Phase A freeze has 87 geometry cases, plus the earlier drawing-order
and descent cases. The pristine native baseline captures 90 PDFs, including
the pre-existing ZapfDingbats panic. The candidate preserves that serialized
baseline exactly (SHA256
12809543910ee88cfeff497ad8de7c5da7d4496aa223d35647904eb8660802b6).

The enriched font owner fixes the original missing descent/metric information.
It is not yet an adopted application parser. Its full raw-geometry check still
has 30 differing fields across Type3 scaling and decimal tolerance cases.
Running the unchanged reference text assembly on that initial matrix's candidate
glyphs produces matching text; this does not prove the numeric losses harmless.

A separate minimal public pair proves a text-changing collision:

| PDF | Second glyph baseline | Pinned reference text | Native baseline |
|---|---|---|---|
| at-boundary | 717.00000 | `A B` | 717.0 |
| over-boundary | 716.99999 | `A\nB` | 717.0 |

The entire enriched glyph records are identical, not just the displayed numbers.
The explicit `decoder_precision_preserves_line_boundary` regression fails at
`assert_ne!` for this collision. No deterministic assembly of identical records
can produce both reference results. A looser comparison tolerance cannot repair it.

First loss: **lopdf 0.42.0 src/parser/mod.rs:99–110**, where decimal token bytes
are parsed directly with `f32::from_str`. **src/object.rs:42** stores `Real(f32)`;
widening later cannot recover the digits. This predates our candidate. The new
geometry interface exposed it; it did not introduce it. The accepted r4 patch
surface is pdf-extract, not lopdf's numeric representation.

The installed lopdf archive is pinned by Cargo.lock SHA256
25aab26d99567469098e64a02f42679f8965c6401263eefa31d8f2dcc37a221c.
Its VCS metadata says b68476c2a067f3b5158de60cd8ebce69f72068c8 **dirty: true**;
therefore the exact archive/source hashes, not an assumed pristine VCS tree,
are authoritative. The package contains its MIT notice (Junfeng Liu), unlike
the previous dependency. `numeric-owner-inventory.json` records the inspected
owner hashes. No extra download or upstream issue is proposed.

## Required change surface and every new departure

Proposed direction: a second pinned local dependency patch with an explicitly
selected 64-bit numeric path, preserving legacy defaults. Do not make all old
consumers silently change precision.

1. Stage verified lopdf 0.42.0 under `src-tauri/vendor/lopdf`, retaining its
   licence, pristine manifest and separate delta. Wire both application and
   pdf-extract to the same pinned copy. No version or transitive upgrades.
2. At the single decimal-token owner, add an explicit precision parameter.
   Its precise branch parses the original token directly to f64 and stores a
   distinct `Real64` value. The existing `Real(f32)`, constructors and default
   loaders keep their old semantics. No process-global flag or second lexer.
3. Propagate that request-owned mode through the document reader, nested
   arrays/dictionaries, indirect/compressed objects, and content operators.
   Known entry points: reader.rs load options and indirect-object dispatch;
   object_stream.rs:92 currently calls direct_object; parser_aux.rs:20–26 calls
   content/content_strict. Cover every reader of the decimal token owner before
   coding, rather than changing only `Td` or only uncompressed page streams.
4. Extend numeric access/serialization helpers needed by the new representation
   without silently narrowing precise values. The type/display/writer paths
   must handle it explicitly. Keep old serialization bytes unchanged.
5. The enriched pdf-extract entry point must consume a document created through
   the precise loader. Use a wrapper with private construction so a legacy,
   already-rounded Document cannot be passed as if it retained precision.
   Decode its content streams with the same mode. Resolved font metrics/page
   attributes use that representation once; no downstream string recovery,
   PDF re-scan, coordinate rounding or document-specific rules.
6. Application adoption stays behind r4 Phase A and r2's identity, immutable
   saved-artifact, full private page parity, dependency/writer and C9/SAME60 gates.
   Only its new native parse loader will opt in when those gates pass. Existing
   OCR parsing and saved/continued artifacts remain on their existing path.

This expands maintenance from one vendored library to two and adds an explicit
numeric representation. It is a real cost, required by the hard reference
parity boundary; it is not a claim that the model needs more capacity.

## Explicit non-scope

No numeric tolerance changes, normalization to hide changed clustering, new
PDF/page admission limits, global float-mode change, Python sidecar, whole
decoder replacement, unrelated parser fixes, model/OCR calls, prompt/settings
changes, database migrations, installation or held-stack merge. Existing
public evidence is immutable. No benchmark labels or new private tuning.

## Verification plan

* Reuse the exact public pair. Before repair it fails with identical native
  glyphs; afterwards both geometry and reference text must differ correctly.
  Removing precision propagation must kill that test.
* Cover original decimal bytes in page operators, font dictionaries/Type3
  matrices, inherited page boxes, nested arrays, compressed object streams,
  positive/negative/zero/exponent-forbidden/malformed tokens and parser error
  paths. Probe both sides and the exact line-grouping boundary. Keep the existing
  fixed geometry comparison and exact-text requirements.
* Demonstrate default loaders/callbacks/text/errors and serialized outputs are
  unchanged, including mixed interleaved precise and legacy loads. No global
  state may select precision. Test that the enriched API cannot accept an
  already-rounded legacy document.
* Rerun the unchanged public geometry matrix, width/default/rotation controls,
  descent twins and legacy compatibility. Complete remaining Phase A coverage
  (including transformed content and actual page continuation) before adoption.
* Only then run the original private PDFs through production parsing and all
  accepted downstream offline gates. No substitute flat text or model run.
* Focused parser/reader tests, fmt/clippy, and Windows/Linux/macOS evidence remain
  required before merge. Any newly exposed discrepancy stays a failure.

## Assumptions and blockers

The numeric source owner and minimal failure are proven. The opt-in representation
and all reader propagation are not implemented or qualified. Independently review
this proposal and obtain explicit operator acceptance before modifying lopdf.
If the proposed surface misses another decoder behavior, record it and stop
before silently expanding the patch or changing compatibility promises.

## Implementation summary / cold diff / gap audit

NOT DONE. This is a proposed amendment only. No lopdf source, numeric parsing,
default precision, or application parser selection has been changed. The current
r4 candidate is unpublished and unqualified; the fail-first evidence and initial
partial successes do not clear Phase A or any production/merge hold.

## Acceptance record

The frozen proposal above (SHA256 ba9ca99c9e77e8a9005dbaa476d1d7c31ff450c345d55da0e4538b2aa8aa8175) received oversight CONTRACT PASS in mail `4e456465`. Juan then accepted verbatim: **"accept R3.2 and r5"** (human message `c5fdff5b-12f3-47e0-9abc-049a077052b3`, 2026-10-29T01:31:12.690Z; relay `ce0edfe2`). Only r5 applies to this lane. Its implementation is now authorized; the original proposal text is preserved above.

Binding reader inventory before implementation includes both decimal-token grammars; direct-object, indirect-object, compressed-object, and content-stream entry points; and every pdf-extract numeric accessor. The additional `common_data_structures/mod.rs:75,99` direct-object callers are legacy decoder tests and remain covered. All remaining 30 raw-geometry differences stay visible until individually cleared or explained. No tolerance change, legacy-default change, model call, application adoption, or merge hold is authorized by this acceptance.

This acceptance record contains no implementation. The durable operator receipt is `operator-acceptance-r5.json`.

## r5 implementation and offline proof

The opt-in loader now preserves original decimal tokens as Real64 through nested
objects, stream dictionaries, inherited attributes, compressed objects, deferred
decryption, and content operators. Default loads and From<f64> still use Real(f32).
A private-construction PreciseDocument wrapper owns the geometry entry point;
the shared content dispatch also rejects a geometry callback on a legacy document,
using the reader's stored precision marker. No text re-scan or tolerance change.

The original 87-case geometry matrix has zero differences; all 30 previously
reported numeric fields are cleared. The pinned reference assembly on candidate
glyphs has zero text differences. The exact boundary twins yield `A B` and `A\nB`.
The 90-PDF legacy snapshot remains byte-identical, including its previously
recorded ZapfDingbats error. The adjacent parser suite reports 40 passed, 0 failed,
5 explicitly held probes ignored. Format and Clippy pass (inherited vendor warnings).
The compile-time wrapper negative/positive probes and its documented example pass.

Reproduction and regression sequence:

1. Preserved pre-r5 public collision and raw numeric failures identify the f32
   token owner (lopdf parser/mod.rs real). The source, not the model, lost digits.
2. Add the selected precision at that owner and carry it with the document and
   content decode. Both application and pdf-extract use one pinned local lopdf.
3. Run unchanged geometry/text/legacy proof and nested/compressed/error controls.
4. Remove content precision propagation: the boundary regression fails, observing
   717.0 instead of 716.99999. Restore the exact source: it passes.
5. Two defects in this unpublished implementation were reproduced and corrected:
   Real64 serialization lost negative zero (decimal-token serialization now
   preserves it); public geometry callbacks could bypass the wrapper (shared
   geometry dispatch now consumes the reader's precision marker). Both regressions
   failed before their fixes and pass in the final parser suite. No published
   introducing commit exists; these were local implementation rework.
6. A test initially assumed adjacent decimal operands are rejected. Existing
   grammar accepts them and treats `e` as an operator, not an exponent. Tests now
   assert the unchanged tokenization; the parser grammar was not tightened.

## Cold diff audit

| File / owner | Actual change and contract trace | Proof |
|---|---|---|
| Cargo.toml / Cargo.lock | Single local lopdf 0.42.0 patch; no transitive changes | locked offline builds |
| vendor/lopdf source / PROVENANCE.md / LICENSE | Archive-pinned source, original MIT notice; nine source files changed | pristine manifest and separate delta |
| load_options.rs / lib.rs | Explicit default-Legacy enum and opt-in option/export | interleaved load controls |
| parser/mod.rs | One real-token lexer, selected representation propagated through object/content grammars | public boundary, nested object and strict/error controls |
| reader.rs / document.rs / object_stream.rs | Store selected precision and preserve it across all compressed/deferred readers | nested/compressed and deferred-decryption tests |
| object.rs / writer.rs | Explicit Real64 access/type/debug/serialization; f32 accessors reject it | round-trip, signed-zero and nonfinite serialization tests |
| parser_aux.rs | Explicit content APIs and document-owned mode | content and page decode tests |
| vendor/pdf-extract lib.rs / geometry.rs | Non-narrowing accessors, precise wrapper and dispatch marker check | geometry parity, callback negative, compile-time boundary |
| parser_layout_tests.rs / public continuation fixtures | Preserve old freeze; add precision and continuation proofs | parser suite plus explicit continuation failure |

Legacy CMap metadata dictionary helpers remain on their original representation;
they do not decode page/font geometry. Existing legacy width tables, OCR selection,
application parser identity, prompts/model settings, and saved artifacts are unchanged.
No older downstream workaround was added or retained to mask numeric loss.

## New Phase A STOP: Form transforms

The frozen supplementary public probe reports 28 differing raw fields, all in
`transformed-form`. Ordinary transformed content and actual page continuation
match. Origin: pdf-extract Processor::process_stream's `Do` branch re-enters the
processor without passing the caller's CTM or reading the Form's Matrix. Its new
GraphicsState starts from initial_ctm. This behavior existed in the pristine
upstream source; our f454b28 enriched candidate inherited it. The public fixture
includes independent caller translation and Form scaling/translation, so neither
can pass by coincidence. No Form repair or downstream coordinate compensation
is included in r5. This is the accepted r4/r5 new-decoder-behavior stop.

**Gap audit: NOT DONE overall.** Numeric repair and its offline proofs pass.
Full Phase A, application adoption, original private-PDF parity, identity/storage,
dependency/writer/C9/SAME60, platform qualification, and independent implementation
review remain incomplete. No model/OCR calls, installation, push or merge.

Durable evidence alias: `c9-native-layout-parity-20261027/r5-result.json`.
