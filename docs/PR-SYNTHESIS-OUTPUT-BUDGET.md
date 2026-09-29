# Synthesis response capacity (#95, PR #107)

## Root cause

The original application permits eight 1,200-character text units plus source
IDs and JSON, but grants only 2,048 output tokens. B's saved reproduction used
all 2,048, stopped at its output limit and delivered no summary. It contained
progressing content, not a repeated-unit cycle.

The first version of this PR introduced a second defect: it selected text
length with a six-byte-per-character worst-case bound. Eight times 1,200 times
six exceeds 32k before the prompt, so full paragraphs could never be admitted.
The test required that shrink. The operator's issue-comment instruction to
correct this was missed before publication. This document and implementation
supersede that sizing rule; earlier delivery passes did not qualify it.

## Required change surface

One request-local coherent budget owner serves catalog/source-selection
admission, generation and repair. It searches the existing exact runtime
preflight for all available output after framing and initial-feedback reserves,
and grants that allowance. It keeps the existing 1,200-character ceiling when
the calibrated estimate fits. Only a measured-estimate shortfall can reduce the
initial ceiling; the final schema is re-admitted and locally enforced.

Count canonical JSON and the longest permitted source IDs as byte overhead,
plus the calibrated text estimate and 256 output tokens for formatting and a
completion boundary. Initial admission reserves 512 tokens for repair feedback
on top of runtime framing. Repair consumes that headroom and re-admits its real
prompt without reducing the previously admitted text ceiling. Existing bounded
repair counts are unchanged. The six-byte bound is diagnostic only.

The gateway retains its existing 4,096 output cap and capacity rejection code.
The initial probe includes feedback headroom, so it conservatively leaves 512
of that cap unused; repair can consume it. Its preflight checks wire bounds,
not the deployed model's exact input count. Direct runtimes retain their exact
pinned tokenizer/framing checks. No new runtime interface is introduced.

Safety stays with the verified completion boundary: llama.cpp output-limit
stops and Ollama length stops reject the entire response, including parseable
JSON. Unknown/missing stops remain rejected. Usage and distinct failure reasons
are retained; no prefix salvage. Existing content, source, modality, coverage,
verification and persistence gates still apply after a completed response.

## Explicit non-scope

No prompt tuning, presets, installed settings, new runtime/dependency/API,
storage changes, extra generation retries, source-catalog changes, wider clip
repair, coverage-policy changes, OCR, or #100 push. Analysis, selection and
verification retain their existing separate budgets. This does not qualify
model-generated contract prose or promote a model.

## Assumptions and verification

The measured ratio is an estimate from a limited retained corpus; qualified stop
checks are the safety boundary even when that estimate is exceeded. See the
calibration below. Exact preflight errors unrelated to capacity stay fatal.
All admission data is request-local: no shared mutable state, race, queue, new
storage write or additional inference call is introduced.

Fail first on B's 32k request retaining 1,200. Verify small/tiny 32k and fitting
8k requests, true shortfalls, exact output boundary plus one, zero/exhausted
contexts, malformed schema, gateway caps, frozen repairs and local length
checks. Capture production generation/repair requests, and reject pathological
escaped text after a limit stop in both adapters. Run adjacent synthesis tests,
adapter tests, format and strict clippy; CI owns duplicated platform suites.

Offline replay must use original saved requests and the fingerprint-verified
production tokenizer. Live proof reruns A/B General and the public coherent
control with the same isolated 9B/32k profile, exclusive GPU, unchanged delivery
gates, and separate raw-versus-delivered results. Preserve every failed attempt.
Use opaque A/B labels; private text, filenames and paths never enter public Git.

## Implementation and current evidence

Contract correction `d08e6fd` precedes implementation `703ede7`. The new
`b_and_small_32k_prompts_preserve_1200_character_sections` failed on the published
selector with actual 210 versus expected 1,200 at input count 20,395. The fix
passes seven budget tests and the adjacent synthesis suite (249 passed, 10 live
ignored). Adapter suites pass 33 Ollama and 30 llama.cpp tests, including escaped
parseable JSON at the stop boundary. Formatting and strict all-target/all-feature
clippy pass. No broad/platform CI result is claimed for this corrected head.

Two initial adjacent failures were test issues in this correction: the new
repair fixture lacked the modal source needed to exercise a second call, and an
existing coverage test expected identical output allowances. The fixed tests
exercise the repair and retain schema/seed/ordinal/call-count checks, while
allowing the repair to consume the reserved feedback space.

The earlier matrix at `ccc93df` plus frozen #100 source `a192521` passed eight
delivery gates, seven via fallback. Its smaller decoder bounds are the defect
being corrected, so those results are historical only. The clean additional
three-stage repeat at `da63922` is also historical. Their receipts, the
interrupted foreign-GPU attempt, and the earlier four-request replay remain
retained privately.

