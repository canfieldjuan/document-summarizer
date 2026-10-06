# Bounded segment-text enums for General verification

Status: design accepted by Codex in response to the operator's clarified
recommendation. This is the contract before implementation; no production or
test code has changed. The concrete splitting policy and gates below are the
standard for the next implementation. PR116 remains fidelity-held.

Base: `49e22a7e3422c42ef4202d5048b15e1693ff119f`, on
`codex/verification-revision-contract`. Do not base this work on, or cherry-pick
the production code from, the failed position candidate. Its four local commits
end at `5446ed15df91921d064314ffab3e3bd47782f017` and remain a failed experiment.

## Root cause

At the base, `summary/comparisons.rs:79-112` materializes every token-bounded
excerpt up to 240 Unicode scalar characters. `Prepared::new` at lines 148-175
places these overlapping strings in source/claim schema enums. A short input can
therefore exceed the schema limit even though its authoritative text fits the
input budget. The downstream cap is correct; the catalog representation expands.

The failed position experiment solved size but changed the model's task to
selecting token endpoints. Its frozen gateway controls passed only 2 of 4 verdict
checks and exactly matched 2 of 32 recorded passages. The parser reconstructed
the chosen ranges; it did not recover the missing semantic comparison. These
results do not prove why the model chose those passages, or that another protocol
will restore fidelity. They rule out qualifying that candidate.

The new hypothesis preserves constrained text selection and changes only the
available passages: whole bounded sentences/sub-clauses instead of every excerpt.
The claimed historical C9 success is motivation, not proof for this new catalog.

## Required change surface

### One complete, bounded catalog at the comparison owner

- Replace `span_catalog` with a deterministic, source-independent segmentation
  policy in `summary/comparisons.rs`, or a directly owned helper if needed.
  It must not consult control labels, recorded C9 selections, generated claims
  to choose source cuts, or model output to repair a cut.
- Build separate source and claim catalogs. Retain the original full claim,
  every complete governing source context, and existing evidence/context IDs.
  The full clause remains the authority for interpreting a selected piece.
- Preserve the exact source bytes. Use existing Unicode sentence-boundary
  primitives with the current abbreviation/decimal policy; do not reuse the
  quotation-selection pipeline's omission/packing policy. That owner deliberately
  withholds incomplete or oversized units (`quote_segments.rs:103-125`).
- A sentence of at most 240 scalar characters is one selectable piece. For a
  longer sentence, cut at explicit sub-clause punctuation: a semicolon or colon
  followed by whitespace, or a paragraph break. Choose the furthest such boundary
  within the cap and repeat. A physical wrapped line is not a paragraph boundary.
  Existing word/token boundaries determine the outer edges; preserve internal
  whitespace and punctuation exactly. Keep bounded trailing text as an exact
  source piece without asserting it is a complete sentence.
- Do not cut a word, invent punctuation, generate overlapping windows, or split
  merely because an arbitrary character count was reached. If all non-whitespace
  text cannot be covered by bounded pieces under this policy, reject the input
  before generation. Do not silently omit an unrepresentable restriction while
  offering the rest. This may retain fallback for long unbroken prose; disclose
  it as a measured limitation rather than adding a control-specific split.
- Each non-whitespace source character belongs to one piece. Inter-piece
  whitespace stays in the complete original context. Deduplicate identical
  segment strings within each side's enum; identical text need not occupy two
  grammar entries. Keep every owning full context in the request. Deduplication
  must not merge the contexts or equate their surrounding meanings.
- Number of entries and total segment text grow with the input, not with all
  pairs of token endpoints. Do not add a top-N truncation, sampling, or a second
  catalog-count workaround. Measure actual serialized bytes, including escaping.

### Exact segment text in the decoder and parser

- Keep `source_spans` and `claim_spans` as arrays of strings. Their item schemas
  reference finite enums containing the exact request-owned segment strings.
  The grammar selects text directly. No positions and no free copied quotes.
- Present those same bounded choices to the model alongside complete source and
  claim text, using the existing request/context projection. Construct prompt,
  schema and parser membership from one prepared catalog, not independent copies
  with different splitting rules. Preserve decoder dimension/field order.
- Preserve all four dimensions, four selections per side per dimension, existing
  relation meanings, required-field checks, positive-evidence requirement and
  verdict aggregation. The base parser already checks set membership before
  relations (`comparisons.rs:206-244`); keep that enforcement point.
- **Explicit override of the failed contract's quote-output prohibition:** the
  model may return a segment string permitted by the finite decoder enum. After
  JSON decoding, Rust must compare its UTF-8 bytes with an exact member of this
  request's owned segment set on the correct side, before deriving a verdict.
  A returned quote is checked byte-for-byte against owned text, never trusted.
  No case folding, whitespace normalization, fuzzy matching, quote repair,
  joining separate selections, or acceptance merely because a substring occurs
  somewhere in the document. JSON escape spelling may differ; decoded text may
  not. A bypassed/ignored grammar must still fail closed in Rust.
- IDs are only a possible separately contracted fallback if text enums cannot
  satisfy the runtime gates. Do not implement an automatic ID retry or dual path.
- Give the changed verification protocol a distinct version/schema identity;
  reserve the failed experiment's verification13/schema-v2 identities. Start
  from the base's verification12 support, preserve completed base/older results,
  and add no historical live generation path. No transport changes unless actual
  schema admission proves the shared identity change insufficient.

### Carry forward the proof, not the position implementation

Port the following narrowly from the failed experiment's test support, adapting
only its connection to the new owner:

1. Boundary measurement/regression, including admitted/rejected neighbors for
   sparse, prose and dense layouts and both runtime cap profiles. Old numerical
   maxima are observations of the old prompt, not expected values for this one.
