# Proposed r6: preserve Form invocation context at the decoder

Status: ACCEPTED; implemented locally with offline evidence below. Based on local r5
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

## Acceptance record

Juan accepted the frozen proposal above (SHA256 46534287658e7f46ddbb5b247f753cea5b5c3803f001a9467e18ccbc9a07cba5)
verbatim: **“I accept r6, including errors for cyclic or malformed Forms. Let’s implement it and run the offline tests, keeping existing defaults unchanged.”**
Oversight CONTRACT PASS: `e7a63d12`. Binding malformed-invocation coverage
includes missing/wrong-type Do operands, missing XObject entries and dangling
references: return an error, never panic or return a successful partial page.
The durable receipt is `operator-acceptance-r6.json`. No implementation is
contained in this acceptance record.

## r6 implementation and offline proof

The opt-in geometry path now enters Forms through one invocation owner in
`vendor/pdf-extract/src/lib.rs`: it resolves the target, distinguishes subtype,
validates the Matrix/resources, composes the supplied caller CTM, and creates
fresh child state. The shared operator engine suspends parent frames explicitly.
Canonical stream IDs are tracked only on the active path, rejecting cycles while
allowing repeated sequential use. No fixed nesting/page cap was added.
The geometry Tf branch now reports missing resource/font entries as errors
instead of panicking or borrowing a parent's cached font. The legacy branch
retains its prior behavior. No downstream compensation was introduced or found
that needed removal; the context-losing geometry recursion itself was replaced.

Reproduce → isolate → explain → fix → prove → prevent regression:

1. Preserve the earlier transformed-Form failure (28 fields), then freeze the
   new reference controls before source edits. The direct-empty Resources
   regression panicked on missing Font. Malformed-invocation controls either
   panicked at operand/reference lookup or incorrectly returned success with
   invalid Matrix/resources. These outputs are retained, not rewritten.
2. The first loss is the inherited Do entry, not glyph assembly: it previously
   discarded caller CTM, ignored Matrix/Subtype and used unsafe lookup helpers.
   Direct-empty resources also chose the empty dictionary instead of the
   caller. Indirect-empty was already correct and remains a positive control.
3. Repair this owner and move invocation state into explicit frames while
   retaining one operator implementation. The collector already propagates
   extraction failure before returning the page; error tests exercise that
   boundary, including failure after valid glyphs.
4. On the final source: all 87 original geometry cases, 19 new reference Form
   cases and 4 continuation page cases match the frozen geometry and text.
   The 21 native-policy malformed/cycle fixtures return errors without panic or
   successful partial pages. Additional falsy/default checks reject invalid
   values and accept an all-zero Matrix. Repeated traversal of a 4096-level
   acyclic Form chain succeeds without host-stack recursion.
5. The adjacent parser suite passes: 47 passed, 0 failed, 5 held ignored. The
   original continuation test is explicitly run despite its held marker and
   passes. The 90-PDF legacy compatibility snapshot remains byte-identical.
   Format and Clippy pass, with inherited vendor warnings. Platform CI and
   independent implementation review have not run for this local candidate.
6. Seven deliberate mutants are killed: omitted caller CTM, omitted Form
   Matrix, leaked child text state, parent-resource substitution, Image bytes
   parsed as operators, cycle returning partial success, and disabled cycle
   rejection. The last is compiled first and killed by a bounded execution
   timeout. Exact source is restored afterwards. These runs predate only the
   separately proven gs dispatch correction described below; their source hash
   and logs are retained. No claim of independent review is made.

### Own defects found and corrected before publication

* The copied r5 scoring script used `utils.extract_text`, not the Page textmap
  path of the frozen `page.extract_text()` reference. A mixed-orientation Form
  exposed that difference even when the scorer received the reference's own
  glyphs. `r6-scorer-reproduction.json` preserves both wrong-helper results.
  The r6 scorer now calls the exact pinned Page assembly path and first proves
  it reproduces every frozen page from reference glyphs, then scores candidate
  glyphs. Frozen expected text and decoder output were not changed. Earlier
  r5 evidence/scripts remain immutable; rechecking the original matrix through
  the corrected path still yields zero differences.
* The uncommitted mechanical state refactor accidentally renamed the literal
  `gs` operator to `frame.gs`. Cold diff review found it. A new fail-first
  regression showed its pre-existing validation was silently skipped. Restoring
  `gs` makes the valid/invalid controls pass on both legacy and geometry paths;
  a complete comparison of operator names against the base now matches. This
  is our local rework, not an upstream bug. General ExtGState validation was
  not redesigned. The initial cycle-disabled mutant also failed to compile
  because removing insertion removed type inference; the corrected mutant
  keeps insertion and disables only rejection, and is killed at execution.

### Cold diff audit

| Owner | Change | Evidence |
|---|---|---|
| pdf-extract lib.rs invocation frame / Do / geometry Tf | Preserve source-owned transform/resources/state; typed errors; bounded active-path execution; unchanged legacy branch | frozen geometry, native errors, deep/repeated traversal, mutants, legacy snapshot |
| parser_layout_tests.rs and new tests/forms fixtures | Public fail-first inputs plus sibling/error/default controls; gs dispatch regression | adjacent parser suite and independent continuation invocation |
| PROVENANCE.md and this amendment | Record maintenance boundary, observed proof and remaining holds | documentation-only; no new runtime behavior |

