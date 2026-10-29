# Proposed r6: preserve Form invocation context at the decoder

Status: CONTRACT ONLY; not accepted and not implemented. Based on local r5
`8ffa475ca8f3324681e04951c28659993c936417`. Oversight mail `bc000a45`
independently confirms the Form STOP, not an implementation PASS for r5.
The r5 implementation still needs its own draft PR and exact-head review.

## Proven setup and inventory

Reuse r2, r4 and r5 without replacing their fixtures or changing their defaults.
The durable evidence root is
`~/Desktop/codex-evidence/document-summarizer/c9-native-layout-parity-20261027/`.
`reference-runtime.json` pins Python 3.13.11, PDFPlumber 0.11.10 and
PDFMiner.six 20260107, including the reference source hashes. The reference is
default `page.extract_text()`: x/y tolerance 3, use_text_flow false and its
direction-aware ordering. The original historical extractor executable remains
unknown; the pinned reference reproduces the frozen prepared text and all ten
pages. Original PDF and prepared-input bindings remain those of accepted r2.

The original summary runner consumes prepared text. Its model/server pins,
settings, framing, prompts, inputs and results are inherited explicitly from
the r2 inventory, not reinterpreted here. This amendment exercises no model or
OCR stage and makes no new inference-qualification claim. Production's selected
parser, runtime settings and saved artifacts stay unchanged.

Locally recorded r5 evidence (`r5-result.json`, SHA256
`fceb868b68982d0e4581f0e0b2298f1245ab34b773b7d4fbdae9c609cbc20924`):
87 geometry cases have zero differences, the original 30 numeric differences
are cleared, reference assembly text matches, and the 90-PDF legacy snapshot
is byte-identical. Parser tests: 40 passed, 0 failed, 5 held ignored. These are
recorded local results, not independent approval. The additional Form probe has
28 differing fields; ordinary transformed content and actual page continuation
match. Full Phase A has therefore NOT passed.

## Root cause

At the base head, `src-tauri/vendor/pdf-extract/src/lib.rs:1596-1625`
creates a new graphics state on each `Processor::process_stream` entry with
`ctm: self.initial_ctm`. Its `Do` handler at lines 1905-1913 recursively calls
that entry with only content/resources/page arguments: it loses the caller's
current CTM and never reads the Form's `/Matrix`. It also does not discriminate
`/Subtype`, so Image bytes can be parsed as text operators. This is inherited
upstream behavior exposed by our opt-in geometry candidate, not introduced by
r5's numeric precision change. The exact original upstream introducing commit
is not established; do not invent one.

The pinned reference's `pdfinterp.py:1317-1350` dispatches by subtype and enters
a Form with `mult_matrix(matrix, self.ctm)`, defaulting a missing Matrix to
identity. `init_state` starts fresh child text/graphics state; it does not inherit
the caller's font/text state. `subinterp` and `execute` track ancestor stream IDs.
The defect belongs to this invocation boundary, before glyph geometry exists.
No later coordinate correction, text re-scan or assembly exception can repair it.

The public `tests/continuation/transformed-form.pdf` (under the vendored
pdf-extract directory) has SHA256
`6f64f4a0130a8c5bacbae937916a07b2f7165895b16971cea6229a2ae68c2f42`.
It combines a parent translation and a nonidentity Form Matrix. Its frozen
reference JSON and the fail-first `decoder_geometry_continuation_reference`
test remain immutable; use the recorded failure rather than replacing it.

## Required change surface and departures

1. Repair the opt-in geometry invocation owner in
   `src-tauri/vendor/pdf-extract/src/lib.rs`. One invocation context owns the
   supplied CTM, local resources, child state and active stream identities.
   Compose caller CTM and Form Matrix in the reference order, using retained
   f64 values. Fresh child state must not overwrite parent state. Nested Forms,
   q/Q inside and outside them, and following page text must obey this owner.
   Keep the existing operator implementation shared; do not create a second
   text interpreter or retain downstream transform compensations. None have
   been identified to remove. Replace the geometry path's context-losing
   recursion rather than stacking another geometry correction on it.
2. Dispatch before decoding XObject bytes. Match the reference's Form-with-BBox,
   Image and unsupported-subtype decisions. Images emit no text glyphs; image
   pixels are not content operators. Unsupported subtypes and Forms lacking
   BBox remain non-text as in the reference. Do not add BBox clipping: the
   reference text collector is the specification, not a new renderer.
3. Resolve resources at the invocation owner. Preserve the pinned reference's
   distinction: absent/null/direct-empty Resources fall back to the caller;
   an indirect reference to an empty dictionary selects that empty dictionary.
   Nonempty child Resources replace rather than merge with parent Resources.
   Resolve references before use, without losing that original presence/type
   distinction. Same font names in parent/child must use their actual owners;
   the current geometry Tf path already resolves fonts per resource dictionary.