Corrected offline replay passed on `703ede7`. All four saved requests reproduce
provider input-token counts and retain 1,200-character sections. Original A
(input 9,701) receives 22,043 output tokens; original B (20,395) receives 11,349.
The after-B request (20,385) receives 11,359. Eight full-length units using the
Unicode/escaped-prose fixture and maximal source-ID lists use 2,915 tokens for A
and 2,979 for B. The pathological fixture uses 22,708 / 22,772, exceeds available
space, and is not claimed to fit. Runtime limit-stop rejection is tested
independently. The production Rust tokenizer also reproduces all retained text
counts and both stress-fixture counts used in calibration.

Offline replay receipt SHA-256: `05dd4ff3350d1433c08fa3e2b75d4a24a35f560c2dbdeba4f214a5c4b81d30a9`.
A test-executable selection helper failed after a successful build; its failed
setup receipt is retained and no inference ran in that setup step.

Corrected live proof at `703ede7` passed all three selected delivery gates with
actual 32k context, thinking off, exclusive GPU and unchanged source verified.
A/B/public-control all retain 1,200-character sections; their output allowances
are 22,043 / 11,359 / 30,603. A and B deliver verified ledger fallback covering
10/10 and 14/14 text pages. The public report keeps coherent prose on 5/5.
A's raw four units cite pages 1-3; B's two cite pages 1-2. Some units still hit
the original 1,200 ceiling. Real-contract prose remains unqualified; the sizing
repair is not a quality claim. Seeds differ from earlier runs. All 65 responses
are retained, no thinking markers appeared, and GPU cleanup was empty.
Live receipt SHA-256: `18a100319f38a61f7e985a41e9d2b432d511ebdde535dbbe52e79505f4edd573`.

## Cold diff audit

- `summary/coherent/budget.rs`: calibrates text separately from structural
  overhead, maximizes admitted output, preserves fitting paragraphs, reduces
  only for estimated shortfalls, and re-admits the final schema. Budget boundary
  tests cover the controlling values and the actual downstream request.
- `summary/coherent.rs`: replaces the byte-bound test with captured production
  generation/repair requests proving that both retain the original schema.
  Existing production call sites continue to consume the same budget owner.
- `summary.rs`: tests allow bounded token-capacity probes and repair consumption
  of headroom; same schema, seed, one-repair limit and delivery checks remain.
- `model.rs` / `llama_cpp.rs`: this correction adds escaped-output regressions
  only. The earlier stop-rejection/classification implementation remains intact.
- Gateway code is unchanged by this correction. Canonical requirements and this
  contract describe the corrected estimate and its limits.

boundary-probe: fitting 32k/8k requests keep 1,200; measured shortfalls reduce;
zero/exhausted contexts and unrelated runtime errors cannot generate. Exact
capacity succeeds and one extra output token fails. A complete escaped response
can pass the stop gate, while the same parseable content at a limit stop cannot.

effect-trace: the section ceiling is controlled by `budget::admit`, consumed by
the schema in the generation request. The fail-first 210-versus-1,200 assertion
now passes, and generation/repair capture proves the original schema survives.
The output allowance comes from runtime preflight, not the text estimate.

## Gap audit

NOT DONE for merge: corrected implementation, regressions, offline replay and
live proof pass. Publication, review-thread reconciliation and exact-head
CI/review remain. #100 remains frozen; there is no model promotion or prose
qualification.

## Contract revision: preserve paragraph capacity (#107 review)

### Root cause

The original owner treated a worst-case serialized byte upper bound as a normal
output estimate. Eight times 1,200 times six exceeds 32k before any prompt. This
made the existing paragraph ceiling impossible to admit and the regression test
incorrectly required that loss. Limit-stop rejection already provides safety;
shrinking normal prose to protect against every possible encoding is wrong.
The issue comments asking for this correction were missed before publication.

### Required change surface

- `coherent/budget.rs`: search exact preflight for the maximum output allowance
  after the existing framing/initial-feedback reserves and wire cap. Keep 1,200
  characters whenever the measured estimate fits; reduce only for an estimated
  capacity shortfall. Re-admit the final schema and preserve repair ceilings.
- Estimate text at 23/100 tokens per Unicode character, rounded up, plus the
  canonical JSON/longest-ID byte overhead and 256 formatting/completion tokens.
  Keep the six-byte bound for diagnostics, never for admission selection.