2. Passage-accuracy scoring against immutable recorded C9 source/claim strings,
   with exact list/span matches, missing/extra strings and per-side differences.
3. Separate passage-membership validity from complete response/relation validity.
   A correctly copied permitted piece with an invalid relation must not be
   reported as a copying failure. Preserve the missing-side reporting regression.
4. Whole-document fallback for one over-limit claim remains explicit non-scope,
   tracked in [issue #124](https://github.com/canfieldjuan/document-summarizer/issues/124).

## Explicit non-scope

- No synthesis, extraction, heading rules, full clause list, UI, public API,
  dependencies, model/settings/output-budget changes, retries or deployment.
- No position-candidate token tables in the prompt, range decoder, range-to-text
  adapter, new context allocator, or its live historical protocol support.
- Do not bring back overlapping substring enumeration as a production validator
  or an alternative generation path. A bounded test oracle is allowed for proof.
- No label changes, control-specific segmentation, automatic relabeling of
  passage differences, or tuning on a fresh held-out batch.
- Preserve all existing caps. The base input limit is **4,096 characters for
  claim plus unique sources combined**, not 4,096 per side. The prompt limit is
  16,000 characters; output allowance is 4,096 tokens. Gateway schema cap remains
  250,000 bytes and native remains 1,048,576 bytes. Do not promise a 10 KB ceiling
  without measuring the actual serialized schema.
- Issue124's fallback behavior and issue123's upgrade/resume concern are separate.
  PR116's fidelity hold remains until its independent release gates are met.

## Assumptions/blockers

- The operator clarified that **segment-text enums** are the intended format;
  free copied text is ruled out. The earlier copied-quote interpretation was
  incorrect and is superseded here.
- Coarser pieces cannot be expected to equal all old phrase selections. The
  operator's clarified gate measures coverage of the recorded passages, retains
  verdict parity and makes zero wrong approvals mandatory. Exact-match scores
  remain visible; they are not renamed into coverage scores.
- Conservative splitting may leave long unbroken text unrepresentable. Record
  that explicitly in sizing/control receipts. If a recorded control passage
  crosses a fixed cut, report the catalog limitation before inference; do not
  move that cut to match its label or relax the scorer after seeing results.
- Text enums are the candidate, not a proven 9B improvement. Real gateway/native
  grammar compatibility and live selections remain unproven.

## Verification plan

1. Reproduce baseline expansion with a public, sentence/sub-clause fixture and
   the real planner. Freeze input and base commit. Show the admission regression
   fail before the origin change and pass afterward, with sibling inputs.
2. Prove deterministic non-overlapping pieces and complete non-whitespace coverage
   for sentence ends, abbreviations, decimals, initials, tabs/wrapped lines,
   paragraphs, semicolons, colons, Unicode and a trailing fragment. Test lengths
   below/at/above 240; long unsplittable text must fail before generation.
3. Probe parser and decoder independently: exact piece accepted; near-copy,
   normalization, foreign/wrong-side piece, blended pieces, partial piece,
   missing/duplicate/extra fields, empty dimensions and invalid relation rejected
   or ambiguous as the base contract requires. Exercise repeated identical pieces
   across contexts without losing their governing text. No fuzzy matching.
4. Keep frozen C9 fixtures unchanged. Score each chosen source/claim piece against
   each recorded passage in the **same dimension and side**. Coverage means the
   exact recorded bytes occur within one chosen piece at admissible token
   boundaries. Neither overlap nor a numeric/word prefix counts: `$27` inside
   `$270` is a negative regression. Do not concatenate separate pieces to obtain
   a match. Record exact match, full containment, missing references and extra
   selected text separately; neither containment nor membership alone proves a
   correct semantic verdict.
5. Before the live run, report whether the frozen catalog can represent every
   reference under that coverage rule. A reference crossing a piece boundary
   is an explicit failure, not silently removed from the denominator. This
   static check uses the same frozen splitter; it must not tune it.
6. Measure complete projected prompt and schema sizes using both actual runtimes
   and the unchanged settings. Report maximum admitted claim+unique-source sizes
   and first rejected neighbors by layout, including segmentation failures as
   distinct from context/prompt/schema limits. Pin the actual boundaries with
   regression tests. Preflight alone does not prove decoder grammar execution.
7. Run focused comparison/version/transport tests, formatting and strict Clippy;
   reopen copied historical results unchanged. CI owns duplicate broad suites.
8. Freeze one candidate, then run the same four approved controls once with the
   frozen 9B settings. Retain raw requests, schemas, responses, chosen pieces,
   recorded references, full coverage/exact-match differences, memberships and
   relations. Require all four verdicts to match their independent labels,
   zero wrong approvals, and every recorded passage covered in its dimension
   and side. Any invalid copy, missing coverage or verdict mismatch fails this
   candidate. Do not weaken validation, relabel controls or retry with IDs.
9. Only after those gates pass, resume the full public A/B worker proof and
   independent review. Fresh unseen fidelity qualification remains later.

## Implementation summary

Contract only. Source, tests, production settings and frozen fixtures are unchanged
from the specified base. Expected implementation surface: `summary/comparisons.rs`,
its directly owned splitter/tests, the shared schema/version constants and
completed-artifact tests, and the canonical verification contract. No code from
the failed candidate has been carried forward yet.

## Cold diff audit

Only this contract is added. Its baseline catalog/validation claims were checked
against `49e22a7`; its failed-candidate claims were checked against retained raw
receipts. A source-tree comparison proves no implementation is included. No code
tests are needed or claimed for this documentation-only revision.

## Gap audit

NOT DONE. This is an accepted design, not an implemented or qualified candidate.
Splitting, schema/runtime bounds, coverage feasibility, live controls and
independent review remain to be proven before any merge. Preserve and disclose
failures rather than starting another implementation round by default.