### Gap audit

DONE locally: the accepted r6 source repair and offline regression/parity gates.
NOT DONE overall: independent r5/r6 implementation review, platform CI,
application reading-order adoption/identity and saved-artifact immutability,
original private-PDF parity, and downstream writer/dependency/C9/SAME60 gates.
No model/OCR calls, production adoption, installation or held-stack merge.

Evidence aliases under `c9-native-layout-parity-20261027/`:
`r6-form-reference/manifest.json`, `r6-direct-empty-before.log`,
`r6-malformed-before.log`, `r6-continuation-after.log`, `r6-parser-final.log`,
`r6-results/parity-score.json`, `r6-mutation-results.json`,
`r6-gs-dispatch-before.log`, `r6-operator-inventory.json`, and `r6-result.json`.
The final receipt binds the exact source snapshot, these logs and their hashes.

## Review repair: raw Form dictionary semantics

Oversight B1/B2 on the published candidate traced to our introducing commit
6c83d00, at `geometry_form_frame`. This is our own porting error: the original
repair did not consistently preserve the reference's raw dictionary semantics.
The reference's parser removes direct-null dictionary entries before dispatch;
its interpreter compares the raw Subtype with the Form literal and unpacks a
present BBox into four values. The old implementation instead resolved Subtype,
tested BBox presence without validation and treated null Matrix as malformed.
Only Resources correctly treated direct null as absent.

### Required repair and contract interpretation

One `invocation_entry` helper now owns missing/direct-null absence for Subtype,
BBox, Matrix and Resources. A raw direct Form name selects the Form path; other
subtypes are skipped before content decoding. A present BBox must resolve to four
finite numbers; Matrix uses the same numeric-array validator with six values.
BBox is validated, not used to add clipping. Missing/direct-null Matrix selects
identity. Resources retain their raw direct-empty versus indirect-empty behavior.
This removes the separate Resources null special case, the dereferencing Subtype
reader and the presence-only BBox test, rather than adding a downstream filter.
Legacy dispatch, defaults, dependency versions and application selection stay
unchanged. No new public API, model call, OCR call or adoption is included.

Item 5's absence rule applies after the reference parser's direct-null removal.
The original native-error classification of `matrix-null` was our mistake, not
a declared reference departure. The old manifest remains in durable evidence as
`r6-dispatch-before-manifest.json`; the PDF bytes remain identical. Its new
reference JSON was frozen from the pinned `page.extract_text()` before source
edits. Historical result packets are not overwritten.

### Regression scope

Public fixtures cover every reviewed row, BBox lengths zero/three/four/five,
non-numeric BBox data, indirect BBox, valid zero BBox, missing/null/numeric/indirect
Subtype, Image and unsupported subtype with BBox and executable text content,
and null Matrix. A separate 309-digit Real overflow fixture requires the
`nonfinite Form Matrix` error so an unrelated failure cannot count.
The original overflow fixture does contain a decimal point (contrary to that
detail in the review); its earlier broad error-only assertion still did not
prove which guard rejected it. The new owner-specific assertion removes that gap.
In the nonfinite-check mutant, the later glyph validator still rejects the page;
the regression fails because the required invocation-owner error is missing.

The fail-first `decoder_form_dispatch_key_semantics` lists all wrong outputs
before the repair, then passes after. Existing fixtures are retained; the only
old expectation correction is the explicitly recorded null-Matrix classification.
Tf operand-count and indirect Matrix-element coverage remain non-blocking
follow-ups in issue #156. The precise implementation stays intact.

All prior merge, production and qualification holds remain. The relayed operator
priority is to finish this r6 review repair round first, then prepare the 32k
qualification work order for separate exact-revision acceptance. It authorizes
no live generation, adoption or held-stack merge.

### Verified repair evidence

The fail-first probe exits 101 before the repair and passes after it. Final
parser result: 48 passed, 0 failed, 5 held ignored. The continuation reference
test was also explicitly run. Frozen geometry and exact Page text have zero
differences across 87 original, 29 Form and 4 continuation page cases. Legacy
compatibility passes in the parser suite. All 13 deliberate mutants compile
and are killed at execution (disabled cycle rejection by bounded timeout).
The original source is restored and hash-checked after mutation. Exact CI
formatting and Clippy commands pass; inherited vendor warnings remain.

Durable evidence alias `c9-native-layout-parity-20261027/r6-dispatch-result.json`,
SHA256 `eaa98becf837b5093f0789d237e94cfc06b963201dedee73b261ac5377c17c21`.
The receipt binds the source/tests, pinned-reference probes, before/after logs,
fixture integrity, exact-text scorer and mutation results. Final diff touches
only the invocation owner, its tests/fixtures and canonical documentation.
Fresh exact-head CI and independent review remain pending; local proof does
not clear the existing merge, production or qualification holds.