- Replace tests that require shrinking. Prove real B and small/tiny prompts at
  32k retain 1,200, representative 8k requests retain 1,200, and genuinely tight
  capacity reduces the ceiling. Prove all available admitted output is granted,
  gateway caps/repair reserves remain, and pathological limit stops reject the
  whole answer. Keep unrelated runtime stop and content checks unchanged.

### Calibration and assumptions

The fingerprint-verified Qwen3.5 tokenizer
`d5ea96b68508288e2e6c70c4d80c47ba8471d020019e36a9d842464fcf1ec4b7`
measured 11 unique retained coherent responses from the prior production runs.
Nearest-rank p99 is 0.22922983626440266 tokens per text character (minimum
0.16129032258064516), measured by summing each canonical JSON text-string token
count and dividing by Unicode text characters. Round p99 upward to 0.23.
No private text enters the repository; local calibration retains input hashes.

Measure stress fixtures separately from the natural-output percentile: a
quote/backslash/newline/accent/CJK prose fixture has 361 tokens / 1,332 characters
(0.27102102102102105); a control/escape/CJK/non-BMP pathology has 2,801 / 1,200
(2.3341666666666665). They explicitly exceed the estimate. Including the
pathology as a worst-case selector would recreate the defect; safety remains
verified-stop rejection, not a claim that p99 bounds all output. The estimate is
calibrated to this retained sample, not a model-agnostic quality guarantee.
A long prompt or ID-heavy maximal response may still need a smaller ceiling
when this measured estimate does not fit. Existing 8k behavior is preserved for
fitting representative shapes; arbitrary 8k inputs cannot guarantee full shape.

### Non-scope

No prompts, presets, installed settings, additional inference retries, model
promotion, source/coverage validators, runtime API, dependencies or storage
change. Keep the stop-classification and gateway fixes already in this PR.
#100 stays frozen. No unrelated work while this correction is outstanding.

### Verification plan

Fail first on the published selector retaining 1,200 at B's 32k input count.
Then run the budget/adjacent synthesis tests, adapter stop tests and format/lint.
Replay saved A/B requests with the pinned tokenizer and full-length representative
responses; distinguish empirical sizing from pathological upper bounds. Repeat
the affected A/B General production runs and public coherent-document control
with unchanged acceptance gates and exclusive GPU before reporting live proof.
CI owns duplicated broad/platform suites. Do not resolve review threads until
the corrected code and proof are published.

### Gap audit

NOT DONE for merge: sizing correction, regressions, offline and live proof pass;
publication and exact-head CI/review remain. The earlier
eight delivery passes did not establish preservation of coherent-summary room.


## Review correction: carry the admitted ceiling through recovery

### Root cause and required change surface

The sizing owner writes a reduced `maxLength` and the response-budget check
accepts it, but the coherent parser, clipped-unit salvage and repair feedback
still use the global 1,200-character default. A completed JSON response with an
incomplete unit at the reduced decoder ceiling therefore becomes a generic
invalid response instead of receiving the existing bounded repair or preserving
valid siblings. This mismatch was introduced by this PR's variable ceiling.

Read the admitted character ceiling once from the request schema using the
budget owner's accessor. Pass it through primary parsing, window/framing repair
analysis, clipped-unit salvage and retained-sibling parsing. Repair feedback must
name the same ceiling. Keep existing unit-count limits, source/metadata checks,
windowed-General eligibility, frozen repair schema and retry counts. The default
1,200-character route and all runtime stop checks remain unchanged.

### Verification and non-scope

Reproduce through the generation loop with an actual preflight shortfall that
reduces the initial schema. Prove one clipped-unit repair, repeated-clipping safe
fallback, and complete text at the exact admitted ceiling needing no repair.
Check below/at/above the ceiling, invalid citation metadata, nested helper paths
and the existing 1,200-character regressions. Run adjacent synthesis tests,
formatting and strict clippy. This is deterministic request-limit propagation;
no model inference or replay of the unchanged full-ceiling live cases is needed.

NOT DONE until the fail-first reproduction, fix and regressions pass and the
published head is reviewed with green required CI and resolved threads.

### Disposition of nonblocking review suggestions

- Defer the proposed output allowance of estimate times 1.25 plus formatting.
  This changes the authorized all-available-capacity behavior and needs measured
  latency/completeness evidence. Existing output-stop rejection remains the
  safety boundary; current proof is not a runaway-latency qualification.
- Defer source-script density calibration. Runtime preflight currently returns
  only success/failure, not prompt token counts. Instruction/JSON/identifier
  density is also not a validated estimate of generated prose density. A shared
  tokenizer/profile calibration change needs separate evidence; this fix does
  not introduce that interface or claim the existing ratio covers every script.

Both suggestions are recorded as nonblocking follow-up work under issue #95;
they are not merge gates for this admitted-ceiling correction.
