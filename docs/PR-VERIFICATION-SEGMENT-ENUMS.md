# Bounded segment-text enums for General verification

Status: design accepted by Codex in response to the operator's clarified
recommendation. Contract commit `5adaa3b` preceded implementation. The local
candidate and focused proof are now complete; actual runtime boundaries and
live controls remain pending. PR116 remains fidelity-held.

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

Implemented only on the requested base. `comparisons.rs::segment_ranges` and
`segment_catalog` build bounded owned pieces; `Prepared::prompt` presents those
same choices; the existing parser retains exact string membership before relation
aggregation. Source/claim text is complete. The old substring expansion and
8,192-entry representation limit are removed. No position implementation or ID
allocator from the failed branch was carried into production.

The shared schema name is v3 and verification is 14.0.0; completed verification12
and earlier supported artifacts retain their validation. No live historical
planner was introduced. Test support carries the boundary, passage-accuracy,
separate validity and saved-view proofs requested by the operator. The frozen
C9 fixture is unchanged from the base.

### Reproduction and origin fixes

- Expansion: the public 12-sentence input failed the real baseline planner with
  `VERIFICATION_INPUT_TOO_LARGE`. Its small source was expanded at the old
  `span_catalog` owner, not rejected for its authoritative input size. Replacing
  that catalog admits the original fixture and the smaller/larger siblings while
  preserving complete source text. The permanent admission regression passed.
- My first splitter used the general sentence iterator, which grouped lowercase
  numbered sentences into one oversized unit. The isolated input
  `w000 w001. w002 w003.` reproduced one piece instead of two. The origin now
  proposes Unicode terminal boundaries directly and applies the shared
  abbreviation/decimal predicate. That regression and the original admission
  regression pass; the frozen input was not changed.
- My terminal scanner initially split `Ask now?!` into `Ask now?` and `!`. The
  fail-first regression reproduced it. The owner now consumes the complete
  terminal/closing-quote group before proposing a cut. The original example,
  repeated terminals and quoted siblings pass. No downstream repair was added.

### Local proof before the live freeze

- Final comparisons: 17 passed, two opt-in tests ignored.
- Focused coherent verification: 10 passed, two ignored; C9/transport: 20 passed,
  one ignored. These counts overlap and are not an aggregate suite total.
- Copied public A/B saved views: one opt-in test passed, both views unchanged;
  original database hashes unchanged. The replay uses read-only connections.
- Strict all-target/all-feature Clippy passed. Frontend build passed to supply
  the fresh worktree's required Tauri test assets; an initial missing-dist
  compiler failure is retained separately from the fail-first evidence.
- Static catalog feasibility covers all 32 recorded passages across four controls
  under the unchanged labels. This is an oracle ceiling, not a live model score.
- Planner boundaries (claim plus unique source): sparse 260/261, ordinary prose
  4,096/4,097, dense punctuation 260/261, admitted/rejected neighbors on both
  cap profiles. The sparse/dense limits arise from unsplittable text, not schema
  growth. Prose reaches the combined input cap. Actual runtimes still must be
  measured. No cap was raised, and no old numerical maxima were copied as claims.

Durable evidence prefix: `verification-revision-contract-20261005/`.
The fail-before/pass-after logs, catalog oracle, boundary measurements, source
receipts and saved-view replay are retained there. The failed position experiment
remains separately archived under its original prefix and is not overwritten.

## Cold diff audit

| File / owner | Change and contract trace | Proof |
| --- | --- | --- |
| `summary/comparisons.rs` | Replace expanded excerpts with owned segment enums; one prompt/schema/membership owner | Admission, segmentation, exact membership and boundary regressions |
| `summary/comparisons/tests.rs` | Carry measurement/scoring/reporting; add containment and parser negatives | Static four-control oracle, focused suite, source-preserving receipts |
| `pipeline/contracts.rs` | Name the changed decoder protocol v3 | Native/gateway C9 transport checks |
| `summary.rs` | Emit verification14; retain completed12 | Saved-version integration and copied-view replay |
| `summary/coherent/verification/tests.rs` | Include saved12 in reopened-artifact cases | Focused integration suite |
| `docs/CONTRACTS.md` and this file | Describe the actual representation, limits, gate and fallback non-scope | Compared with the controlling code; issue124 unchanged |

Boundary-probe: exact/near-copy, mixed/missing fields, wrong side, unknown IDs,
invalid relation, Unicode lengths, token-prefix containment, split/join rejection,
empty dimensions and admission neighbors pass. Runtime measurements are pending.

Effect-trace: avoid excerpt expansion while forcing exact text choices |
`segment_catalog` supplies each enum and `Prepared::parse` checks that same set |
original admission regression now passes; non-member copies fail even if decoder
constraints are bypassed. This does not establish model fidelity.

## Gap audit

NOT DONE. Actual gateway/native measurements and the frozen live control gate are
next. Full public A/B worker proof is conditional on that gate. Independent review
and fresh unseen qualification still precede release; PR116 remains held. A failed
live candidate must be recorded and frozen without relabeling or prompt tuning.