4. Bound recursion structurally, without inventing a fixed Form-depth or page
   cap: use explicit invocation frames, not recursive host-stack calls, and
   reject a repeated canonical resolved stream identity on the active path.
   Remove identities when frames exit, allowing repeated nonrecursive use.
   Active depth is bounded by the document's distinct reachable stream objects.
   Self/mutual cycles and unresolvable invocation identities return an error;
   never silently omit the invocation and report a successful partial page.
   This is an explicit departure from PDFMiner's cycle skip. It is opt-in only.
   This bounds call-stack/cycle behavior, not total work for a large acyclic
   graph; no general decoding work budget is introduced by this amendment.
5. A present malformed Matrix (not six finite numbers), invalid Form resource
   type, or failed required reference resolution returns a geometry error,
   without identity/parent fallback. Only an absent Matrix means identity.
   This explicit error policy differs from permissive reference coercions on
   malformed input. Failed geometry extraction must not return/persist a
   successful partial page; callbacks already emitted before an error do not
   constitute a successful extraction and must be discarded by the collector.
6. Preserve the legacy branch byte-for-byte in behavior, including existing
   callback/text/error snapshots. r6 must not silently change default callers.
   No new public API is required; any unavoidable public API change is a STOP
   for amendment, not an implementation detail.

Tests live in `src-tauri/src/pipeline/parser_layout_tests.rs`, reached through
the existing parser test module. Add a separate public Form fixture collection
under `src-tauri/vendor/pdf-extract/tests/`; leave existing frozen bytes intact.
Provenance/doc updates may cover pdf-extract's patch and this amendment. No
lopdf numeric-reader change or Cargo/dependency upgrade is in scope.

## Verification plan

Freeze new reference fixtures before changing the decoder, after checking the
pinned reference hashes. Retain raw geometry, text, errors and fixture generator
in durable evidence. Keep the existing absolute numeric tolerance 1e-9 and exact
text requirements. Malformed/cycle error expectations are explicitly native
contract expectations, not falsely labelled reference-parity results.

* Reuse the failing transformed-Form case. Add absent/identity Matrix controls,
  caller-only transform, Form-only transform, and nested noncommuting transforms.
* Prove q/Q isolation and parent text after Do, fresh child text state, same-name
  font shadowing, missing/direct-empty/indirect-empty Resources, and repeated
  invocations whose duplicate glyphs must remain present.
* Image bytes containing syntactically valid BT/Tj text must emit no glyphs;
  the same operators in a valid Form must emit text. Cover mixed Image/Form
  siblings, unsupported subtype and missing-BBox controls.
* Cover absent Matrix versus malformed length/type/nonfinite values, empty
  content, unresolved references, direct and mutual cycles, and a deep valid
  acyclic chain. Invalid inputs must error without a successful partial page;
  valid siblings must not be rejected. Test multiple invocations/documents for
  leaked state. A cycle after valid glyphs must still fail the collected result.
* Record fail-first results before implementation. After repair, reproduce the
  same cases, then kill independent mutants that omit caller CTM, omit Matrix,
  leak child state/resources, decode Image bytes or defeat active-path checking.
  Cycle mutants run in a bounded test process, never an unbounded recursion.
* Rerun the unchanged geometry matrix, continuation cases, precision twins and
  legacy snapshot. Run adjacent parser tests, formatting and Clippy. Preserve
  every mismatch; no expected-result edits to make the candidate pass.
* Linux/Windows/macOS checks and independent exact-head implementation review
  remain merge gates. r5's outstanding implementation review remains separate.

## Explicit non-scope

No model/OCR calls, new private-document tuning, application parser adoption,
saved-artifact reinterpretation, PDF admission-limit change, coordinate rounding,
text filtering/deduplication, Python runtime dependency, decoder replacement,
font/precision redesign, unrelated decompression repair, install or held-stack
merge. Existing default precision/callbacks and all fidelity holds remain.
Full private-PDF parity and downstream writer/dependency/C9/SAME60 proof still
follow the original accepted gates; this contract does not claim them complete.

## Assumptions and blockers

The original transform failure is reproduced and source-isolated. New fixture
coverage and iterative-frame feasibility are not yet proven. If another needed
decoder behavior appears, record it and stop before broadening this repair.
Oversight must review this exact proposal and Juan must accept it before code.
In particular, acceptance must cover the declared cycle/malformed-input errors;
peer review cannot authorize those departures. Publishing r5 for independent
review does not constitute r6 acceptance or clear any application hold.

## Implementation summary / cold diff audit / gap audit

NOT DONE implementation. This amendment changes documentation only. No decoder,
test, frozen input, application setting or dependency changed. No code tests are
rerun for this documentation-only proposal; the results above remain bound to
their recorded r5 head. The next authorized step is contract review, followed by
operator acceptance before fail-first fixtures and implementation.
