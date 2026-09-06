# Corpus and native Ollama evaluation — updated 2026-09-05

## Qwen-family qualification: four candidates fail; baseline passes

**Failures first. Neither smaller model qualifies, and neither Qwen 3.8 27B
GGUF can initialize in the current Ollama runtime.** No prompt, validator,
coverage threshold, output allowance, timeout or summary limit was changed
between candidates. Failed candidates remain visible as installed models but do
not enter the product preset registry.

| Candidate and exact digest | NARA robustness fixture | DOL product fixture | Qualification |
| --- | --- | --- | --- |
| Qwen 3.5 4B Q4_K_M, `fa9cc8f5d580d7aa9492360539a99a95cdab2cab11848b2ce0aba1a2bf88b7c5` | **Fail:** 1 claim / 1 evidence; 1/11 raw, 1/10 adjusted pages; 10 omissions; 30 requests; 3,110 completion tokens; 53.39 s | **Fail in analysis:** no delivered claims; 203 requests; 17,693 completion tokens; 273.99 s | Hidden |
| Qwen 3.5 9B Q4_K_M, `9a83bb8ce0da6b12ab5b6cc3f35eb65e6c0bd4ff39b6e134871b89c1ab522fb7` | **Fail:** 1 supported claim / 3 evidence; 1/11 raw, 1/10 adjusted pages; 8 omissions and 2 withheld claims; 29 requests; 5,905 completion tokens; 109.60 s | **Fail in analysis:** no delivered claims; 20 requests; 3,977 completion tokens; 67.92 s | Hidden |
| Base Qwen 3.8 27B Q4_K_M, `f9afc1701e366c19aaf6a7a2fd0b38dcef79610a83e8b19094fa33d7ed52a6f4` | Not started: loader failure | Not started: loader failure | Hidden |
| Jack Qwen 3.8 27B Coder, `ae9075536f80595201465f14970ca65eade0950c53ab71ff2fee34c8f24b1ec8` | Not started: loader failure | Not started: loader failure | Hidden |
| Qwen 3 30B-A3B Q4_K_S, `1eda56426671cdf365913097543c2253a73c57e35b12741306689968d7f70292` | **Pass:** 6 claims / 7 evidence; 6/11 raw (54.55%), 6/7 adjusted (85.71%); 4 omissions and 1 withheld claim; 21 requests; 425 completion tokens; 18.18 s | **Pass:** 82 claims / 83 evidence; 82/111 raw and adjusted (73.87%); no omissions and 1 withheld claim; 176 requests; 5,615 completion tokens; 91.40 s | Selectable full preset and strongest verifier |

The 4B and 9B failures are capability results, not low-context results. Both
were run as analysis models at an 8,192-token qualified context with the
qualified 30B verifier. Both repeatedly filled the 1,536-character grammar
ceiling instead of returning a complete bounded paraphrase; their DOL runs
failed before verification. NARA additionally showed extensive typed
`ParaphraseUnrepairable` omissions. All loadable-candidate attempts used native
structured output with zero schema-fallback attempts.

The two 27B files are distinct candidates and were tested through Ollama, not
the LM Studio runtime. `/api/show` reports `qwen35`, 27.3B parameters and a
262,144-token model maximum for both. The base file is 16,810,714,604 bytes and
the Jack file is 12,599,204,589 bytes. A minimal native structured `/api/chat`
request fails for each before generation with `qwen3next: layer 64 missing
attn_qkv/attn_gate projections`. This is an Ollama loader incompatibility, not a
prompt, schema or corpus verdict.

The three loadable model descriptors all report a 262,144-token maximum, but
qualification intentionally retains an 8,192-token effective context. Every
native request carries that `num_ctx`, its stage `num_predict`, `think: false`,
temperature zero and the run-derived seed. Exact serialized request admission
uses the pinned tokenizer versions `qwen3-qwen2-pre-f2ec4434-v1` and
`qwen35-pre-cc5fb918-v1`; it does not infer compatibility from a model name.

| Model / fixture | Analysis requests / completion tokens | Verification requests / completion tokens | Schema fallback attempts |
| --- | ---: | ---: | ---: |
| 30B / NARA | 20 / 341 | 1 / 84 | 0 |
| 30B / DOL | 170 / 4,627 | 6 / 988 | 0 |
| 4B / NARA | 29 / 3,092 | 1 / 18 | 0 |
| 4B / DOL | 203 / 17,693 | 0 / 0 | 0 |
| 9B / NARA | 28 / 5,865 | 1 / 40 | 0 |
| 9B / DOL | 20 / 3,977 | 0 / 0 | 0 |

The hoped-for small-model speedup did not materialize. On DOL, 4B ran about
three times longer than the completed baseline before failing; 9B failed early
and therefore is not a completed-work speed comparison. The accepted outcome is
therefore a stronger Qwen-family runtime boundary with one qualified preset,
not nominal multi-model support that weakens the product for the smaller files.

Contract commits `76b209b` and `34f036b` precede native runtime/settings commit
`3a75d04`; immutable run-profile persistence lands last in implementation commit
`4d42890` as schema version 15. Discovery credential parity is hardened in
`9bc9afc`. A cold-diff audit then found two implementation gaps before final
review. An unreadable `/api/show` response for one installed model aborted the
whole catalog instead of leaving that model visible but unqualified, and desktop
retry/continuation rebuilt the current global preset before comparing it with
the persisted run snapshot. The corrected boundary isolates only per-model
metadata failures while keeping transport/authentication fatal, and reconstructs
continuation/retry runtimes from the immutable admitted snapshot; new runs alone
use the current setting.

Review reconstruction found two further recovery defects before merge. The
frontend treated "stored preset is valid" as "there is a selectable recovery
preset," so a later runtime-status refresh disabled the recovery option. The
settings writer also used plain `fs::rename`, which cannot replace an existing
destination on Windows. Selection availability and selection-in-flight state are
now distinct, and same-directory atomic persistence uses replacement semantics
on Windows while retaining the exact private mode on Unix.

Exact-head review then found two identity-boundary gaps. Pre-schema-v15 runs
that had already persisted analysis or synthesis artifacts had no immutable
model profile, yet history still advertised continuation and desktop admission
could construct the current preset before later failing. Those model-artifact
checkpoints are now non-continuable without a snapshot and are rejected before
worker admission; pre-model checkpoints may still select the current qualified
runtime. A reconstructed snapshot runtime is also health-checked before a
continuation worker is spawned or a retry child is created, so an unavailable or
repointed model leaves the durable checkpoint, events and retry lineage
unchanged. Separately, an Ollama tag can be repointed after stage health succeeds.
The first repair re-read authenticated `/api/tags` after each successful
`/api/chat`, but exact-head review found that a tag could be repointed during
generation and restored before that lookup. Qualified chat requests now retain
their runner for 30 seconds, and the adapter reads authenticated `/api/ps`
before parsing the response. It accepts output only when exactly one running
record for the requested name reports the qualified digest; a missing,
ambiguous or mismatched record fails closed. Request counts in the corpus tables
remain model-call counts, not discovery or execution-provenance calls.

The next exact-head review found two remaining admission-order gaps. Connect
constructed its runtime only after committing the job, so worker scheduling
could bind an accepted run to a preset selected later. It now constructs the
runtime before the guarded acceptance transaction and moves that exact instance
into the worker; construction failure removes the received import and persists
no job. Desktop retry now performs the same read-only source-state, version,
checkpoint and lineage checks used by the final transaction before model
discovery and health preflight. The transaction repeats those checks, preserving
race safety while stale or ineligible requests return retry errors rather than
model errors.

Final exact-head review found two related identity/error boundaries. Connect
had selected and moved the exact runtime into its worker but did not persist the
runtime profile until worker processing began, leaving a crash window in which
an accepted ingested run had no immutable profile. The profile snapshot now
commits in the same transaction as ingestion and Connect acceptance, and a
snapshotless production runtime is rejected before acceptance. Separately,
post-acceptance model failures were projected through a stale retryable-code
allowlist. Connect job errors now preserve the typed summary-stage failure's
`recoverable` value directly.

A delayed security review found that the byte-bounded `/api/tags` response had
no model-record cap and each sequential `/api/show` probe received a fresh
five-second timeout. A hostile same-user loopback listener could therefore
multiply startup delay by the number of records in one accepted response.
Discovery now admits at most 256 records before issuing any metadata probe and
shares one five-second deadline across tags and all show requests. Boundary
tests admit 256, reject 257 without a show request, and prove a stalled metadata
listener cannot multiply the aggregate deadline.

The local all-target gate passes with 302 library
tests and six ignored, three acceptance tests and three ignored, and three
release tests. Strict all-target/all-feature clippy with warnings denied,
formatting, frontend TypeScript/Vite build and diff checks pass. Hosted CI is not
claimed; the operator requires local checks because private-repository Actions
minutes are exhausted.

Both corpus acceptances were rerun after the execution-provenance change. NARA
passed with seven supported claims from seven retained evidence items, seven of
11 raw pages and all seven adjusted pages cited, 21 model requests, 432
completion tokens, and 28.74 seconds wall time. DOL passed with 82 supported
claims from 83 retained evidence items, 82 of 111 raw and adjusted pages cited,
176 model requests, 5,614 completion tokens, and 188.70 seconds wall time. Both
used primary structured transport with no schema fallback, and neither produced
an unverified execution record. DOL's higher wall time than the prior baseline
was concentrated in two analysis calls; request count and delivered coverage
were unchanged.

## Latest slice: both corpus acceptances pass; NARA remains robustness-only

**Known fidelity limitation first: NARA is not OCR-table accuracy evidence.**
Its flattened text layer fuses table rows on page 3. In this run verification
withheld that derived claim as unsupported, but an earlier run accepted the same
row-fused sentence. The new pass proves delivery, warnings, omission admission
and no crash; it does not prove that model verification can reconstruct table
relationships absent from extracted text. NARA remains an OCR/parser robustness
fixture.

**The exact-head NARA acceptance now passes without lowering either coverage
floor.** The pipeline retained seven evidence items and delivered six supported
claims. It cited six of 11 native-text pages (54.55 percent raw) and six of seven
omission-adjusted pages (85.71 percent). Page 4 was structurally denied the model
omission option because its complete-page text contains material markers; it was
retained, verified and cited. Pages 6, 7 and 11 remained recorded model omissions
for scan noise, and the page-12 date stamp remained a deterministic omission with
no model call. Page 3 was the single unsupported claim.

The exact-head rerun used 21 requests, all Primary transport, 425 completion
tokens and 21.58 seconds wall time. It completed with warnings including
`OCR_TEXT_LAYER_STRUCTURE_RISK`, `ANALYSIS_PAGE_OMITTED`,
`SEMANTIC_CLAIMS_WITHHELD` and `SUMMARY_COVERAGE_SHORTFALL`. The complete result
survived the acceptance test's independent reopen.

**The DOL native-text scale fixture still passes on the preceding exact head.**
That recorded run retained 83 evidence items and delivered 81 supported direct
claims, citing 81 of 111 pages (72.97 percent raw and adjusted). It used 176
requests, 5,657 completion tokens and 178.48 seconds. DOL was not rerun for the
marker-admission change because it recorded zero omissions and the changed path
is the NARA short-form case.

The NARA delivered summary was printed between
`OFFICE_LIVE_DELIVERED_SUMMARY` markers. Its complete text was:

> Item 2 (FN 213) remains active and may be used to disposition records, but the
> remaining items on this schedule are superseded, obsolete, or not approved for
> record disposition. [p. 1]
>
> The disposal request is subject to the provisions of 44 U.S.C. 3303a. [p. 2]
>
> For those formats marked with an asterisk (*), see line 11 of this form, as
> noted in the attached memo dated May 28, 2008. [p. 4]
>
> Upon approval of this agreement, transfer all eligible (30 years old)
> electronic records directly to NARA and pre-accession all additional records.
> [p. 5]
>
> The PDF records may contain embedded files or form data, and if so, this
> information will be captured and transferred to NARA separately from the PDFs.
> [p. 8]
>
> The PDFs contain embedded fonts, including the "base 14," as confirmed by the
> agency. [p. 10]

The DOL delivered text was 17,393 characters with SHA-256
`8436228de7271f893ed7de6b21b601e3c21766c3405439f8cfbdd0a9930cc987`.
Response text tracing was disabled for that earlier retry, so it proves the
persisted artifact and acceptance metrics but does not add another transcript.

Documentation-only contracts `e5bc59b`, `4344ec0` and `63b6ba8` precede their
implementations `77742f4` and `d1e9ac3`. Local all-target tests pass with 265
library tests and six ignored, three acceptance tests and three ignored, and
three release tests. Strict clippy, formatting and diff checks pass. The
separate NARA live acceptance passes on the implementation head as reported
above. Hosted CI is not claimed; the operator requires local checks because
private-repository Actions minutes are exhausted.

## Previous slice: direct paraphrases implemented; live control timed out

**NOT DONE. No new NARA or DOL delivered summary is established for this slice.
The previous corpus failures below remain the last completed measurements.**
The live substantive-omission control test failed with
`MODEL_RUNTIME_UNAVAILABLE` / `MODEL_ANALYSIS: Local model request failed`, exit
101 after 900.01 seconds, with no control outcome returned. NARA and DOL were
not started behind that failed probe. Local green gates do not settle whether
the omission model decisions are sound or the delivered direct summary is
readable and faithful. No new delivered text exists to print.

| Current slice | Claims / evidence | Raw / adjusted cited pages | Requests / completion tokens | Result |
| --- | --- | --- | --- | --- |
| NARA | Not measured | Not measured | Not run | Pending available Ollama |
| DOL | Not measured | Not measured | Not run | Pending available Ollama |

Observed runtime contention, not an isolated pipeline diagnosis: the website
generator's `build.py examples/prospect-plumber-template.json` process had an
established connection to the same Ollama throughout the probe and remained
connected after it timed out. Server load logs reported Parallel=1, context
40,960, 42/49 layers on GPU and an output layer on CPU; `/api/ps` confirmed the
selected `qwen3-30b-a3b:latest` model. The application still plans against its
unchanged 8,192 context assumption; this adapter does not set context. No other
job was interrupted, runtime restarted, context changed or timeout raised.
An idle-runtime rerun is needed to distinguish queue contention from any new
schema/runtime failure and to obtain comparable timing. The failed control was
`DOC_SUM_MODEL_NAME=qwen3-30b-a3b:latest
DOC_SUM_MODEL_BASE_URL=http://127.0.0.1:11434/v1/
cargo test --offline --lib live_typed_omission_preserves_substantive_controls
-- --ignored --nocapture` from `src-tauri`.

Documentation-only contract `9d81f89` precedes implementation `2ed9c07`.
The new default carries each retained paraphrase unchanged into one claim and
verifies once. No generation, assignment-array or adjacency-reduction call lies
on that path. This removes the observed orphan/forced-merge failure surface,
not semantic verification or the possibility of an incorrect paraphrase.

Correction to the diagnosis: NARA analysis response 5 already says
"Permanent. Cut off annually and transfer to WNRC. Destroy when 7 years old."
The later merge did not originate every source/interpretation defect. The
original deck paraphrases are often readable, but that alone does not prove
all their details are entailed by the selected quotations.

New paraphrase omissions are typed and source/catalog-bound. The option is
exposed only when the selected quote contains the complete page text, compared
with whitespace normalization only. Partial quote input cannot omit unseen tail
content. Full-page admission is not a semantic oracle: a mistaken model omission
remains possible and is recorded as a judgment, not as proof the original has no
facts. Deterministic filters, including the numeric/unit veto, are unchanged.
Raw and omission-adjusted coverage are printed together. Unsupported,
ambiguous and uninspected pages cannot silently reduce the denominator.

### Local verification and cold diff audit

On the implementation tree, `cargo test --offline --all-targets` passes:
255 library tests, six ignored; three acceptance tests, three ignored; three
release tests. `cargo clippy --offline --all-targets --all-features -- -D warnings`,
`cargo fmt --check` and the explicit formatting check for the test-only included
file pass. A serial all-target run also passes. Earlier parallel runs exposed
an unrelated entitlement child-lock test failure; that test passed in isolation
and in the final parallel run. No entitlement code was changed, and this does
not establish a fix for that intermittent test behavior. Hosted CI is not
claimed green; the operator requires local checks instead of private Actions
minutes.

| Changed surface | Actual effect / contract trace | Verification |
| --- | --- | --- |
| `summary/direct.rs:20` | Source-ordered unchanged paraphrases with one original evidence binding, ceiling 512; no model call | 1/83/512 accepted, 0/513 rejected; removed/reordered/rewritten/rebound claims rejected |
| `summary.rs:399`, `:445` | Route production to direct materialization and a single verifier pass; retain zero-supported failure and versioned reload | Direct request capture; shortfall/persistence/reopen and existing negative tests |
| `summary/pages.rs:268`, `:284` | Complete-page admission and disjoint typed omission schema, recorded audit and backfill | Partial/mixed/unknown/forged/duplicate/historical omission rejection; live substantive controls remain pending |
| `contracts.rs:479` | Explicit omission origin and reason, no schema migration | Serialization plus source/catalog/version validation tests |
| `db.rs:1786` | Remove unused retry-synthesis write wrapper; retain immutable attempt storage/read paths | SQL immutability, transaction failure and independent-reopen tests |
| `service.rs:1044` | Analyzed checkpoint now needs verification only, not synthesis generation | Continuation test expects the reduced request count |
| `summary/identifiers.rs:90` | Obsolete generation-error mapping is test-only; verifier ordinals remain production | Existing enum, identity and verdict negative tests |
| `summary/legacy_generation.rs:1`, `summary.rs:25` | Retired generation logic retained only as test fixtures, not an alternate production path | Existing synthesis/reduction/assignment negative tests still pass |
| `office_acceptance.rs:765` | Actual omission-adjusted denominator with raw reporting and opt-in complete delivered text | Existing zero/below/exact/above coverage boundary tests and report privacy tests; live proof pending |
| `CONTRACTS.md:418` | Fold and delete superseded proposed/amendment text into current versioned behavior | Compared against production routing, filters, version and request limits |

boundary-probe: direct capacity and tampered binding; whole-page versus partial
omission, mixed/unknown outcomes and forged fingerprints; verifier plan at the
new claim ceiling and all existing single-claim/batch-count size boundaries pass.
effect-trace: eliminate forced consolidation | production calls direct::synthesize
and then verify once | captured calls contain no synthesis generation, direct
text/bindings are unchanged, and a withheld claim causes no second verifier pass.
These are deterministic source-path proofs, not live quality approval.

## Previous structural run: BOTH corpus acceptances fail

**NOT DONE. DOL fails before delivery on two unassigned claim texts. NARA now
completes below acceptance, at 5/11 native pages (45.45%), versus the prior
passing 7/11 (63.64%).** Do not bury this regression behind passing local gates
or schema compilation. No validator, coverage target, B/K, retention, support
rule, unit veto, timeout or resource ceiling was weakened.

Contract `f4ce9d2` precedes implementation `369a871`. Both full sequential live
runs below exercise exactly `369a871`, using `qwen3-30b-a3b:latest` and the
existing Ollama adapter. There were no retry-until-green corpus reruns.

| Document | Acceptance | Delivered claims | Retained / supported-cited evidence | Native pages cited | Requests | Completion tokens | Model time | Test wall time |
| --- | --- | ---: | --- | --- | ---: | ---: | ---: | ---: |
| DOL deck | exit 101 | None | 83 / not verified | Not delivered | 174 | 6,809 | 90,852 ms | 93.27 s |
| NARA | exit 101 | 4 | 10 / 5 | 5/11 (45.45%) | 30 | 1,499 | 21,555 ms | 22.70 s |

### Exact failures, not inferred from constants

DOL response 173, the seventh initial synthesis request, contains eight claim
texts and eight assignments: `[0,1,2,2,4,4,7,6]`. Every slot exists and every
index is in range. Claim indices 3 and 5 have no assigned evidence: the former
concerns toilet units and water-container maintenance; the latter concerns
State Plan States and Wage and Hour Division authority. Rust rejects this as
`MODEL_CLAIMS_RESPONSE_INVALID`: every claim needs bounded nonempty evidence.
The preceding batches returned 8, 8, 8, 8, 3 and 8 claims with valid complete
assignments. The array shape solves missing evidence slots, but does not ensure
that every returned claim is used. This is the explicitly retained orphan-claim
rejection, not a foreign identifier or token exhaustion (the failed request used
310 of 4,096 completion tokens). No reduction or verification ran; no DOL
delivered coverage or independent-reopen evidence exists. Do not silently delete
the orphan texts or guess different assignments.

NARA completes the pipeline with durable warnings after its existing single
re-synthesis. Each pass initially produces eight plus two claims, then Rust
selects two pairs for text-only reduction to B=8. All ten retained evidence items
are cited by the synthesized set. In both passes the verifier withholds the same
four claims: one unsupported consolidated permanent-records/destruction and
asterisk-instruction claim, plus three ambiguous noise-description claims. The
final supported set has four claims and five evidence items/pages. Both 60%
supported-evidence and native-page requirements are missed; the test stops at
the supported-evidence assertion (`office_acceptance.rs:834`), before its final
independent reopen. Attribution to both pair members is exercised successfully;
semantic support is not inferred from that attribution. The permanent-records
source-fidelity caution remains, not a human-confirmed model verdict.

### What is proved and what changed in cost

All 174 DOL requests and all 30 NARA requests used Primary transport. The new
initial-synthesis alternatives and text-only reduction schemas compiled with no
JSON fallback. An early focused probe of the actual eight-alternative schema
also passed: eight slots assigned to one consolidated claim, 36 completion
tokens, 463 prompt tokens, 6,785 ms, Primary. The full deck additionally exercised
different valid claim counts, including three, before the orphan failure.

`summary.rs:2252` builds a complete object alternative for each feasible claim
count, with matching exact assignment length and integer-index enum; it does
not collapse the allowed claim-count range to a fixed number. `:2273` permits
only one text-only reduction claim. `summary/structural.rs:13` derives durable
evidence from ordered assignments; `:64` attributes both selected candidates;
`:88` validates the exact pair and its metadata before inference. The existing
durable parsers at `summary.rs:3048` and `:3144` still reject corrupt references,
noncanonical candidate evidence, duplicate references and excess evidence.
The loop at `:1247` is unchanged: reduce only above the budget. Verification
retains its exact local-ID enum and missing/duplicate verdict rejection.

| Comparison with ordinal baseline `463069b` | Before | Structural run |
| --- | ---: | ---: |
| DOL initial batches reached | 11 | 7, fails in seventh |
| DOL reduction requests reached | 2, failed first pair and repair | 0 |
| DOL total requests / completion tokens | 180 / 8,337 | 174 / 6,809 |
| DOL test wall time | 110.27 s | 93.27 s |
| NARA initial batches per synthesis pass | 2 | 2 |
| NARA reductions across both passes | 2 | 4 |
| NARA total requests / completion tokens | 28 / 1,538 | 30 / 1,499 |
| NARA test wall time | 22.36 s | 22.70 s |

DOL stops earlier, so its lower totals are not a completed-work speedup. NARA
does more reduction work and delivers less supported coverage. Run-derived seeds
and model output differ; this is measured end-to-end behavior, not a controlled
causal isolation of each prompt edit. Actual synthesis input allowance rises
from 8,068 to 8,143 characters only because the new prompt is shorter; the whole
request limit remains 10,752. Maximum-text 83-item preflight still needs twelve
batches, reserves 62 synthesis calls; escaped maximum-text cases reserve 204,
both below the unchanged 256 ceiling. No packing improvement is claimed.

Stage costs: DOL analysis 167 calls / 4,334 completion tokens / 69,198 ms;
synthesis 7 / 2,475 / 21,654 ms. NARA analysis 20 / 382 / 8,565 ms;
synthesis 8 / 927 / 9,868 ms; verification 2 / 190 / 3,122 ms.

### Local gates, cold audit and remaining gaps

`cargo test --offline --all-targets`: 253 library tests pass, five ignored;
three acceptance tests pass, three external tests ignored; three release tests
pass. Strict all-target/all-feature clippy with warnings denied and fmt --check
exit 0. The focused live schema test was run explicitly; its default ignored
status does not substitute for the two failed corpus tests. Two initial stale
expectations (old prompt wording and its derived character allowance) were
corrected before the final passing gates. Logs: `/tmp/doc-sum-structural.ypkpoT/`
contains `tests.log`, `clippy.log`, `dol.log` and `nara.log`; prior full logs are
`/tmp/doc-sum-ordinals.FAOByA/dol-final.log` and `nara-final.log`.

Boundary-probe: exact assignment length and per-alternative integer vocabulary;
short/long/mixed/out-of-range slots; empty/orphan/overfull claims; forbidden
model ID fields; exact pair attribution and canonical durable restoration;
duplicate/foreign/noncanonical candidate metadata rejected before a model call.
Existing durable negative tests and historical reload tests remain passing.
Effect-trace: model output now passes through structural attribution and then
the unchanged durable validators; NARA exercises actual text-only pairs and
both corpora exercise actual assignment arrays on Primary transport. The change
removes reference transcription/omission, not semantic loss or orphan claims.

Cold diff: only summary transport/schema/prompt code, its request-local helper,
the scoped structural helper/tests and documentation changed. No acceptance
assertions or source-selection filters changed. GAP AUDIT: NOT DONE for corpus
closure. The requested representation changes are implemented, but resolving
the orphan-claim failure and restoring NARA supported coverage requires further
contracted work. The broader pending materiality/audit obligations also remain.
PR #30 remains ready for review, not merged.

## Previous result: all identifier schemas constrained; DOL still fails consolidation coverage

**NOT DONE: the deck still has no delivered summary.** The first pair reduction
returns only c1 and repeats the same claim on its bounded repair, omitting c2
again. Rust rejects it as `SYNTHESIS_MISSING_REFERENCES` before verification.
The identifier-copying defect is fixed across synthesis, reduction and
verification, but membership is not complete coverage. NARA passes. No validator,
acceptance threshold, retention target, B/K, support rule or resource limit was
relaxed to obtain these results.

Contract `b884235` precedes code `4b4c7ed`; cold-audit correction `463069b` restores
terminal missing-reference errors to durable IDs while keeping repair feedback
local. Both final live runs below exercise exactly `463069b`, sequentially with
`qwen3-30b-a3b:latest` through the existing Ollama adapter.

| Document | Acceptance | Delivered claims | Retained / supported-cited evidence | Native pages cited | Requests | Completion tokens | Model time | Test wall time |
| --- | --- | ---: | --- | --- | ---: | ---: | ---: | ---: |
| DOL deck | exit 101 | None | 83 / not verified | Not delivered | 180 | 8,337 | 107,912 ms | 110.27 s |
| NARA | exit 0 | 6 | 10 / 7 | 7/11 (63.64%) | 28 | 1,538 | 21,516 ms | 22.36 s |

### What the deck actually failed

Analysis completed with 83 evidence items. Eleven initial synthesis batches
returned 6, 7, 4, 8, 6, 6, 6, 6, 8, 8 and 3 candidates: 68 total, covering all
retained evidence. Four reductions would bring that catalog within B=64.
The first reduction and its repair, response ordinals 178 and 179, both return
candidate_ids=[c1] and the identical sentence about Labor Standards in Agriculture
ensuring fair treatment and working conditions. They use 31 and 32 completion
tokens respectively, against the unchanged 4,096 allowance. No verdict on the
faithfulness of that sentence was obtained; verification never ran.

The error is missing coverage, not a foreign or duplicated identifier. The
schema allows each member only from the supplied c1/c2 enum, and the existing
parser still requires both supplied candidates to be covered. Repair receives
the missing local c2 without exposing a durable identity. After exhaustion,
the terminal failure correctly names the durable missing candidate
`candidate-c2aeb18f61646ad90f148284d44c65ba1f8bdf08a6b5b4dd832afdaacefe6435`.
There is no completed synthesis artifact, supported-page measurement or DOL
independent-reopen proof. Do not append c2 to unrelated prose, drop the candidate,
lower coverage or add another retry to describe this as success.

### Identifier and payload proof

All final-run requests used Primary transport, with no JSON fallback or invalid
identifier response. NARA exercises all three new enums through real inference,
including verification; its full acceptance and independent reopen pass.
`summary.rs:2206`, `:2243` and `:2267` enumerate request-local e/c/k vocabularies.
`summary/identifiers.rs` restores exact durable identities before the existing
Rust parsers. Candidate prompts carry evidence_count, not evidence_ids.
Uniqueness and complete coverage remain Rust obligations, not enum guarantees.
The shared decoder already stripped uniqueItems before transport; the two
synthesis schemas no longer advertise the keyword either.

| Comparison with pre-ordinal `8ece146` | Before | Final ordinal run |
| --- | ---: | ---: |
| DOL initial synthesis batches | 11 | 11 |
| DOL initial synthesis user characters, total | 47,896 | 42,003 |
| DOL reduction requests reached | 7, foreign ID on seventh | 2, first pair plus failed repair |
| DOL total requests | 185 | 180 |
| NARA initial batches per synthesis pass | 2 | 2 |
| NARA reduction calls, both passes | 2 | 2 |
| NARA total requests | 28 | 28 |
| NARA completion tokens | 3,776 | 1,538 |
| NARA test wall time | 41.49 s | 22.36 s |

Thus payload overhead drops, but batch count does not. On the maximum-text
83-item production preflight, aggregate synthesis user characters fall from
92,683 to 86,790: 5,893 saved, with twelve batches still required. Original
6,059-character evidence-ID arithmetic describes the whole catalog, not one
request. Local IDs reset within batches. Maximum-text and escaping reservations
remain 62 and 204 calls respectively, under the unchanged 256 ceiling.
NARA's candidate user payloads total 918 characters versus 1,506 before; verifier
user payloads total 12,709 versus 15,195. Content can differ between runs, so these
totals are measurements, not controlled isolation of every byte or timing gain.
DOL stops earlier in consolidation than before; fewer calls and shorter wall
time are not a speedup on completed work. Seeds remain run-derived.

Final DOL stage cost: analysis 167 calls / 4,298 completion tokens / 70,991 ms;
synthesis 13 / 4,039 / 36,921 ms; verification not reached. Final NARA: analysis
20 / 399 / 8,616 ms; synthesis 6 / 949 / 9,728 ms; verification 2 / 190 / 3,172 ms.
NARA retains margin 3 above A=7, omits only the page-12 stamp, and loses three
pages through two withheld claims. Coverage and claim count are unchanged from
the pre-ordinal run, with existing durable warnings. The permanent-records /
destruction source-fidelity caution remains; model support is not human review.

### Verification and remaining gaps

Final local gates: `cargo test --offline --all-targets` passes 250 library tests
(four ignored), three acceptance tests (three external tests ignored), and three
release tests. Strict all-target/all-feature clippy with warnings denied and
fmt --check both exit 0. Tests cover exact enum vocabularies, wrong-kind and
malformed ordinals, shuffled durable mapping, mixed/duplicate/foreign references,
missing/duplicated verdicts, batch-local scope, candidate counts without durable
lists, and unchanged repair maps with durable terminal errors. Preflight and
dispatch measure the same local-ID serialization; historical reload and existing
negative tests still pass. One initial compile ownership error and five stale
fixture assumptions were corrected before these passing gates.

Initial live probes on `4b4c7ed` produced the same outcomes: DOL failed at c2
omission with 180 requests / 8,337 completion tokens / 118.80 s; NARA passed with
28 / 1,538 / 26.26 s. They were repeated after the terminal-error identity fix,
not retried until a favorable model result appeared. Logs for both checkpoints
are in `/tmp/doc-sum-ordinals.FAOByA/`: `dol.log`, `nara.log`, `dol-final.log`,
`nara-final.log`.

The requested identifier slice is implemented and exercised across all three
paths, but the live deck product goal remains blocked on actual consolidation
coverage. Any next behavioral remedy requires its own contract; do not weaken
the existing rejection. Focused live heading controls and separately sequenced
durable partial-analysis/individual-attempt auditing remain open. PR #30 stays
ready for review and unmerged.

## Previous result: retention reaches 83 deck items; consolidation blocks delivery

**NOT DONE: DOL again fails before delivering a summary.** The retention reserve
works at analysis, but the seventh hierarchical reduction returns a foreign
candidate identifier and fails closed before verification. NARA passes with
warnings. The larger retention plan therefore has not yet proved a user-visible
win on the deck; passing arithmetic and unit tests are not corpus closure.
Contracts `bf42810` and `5dfcd90` precede implementation `8ece146`. Both runs below
exercise that exact code with `qwen3-30b-a3b:latest`, sequentially through the
existing Ollama adapter, without changes to acceptance, B/K, support, unit veto,
prompts, output/context/timeout or resource ceilings.

| Document | Acceptance | Delivered claims | Retained / supported-cited evidence | Cited native pages | Requests | Completion tokens | Model time | Test wall time |
| --- | --- | ---: | --- | --- | ---: | ---: | ---: | ---: |
| DOL deck | exit 101 | None | 83 / not verified | Not delivered | 185 | 14,470 | 158,532 ms | 160.64 s |
| NARA | exit 0 | 6 | 10 / 7 | 7/11 (63.64%) | 28 | 3,776 | 40,684 ms | 41.49 s |

### Failure: hierarchical IDs still rely on exact string reproduction

The final deck response, ordinal 184, contains two distinct candidate IDs. One
is 74 characters; the other is only 58:
`candidate-dfdf4cd950e0d1fbfd02182ce3b376d95f9bed6792b987c8`.
Rust constructs every candidate ID as `candidate-` plus a full SHA-256 hex digest
(`summary.rs:4281-4311`), so the short value cannot be a supplied ID. This is
foreign membership, not a duplicate: the parser rejects it at
`summary.rs:3164-3173`. The captured response is complete JSON and uses 191 of
4,096 completion tokens; it does not demonstrate output-token exhaustion.

The controlling schema still accepts candidate references as arbitrary nonempty
strings (`summary.rs:2194-2230`); unlike analysis selection, there is no supplied-ID
enum. `request_candidate_claims` passes only the reference count into that schema
(`summary.rs:1380-1426`). The bounded repair handles only omitted references,
not foreign IDs (`summary/repair.rs:80-96`), so this fails immediately rather
than taking another attempt. The validator is correctly protecting provenance.
Do not silently repair/dedupe IDs or broaden retry admission as part of retention.

Analysis made 167 requests: 83 selections and 84 paraphrases, including one
394-character draft shortened to 276. It completed with 83 evidence items.
Eleven initial synthesis responses produced 74 candidates, citing all 83 items;
their counts were 8, 8, 5, 8, 8, 7, 6, 5, 8, 8 and 3. That leaves ten reductions
needed to reach B=64. Six pair reductions validated; the seventh failed. The
74 candidates are not a delivered summary or a completed synthesis artifact.
There are no supported claims, supported-page fraction, withheld/lost-page
measurements or independent-reopen proof for this run: verification never ran.

This is the predicted consolidation surface growing, but not yet evidence that
the model cannot combine the meanings. The observed stop is copying identity.
A next contract should make pair-reduction identity application-owned or
constrain scope-local references, preserving full evidence coverage and every
existing rejection boundary. That change is not implemented in this slice.

### Measured retention and cost

| Stage | DOL requests / completion tokens / model ms | NARA requests / completion tokens / model ms |
| --- | --- | --- |
| Analysis | 167 / 4,298 / 71,384 | 20 / 399 / 8,641 |
| Synthesis | 18 / 10,172 / 87,148 | 6 / 2,312 / 21,039 |
| Verification | Not reached | 2 / 1,065 / 11,004 |

All requests used Primary transport with no fallback. The deck's prior run used
163 requests and 275.66 seconds but reached both verification passes; the new
185-request run failed earlier in the pipeline. Its shorter wall time is **not**
a speedup on completed work. Fresh run-derived seeds and a cold model at the
start also preclude a controlled same-seed performance comparison.

For NARA, A=7, desired reserve=16, R=11, E=10 and actual retained margin=3.
All 11 native-text pages were inspected, with only page 12 omitted as a stamp.
Source exhaustion prevents full reserve. Both synthesis attempts cover all ten
items; the selected verification withholds two claims, losing three pages and
leaving seven. The existing bounded re-synthesis ran once. Delivered supported
evidence coverage is 70 percent, above the unchanged 60 percent gate. Full
acceptance, exact provenance and independent database reopen pass. The source-
level permanent-records/destruction fidelity caution remains: a model-supported
verdict is not independent human validation. Coverage stays at 63.64 percent
versus the prior run, while requests rise from 20 to 28 and wall time from
34.01 to 41.49 seconds. This is enforcement, not a measured coverage increase.

### Local proof and remaining scope

`cargo test --offline --all-targets`: 247 library tests pass (four ignored),
three acceptance tests pass (three external tests ignored), three release tests
pass. `cargo clippy --offline --all-targets --all-features -- -D warnings` and
`cargo fmt --check` both exit 0. An initial new-test compile error passed the
wrong artifact type to a preflight helper; corrected before the passing gates.

Production preflights reproduce twelve maximum-text synthesis batches and 62
reserved calls for 83 items; the escaping fixture yields 83 batches and 204
reserved calls, both below 256. The real verification path accepts a compact
16-reference claim and rejects 17. Synthetic withholding tests produce 83, 67,
66, 51 and 83 supported pages for zero loss, the one-claim reserve, insufficient
reserve, two disjoint losses and overlapping references respectively. Historical
page plans/identities through 7.1.0 remain readable; density/chunk independence,
deterministic omission/backfill, source exhaustion and no repeated page work are
covered. The capacity test rejects N=1,707 before any generation; full reserve
ends at N=1,680, not full coverage feasibility.

Logs: `/tmp/doc-sum-retention.wXKkse/dol.log` and `nara.log`. The code slice is
implemented, but live deck acceptance remains blocked. Focused live heading
controls and the separately sequenced durable partial-analysis/individual-attempt
audit also remain open. PR #30 stays ready for review and unmerged.

## Previous result: DOL delivers a summary but fails native-page coverage

**NOT DONE: DOL acceptance still fails, now on page coverage rather than
pipeline admission.** The pipeline delivers 56 supported claims citing 66/111
native-text pages (59.46%), below the unchanged 60 percent target. It completes
with warnings. NARA passes. Contract `98965a5` precedes implementation
`446fe88`; both live runs below exercised that exact implementation with
`qwen3-30b-a3b:latest` through the existing Ollama adapter.

| Document | Acceptance | Delivered claims | Retained / supported-cited evidence | Cited native pages | Requests | Completion tokens | Model time | Test time |
| --- | --- | ---: | --- | --- | ---: | ---: | ---: | ---: |
| DOL deck | exit 101 | 56 | 67 / 66 | 66/111 (59.46%) | 163 | 25,505 | 272,996 ms | 275.66 s |
| NARA | exit 0 | 7 | 8 / 7 | 7/11 (63.64%) | 20 | 3,053 | 33,141 ms | 34.01 s |

The requested admission fix is exercised live: DOL verification runs five
size-based batches in each of two passes. First-pass input sizes are
10,227, 10,167, 10,469, 10,226 and 8,569 characters (49,658 total); second-pass
sizes are 10,287, 9,735, 10,463, 10,222 and 8,575 (49,282 total). Every request
fits 10,752 and both plans fit the explicit 64-batch ceiling, despite exceeding
the removed 43,008 aggregate estimate. There is no per-request or claim-budget
relaxation. No new retries or model/context/output/timeout change.

Actual DOL cost: analysis used 135 requests / 3,471 completion tokens / 62,602 ms;
synthesis used 18 / 14,099 / 129,763 ms; verification used 10 / 7,935 / 80,631 ms.
The existing bounded re-synthesis ran once. The analysis word-target repair
again shortened a 394-character draft to 276. All requests used Primary
transport, with no fallback. Fresh run-derived seeds mean these are not
same-seed causal comparisons to earlier runs.

The remaining gap is visible without blaming another admission guard: analysis
inspected 67 pages, retained one item from each, and omitted none. The final
supported set cites 66 items (98.51 percent evidence coverage) and clears K=32
and B=64, but loses the one page needed for the raw page target. The first
verification pass withheld one claim as unsupported; the second withheld one
as ambiguous. The retained logs identify those claim IDs but do not reliably
bind them back to a source page: full request payloads were not printed, and
the test database is deleted on unwind. Do not infer a specific missing page
or that the two withheld IDs describe the same claim from response order alone.

Both persisted synthesis attempts cover all retained evidence; durable warnings
include `SEMANTIC_CLAIMS_WITHHELD` and `SUMMARY_COVERAGE_SHORTFALL`. DOL fails at
`office_acceptance.rs:821`, after these checks and before the independent-reopen
assertions. Therefore this run is evidence of a returned/persisted summary,
not a completed DOL reopen acceptance or independent human fidelity review.
NARA completes the full acceptance including reopen, with its coverage warning.
The earlier source-level fidelity caution remains.

Resource trade: removing the aggregate estimate expands maximum admitted total
work. At 64 batches of at most 10,752 characters, a pass is bounded by 688,128
input characters; two passes permit at most 128 logical verification calls.
This is not an unchanged aggregate envelope. Admission now uses actual batches,
with catalog matching and claim-budget checks shared by synthesis preflight,
synthesis reload and verification. Artifact formats/identities are unchanged.

Local gates on `446fe88`: `cargo test --offline --all-targets` passed 243 library
tests (four ignored), three acceptance tests (three external tests ignored),
and three release tests. Strict all-target/all-feature clippy with warnings
denied and formatting passed. The first clippy invocation found an unused
stored size field after aggregate removal; its storage is now test-only while
production still measures and validates every serialized batch.
Boundary tests execute a 64-batch plan, reject 65 claims before runtime health,
probe the batch-count guard at 0/1/63/64/65, distinguish size/count/mixed splits,
and reject oversized singleton and mixed valid/oversized catalogs before calls.

Logs: `/tmp/doc-sum-batch-admission.quGq4k/dol.log`, `nara.log`, `local-tests.log`.
The next coverage design must account for verification withholding after the
analysis target is reached; do not lower the target or force a supported verdict.
Successful DOL acceptance, focused live heading controls and the separately
sequenced durable partial-analysis/individual-attempt audit remain open.
PR #30 stays ready for review and unmerged.

## Previous result: word targeting clears DOL analysis; verification admission blocks delivery

**NOT DONE: DOL still has no delivered summary.** The new failure is not
paraphrase length: synthesis's verification preflight rejects 49,670 aggregate
input characters against the unchanged 43,008 allowance. No verifier request
ran. Contract `0f108e5` precedes implementation `05d046f`; both live runs below
exercised that exact code commit with Qwen through the existing Ollama adapter.

| Document | Result | Supported claims | Evidence | Cited native pages | Requests | Completion tokens | Model time | Test time |
| --- | --- | ---: | --- | --- | ---: | ---: | ---: | ---: |
| DOL deck | exit 101 | None; 60 unverified synthesis candidates | 67 completed analysis items; all referenced by synthesis candidates | No delivered summary | 144 | 10,535 | 132,455 ms | 134.97 s |
| NARA | exit 0 | 7 | 8 persisted; 7 cited by supported claims | 7/11 (63.64%) | 20 | 3,036 | 35,144 ms | 36.29 s |

The prompt-only experiment worked at the analysis boundary in this run. DOL's
earlier failing position (response 37) now returns 356 characters / 55 words.
A later response (109) still overshoots at 394 characters / 62 words, but the
single word-targeted retry (110) returns 276 characters / 42 words. The model
therefore performed a real shortening this time; no tolerance band was needed
to complete analysis. This is not a claim that word targeting guarantees length
compliance or that unverified paraphrases are factually faithful.

DOL analysis used 135 requests (67 selections, 68 paraphrases including one
repair), 3,471 completion tokens and 67,509 ms. Synthesis used nine requests,
7,064 completion tokens and 64,946 ms. Their candidate counts were
6, 6, 8, 8, 8, 5, 8, 8 and 3, totaling 60 claims referencing all 67 distinct
evidence IDs. No reduction was needed to reach B=64. The analysis artifact
completed before synthesis started; the rejected candidate catalog is not a
completed synthesis/verification artifact or a delivered summary.

The new blocker is explicit in `summary.rs:1880` and `:2007`: aggregate allowance
is min(64,000, V_request * ceil(B/16)), whereas actual greedy batches split by
both count and serialized size and repeat the system prompt. For B=64 that
allowance is 43,008. The real catalog requires 49,670, exceeding it by 6,662,
even though each individual request passed its own guard. The preflight fails
closed before verification, wrapped as `MODEL_CLAIMS_RESPONSE_INVALID` at
synthesis. This is the previously unit-tested aggregate refusal now encountered
on the live deck, not a context overflow or a paraphrase regression. Changing
the aggregate resource contract is a separate decision; this change does not
raise it, drop references, lower coverage, or silently skip verification.

NARA retains its durable `SUMMARY_COVERAGE_SHORTFALL` warning after the existing
bounded re-synthesis. Eight paraphrases needed no repair, with lengths
177, 107, 183, 147, 92, 151, 85 and 122. Supported evidence coverage is 87.5
percent. Reopen and provenance acceptance passed; the source-level fidelity
caution in the prior evaluation remains, despite the model's supported verdicts.
All requests on both documents used Primary transport; no schema fallback.
Fresh run-derived seeds prevent treating this as a controlled same-seed trial.

Local gates on `05d046f`: `cargo test --offline --all-targets` exited 0 with
242 library tests passed / four ignored, three acceptance tests passed / three
external tests ignored, and three release tests passed. Strict all-target,
all-feature clippy with warnings denied and `cargo fmt --check` both exited 0.
Tests exercise 61-word feedback, Unicode whitespace counting, short 56-word
admission versus a rejected 385-character one-word claim, draft/source binding,
unchanged character/decoder boundaries and historical version reload. Analysis
version is 7.1.0; the 384 Rust and 1,536 decoder bounds and all packing remain.
Logs: `/tmp/doc-sum-word-target.9G27eb/dol.log` and `nara.log`.

Do not add the conditional 512-character tolerance band on this evidence: the
bounded retry cleared the observed overshoot. Next work needs a contract for the
verification aggregate resource budget; successful DOL acceptance, live heading
controls and separately sequenced durable request/partial-analysis auditing
remain open. PR #30 stays ready but unmerged.

## Previous result: DOL still fails length repair; NARA passes

**NOT DONE: DOL still produces no summary.** On implementation `897d2e9`,
its nineteenth paraphrase returned 392 characters against Rust's 384 limit.
The single draft-aware shortening request returned the identical 392-character
sentence. Both are complete and punctuated; this is a length-only rejection,
not decoder truncation, malformed JSON, or output-token exhaustion. The earlier
333-character agriculture definition now passes, but the next observed failure
is not fixed. Do not raise another limit or claim corpus closure from this run.

Contract `573ad48` and the pre-implementation correction `869e458` precede code
`897d2e9`. Both full acceptances below ran that exact code commit, sequentially,
using `qwen3-30b-a3b:latest` through the existing Ollama adapter. No coverage,
eligibility, completeness, token/context/timeout, synthesis or verification
allowance was relaxed.

| Document | Result | Supported claims | Retained evidence | Cited native pages | Requests | Completion tokens | Model time | Test time |
| --- | --- | ---: | --- | --- | ---: | ---: | ---: | ---: |
| DOL deck | exit 101 | None | 18 transient accepted items; no completed analysis artifact | No summary | 39 | 1,016 | 16,128 ms | 18.01 s |
| NARA | exit 0 | 7 | 8 persisted; 7 cited by supported claims | 7/11 (63.64%) | 20 | 2,896 | 33,717 ms | 34.50 s |

DOL response ordinals 37 and 38 each used 72 completion tokens against 2,048.
The retry metric records `draft_characters:392`, so the actual live shortening
request did receive the rejected draft. Its request has more input than the
initial call (408 versus 248 prompt tokens); the response text is nevertheless
identical. There were 20 paraphrase calls, including exactly one repair. No
synthesis or verification ran. The 18-item count is reconstructed from the
successful response prefix and the fail-before-persistence control flow, not
a durable partial-analysis artifact. New run-derived seeds mean this is not a
same-seed causal benchmark against earlier runs.

NARA made eight paraphrase calls, with lengths 179, 107, 75, 147, 87, 151, 57
and 72; none needed repair. Both synthesis attempts covered all eight retained
items. Verification withheld the corrupted-text claim, and the delivered set
cites seven items (87.5 percent). The acceptance verified the durable
`SUMMARY_COVERAGE_SHORTFALL` warning and database-reopened summary/citations.
This is automated coverage/support acceptance, not independent factual review:
the accepted permanent-records/destruction wording still warrants source-level
fidelity review, and a passing same-model verifier does not settle that concern.

All requests used Primary transport, with no schema fallback. The 1,536
decoder schema therefore compiled both in the early controlled probe below
and through the actual application adapter on the corpus. Unit tests prove
that the retry sends the draft and accepts a valid scripted rewrite, not that
Qwen reliably shortens it; this live retry did not do so.

The production preflight fixtures now executed successfully: at E=59, L=192
versus 384 produces eight versus nine batches and reserves 16 versus 18 calls,
with no reductions. At E=67 the respective reservations are 24 versus 26 calls,
with a maximum of three reductions in both cases. Both remain below 256.
Escaping, individual verification overflow and aggregate overflow are also
tested through the real planners, including rejection before model inference.
These are maximum-text fixtures at the deck's evidence counts, not a live DOL
synthesis measurement: this run failed before synthesis.

Local gates: `cargo test --offline --all-targets` exited 0 with 241 library
tests passed and four ignored; three acceptance tests passed and three external
tests ignored; three release tests passed. Strict clippy
(`--offline --all-targets --all-features -- -D warnings`) and `cargo fmt --check`
exited 0. The initial suite exposed a mock synthesis response whose size grew
with the unrelated analysis cap; keeping that mock's original concise size
restored its intended persistence test path, without changing production
verification limits. Dedicated new tests cover maximum-size packing instead.

Logs: `/tmp/doc-sum-single-claim.Lea4n8/nara.log`, `dol.log`,
`local-tests-final.log`, and `decoder-probe.log`. Raw response tracing was
explicitly enabled for these public corpus documents; new routine paraphrase
metrics contain lengths/ordinals only, not source or draft text.

Remaining gaps: successful DOL acceptance, focused live heading controls, and
the separately sequenced durable partial-analysis/individual-attempt audit.
PR #30 stays ready for review, but must not merge as a completed product fix.

## Pre-implementation finding: nominal cap is not the effective synthesis bound

The following records the pre-implementation checkpoint; completed verification
and its remaining failures are reported above.

The claim that the 16,000-character constant currently admits over-context
synthesis requests is contradicted by the executable call path. In the
`573ad48` source tree, `summary.rs:1531` computes
min(16,000, 3*(8,192-4,096-512)) = 10,752; `summary.rs:1549` subtracts system
text and repair reserve before ordinary request admission. `summary/repair.rs:52`
also checks the full repaired system-plus-user request against that derived
bound before generating. Git history shows the context-derived helper already
landed in `253a6e4`, with the larger output allowances. This amendment does not
introduce that guard or lower an effective 16,000 limit to 10,752.

The nominal constant is easy to misread in isolation. Record that documentation
hazard, not an unproven runtime defect: no bypass was found in the current direct,
evidence-batch or reduction request paths. Earlier unexplained model behavior
cannot be attributed to a context overflow from the nominal constant alone.
The three-characters-per-token proxy is still not a tokenizer proof, and the
actual Ollama context configuration remains a separate evaluation concern.

Cost before the new claim limit lands: for 59 retained evidence items and B=64,
maximum-length, unescaped items require eight batches at L=192 versus nine at
L=384. Neither requires candidate reduction because 59 is below 64. The
sum-of-batch-maxima preflight reserves 16 versus 18 model calls including the
bounded request repair. At E=67, the planned target for 111 native-text pages,
the analogous envelopes are nine versus ten batches and three possible
reduction requests, reserving 24 versus 26 calls. The ceiling stays 256.
These are arithmetic envelopes, not execution of the Rust preflight yet; the
implementation must exercise both evidence counts through production planning
before live inference. Actual shorter text, escaping and model consolidation
can produce different batch/call counts.

Early decoder probe, before implementation: a direct OpenAI-compatible Ollama
request using qwen3-30b-a3b:latest, named strict JSON schema, maxLength 1,536,
temperature 0, seed 42, reasoning_effort none and max_tokens 2,048 returned
HTTP 200 with a complete claim and finish_reason stop. It used 68 prompt and
23 completion tokens in 367 ms, with no fallback attempt. This is a controlled
schema-compatibility probe, not a corpus run or proof of adapter wiring; the
implemented adapter and both corpus runs still require live verification.
Trace: `/tmp/doc-sum-single-claim.Lea4n8/decoder-probe.log`.

## Previous result: DOL still fails; NARA passes revised coverage acceptance

**NOT DONE: DOL still fails analysis after its bounded paraphrase retry.**
The contract amendment is `90a8b91`, followed by implementation `cb53dea`.
Both live runs below exercised that exact implementation commit using
`qwen3-30b-a3b:latest` through Ollama at port 11434. No unit veto, eligibility
threshold, source bytes, token allowance, context, timeout, evidence target or
native-page target changed. The decoder ceiling is now 768 characters while
Rust still accepts at most 192, with a mechanical sentence-ending check and
one feedback-bearing paraphrase retry. That predicate is not proof of semantic
completeness or factual fidelity.

| Document | Result | Supported claims | Retained evidence | Cited native pages | Requests | Completion tokens | Model time | Test time |
| --- | --- | ---: | --- | --- | ---: | ---: | ---: | ---: |
| DOL deck | exit 101 | None | 3 transient accepted items; no completed analysis artifact | No summary | 9 | 231 | 3,649 ms | 5.52 s |
| NARA | exit 0 | 7 | 8 persisted; 7 cited by supported claims | 7/11 (63.64%) | 20 | 2,909 | 37,264 ms | 38.07 s |

DOL responses 7 and 8 contain complete, terminally punctuated agriculture
definitions of 333 and 272 Unicode characters, respectively. The second is the
single feedback-bearing retry. Both exceed Rust's unchanged 192-character
limit and neither reaches the 768-character decoder ceiling. They consumed
76 and 63 completion tokens, respectively, against the unchanged 2,048-token
allowance. Rust rejected the second response with
`MODEL_EVIDENCE_RESPONSE_INVALID: Paraphrase failed bounded length or completion
repair`. There was no synthesis or verification. The wider decoder removed the
boundary-cut mechanism but did not make the model obey the requested length;
do not describe this as product closure or token-budget exhaustion.

NARA's two persisted synthesis attempts each cover all eight retained evidence
items. Its selected supported set cites seven (87.5 percent); the native-page
fraction separately exceeds the unchanged 60 percent target. The ambiguous
corrupted-text claim remains withheld on both verification attempts. The run
completed with `SUMMARY_COVERAGE_SHORTFALL`, and the acceptance test verified the
summary, citations and events after database reopen. Page 6 remained retained;
only the date-stamp page 12 was omitted. No paraphrase retry was needed; the
existing single verification-shortfall re-synthesis ran. This proves the revised
acceptance/warning behavior, not independent human validation of every claim.

Every request in both runs used Primary transport; there was no schema fallback.
The larger decoder schema therefore worked on this live runtime. Runs use new
run-derived seeds, so comparisons are not same-seed causal benchmarks.
Logs: `/tmp/doc-sum-completion-coverage.uubO1a/nara.log`, `dol.log`, and
`local-tests.log`; raw response tracing was explicitly enabled for public corpus
inputs. Counts and token usage come from captured request diagnostics.

Local gates on `cb53dea`: `cargo test --offline --all-targets` exited 0 with
236 library tests passed and four ignored, two acceptance tests passed and
three external/live tests ignored, and three release tests passed. Strict
clippy (`--offline --all-targets --all-features -- -D warnings`) and
`cargo fmt --check` exited 0. Boundary tests cover decoder projection, terminal
punctuation inside/outside quotes, cut-off words at 192 characters, whitespace,
over-limit retry, repeated failure with no persisted analysis, historical v5
reload, and synthesized versus supported coverage on both sides of 60 percent.

The broader materiality feature remains incomplete: durable partial-analysis and
individual repair-attempt audit, focused live heading controls, and successful
DOL acceptance are still open. Keep the PR ready for review but do not merge it
as a completed product fix. The pending full-feature contract is not folded away.

## Previous result: materiality implementation blocked; both Qwen corpus runs fail

**NOT DONE. Neither recorded live acceptance run passed. The numeric-unit
contract conflict is now resolved and local gates pass, but this is not product
closure. Do not merge this checkpoint.** The
approved dependency exception is committed separately as `4b5d8db`; partial code
is checkpointed in `c8098a9`. The DB/immutable request-attempt audit slice has not
started. The proposed contract section remains pending, not folded into current
behavior as though the whole feature were complete.

Approved fixture correction: contract `a3011eb` precedes test-only change
`f7594eb`. NARA page 6 is retained under the unchanged numeric-unit veto; a
unit-free synthetic noise fixture remains an omission positive. The actual
page-analysis path preserves its exact quote catalog and offers only supplied
quote IDs, never the model omission option. No production filter, threshold,
token allowance, validator, or model changed in this correction.

Local gates on `f7594eb`: `cargo test --all-targets --offline` exited 0 with
232 library tests passed and four ignored, one acceptance test passed and three
external/live tests ignored, and three release tests passed. Strict clippy
(`--all-targets --all-features --offline -- -D warnings`) and `cargo fmt --check`
also exited 0. Neither corpus was rerun for this contract/test-only correction;
the exploratory live results below remain failures, not final-head acceptance.

### Live observations, not acceptance evidence on the final checkpoint

These exploratory runs exercised page eligibility, separate quote selection and
quote-only paraphrase, backfill, and synthesis slack on the intermediate working
tree **before the later short-unit veto fix**. They are not live gates on
`c8098a9`. Both used `qwen3-30b-a3b:latest` through Ollama at port 11434, with
the corpus hashes listed below unchanged. Context remained 8,192, analysis
output 2,048, synthesis/verification output 4,096, and timeout 900 seconds.

| Document | Delivered claims | Evidence | Cited native pages | Requests | Completion tokens | Model time | Test time |
| --- | ---: | --- | --- | ---: | ---: | ---: | ---: |
| NARA | 6 | 8 persisted; 6 cited | 6/11 (54.55%) | 20 | 2,953 | 36,951 ms | 37.73 s |
| DOL deck | None | 59 transient accepted items; no complete analysis artifact | No summary | 120 | 2,545 | 43,774 ms | 45.73 s |

Both commands exited 101; all requests used Primary transport. NARA inspected
10 pages and omitted pages 6 and 12 with no generation calls. Its first and
second verification responses both marked the permanent-records/destruction
claim unsupported and the claim about incoherent source text ambiguous. The
existing verification-shortfall re-synthesis did not restore coverage. The
pipeline returned a warning-bearing summary, but acceptance rejected its missing
evidence references; its cited-page fraction also remains below the raw target.

DOL failed at response 119, the sixtieth paraphrase. Its claim_text was exactly
192 Unicode characters and ended `must hire any合格, `, including trailing
whitespace and an unfinished clause. The unchanged canonical-text validator
rejected it. That response consumed 47 completion tokens, not its 2,048-token
allowance. An enforced string length does not guarantee a complete or canonical
claim. No synthesis or synthesis-stage repair ran on this document.

Logs: `/tmp/doc-sum-materiality.ZByf7m/nara.log` and `dol.log`. These contain
explicitly opted-in public-corpus model responses. Metrics were extracted from
the captured per-request diagnostics, not estimated from elapsed wall time.

### Contract counterexample found during the cold audit, now resolved

The first numeric-content filter missed short units on punctuation-heavy pages.
A direct negative control with `5 L` at the tail failed: it was classified as
scan noise. The implementation now retains separated and attached short units,
including `5 L`, `5l`, `5 m`, `5m`, `4 g`, and `4g`; those controls pass.

However, the exact captured NARA page 6 fixture itself contains the token `5l`.
It therefore triggers the conservative unit veto. The earlier contract required
both numeric-content protection and omission of this page; its positive assertion
failed. The operator approved retaining the ambiguous page and making it a
negative control. Contract `a3011eb` records that decision without a threshold
change or corpus-specific exemption. Tests in `f7594eb` preserve the original
captured bytes, verify retention through candidate construction and the emitted
selection schema, and keep unit-free scan noise and the captured stamp as
omission positives. This does not establish that the original page contains an
actual measurement; it avoids discarding ambiguous content.

### Earlier c8098a9 checkpoint verification and remaining gaps

- `cargo test --all-targets --offline`: 229 library tests passed, one failed,
  four ignored; exit 101. The failure is the captured NARA noise assertion.
  Cargo stopped before the other targets.
- Separately, `cargo test --offline --test office_acceptance --test release_contract`:
  one acceptance test and three release tests passed; three live/external tests
  ignored; exit 0.
- Strict all-target/all-feature clippy and formatting check: exit 0.
- Cargo.lock changed only to add the direct dependency edge; no package version
  changed. Analysis artifacts use a new version and preserve historical readers.
- Durable successful-analysis omissions are present, but partial analysis
  outcomes and individual synthesis-repair attempts are not yet durably audited.
  DB integration, failure/cancellation/reopen audit tests, focused live heading
  negative controls, and successful final-head corpus acceptance remain open.

## Previous result: larger output allowances help progress, but both Muse runs fail

**No product closure: neither document delivered a summary.** Contract
`9996ed4` precedes implementation `253a6e4`. Analysis now allows 2,048 output
tokens, direct/hierarchical synthesis 4,096, and verification remains 4,096.
Input admission reserves output and framing within the unchanged 8,192-token
context assumption. All validators, page/evidence/claim floors, temperature,
reasoning request, and the 900-second request timeout remain unchanged.

The requested `Muse-Glimmer-30B-KQuant-17GB-Q4_K_M.gguf` ran directly in standalone
`llama-server` (installed build `c1d0e7a`), using its embedded ATEM template with
Jinja, one slot, context 8,192, and loopback port 11435. **No LM Studio runtime
was used.** This is the same GGUF/runtime configuration as the preceding Muse
probe, not a weights-only comparison with the earlier Qwen/Ollama runs. Each
acceptance run creates a new run-derived seed; these are individual observations,
not a same-seed reproducibility or causal-quality benchmark.

| Run after increase | Delivered claims | Validated evidence | Final cited-page fraction | Requests | Completion tokens | Prompt tokens | Model time | Test time |
| --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: |
| NARA | Not produced | 8, persisted | Unavailable: no summary | 12 | 16,996 | 16,571 | 454,859 ms | 455.60 s |
| DOL | Not produced | 1 transient; no complete artifact | Unavailable: no summary | 2 | 569 | 1,100 | 15,386 ms | 17.26 s |

Both test commands exited 101. All requests used Primary transport, which does
not establish schema enforcement. NARA completed all eight page analyses and
both synthesis attempts. One accepted analysis response used 1,139 output tokens,
above the old 1,024 allowance; re-synthesis used 2,116, above its old 2,048 limit.
The first verification supported six of eight claims and marked two ambiguous,
triggering the existing bounded retry. Final verification consumed all 4,096
allowed generation tokens and returned truncated JSON, producing
`MODEL_VERIFICATION_RESPONSE_INVALID`. Its reported prompt plus completion was
6,529 tokens; server logs show `truncated = 0` (no context truncation). The
observed exhaustion is output, not the full context. No final supported summary
or citation artifact was delivered. Increasing analysis/synthesis allowances did
not establish that verification's unchanged allowance is sufficient for Muse.

NARA completion usage by stage was 5,076 analysis, 3,814 synthesis and 8,106
verification. DOL's second response instead failed
`MODEL_EVIDENCE_RESPONSE_INVALID`: its claim was 196 characters against the
unchanged 192 limit, using only 261 completion tokens. The two quote IDs were
valid in their separate page-local requests; this was length, not duplicate-ID
failure. More generation space cannot enforce the missing runtime length bound.

For comparison, the preceding Muse run with 1,024/2,048/4,096 allowances failed
NARA during analysis: three requests, two transient validated items, 2,034
completion tokens and 55,007 ms model time. Its failing response used all 1,024
generation tokens without final text. DOL previously failed after two requests,
one transient validated item, 538 completion tokens and 14,451 ms model time;
a separate diagnostic found a 218-character claim. Neither baseline delivered
a summary. The additional spend after this increase is not a measured coverage
win, and no claim of successful Muse model-quality comparison is made.

The earlier runtime probe also matters: the installed Muse-specific handler in
`common/chat.cpp` enables grammar for tools but does not apply the JSON response
schema on this path. A strict enum negative probe returned the forbidden value.
The template defaults to high reasoning despite the adapter sending
`reasoning_effort: "none"`. Output allowances are therefore only one limitation;
runtime schema enforcement and model-specific reasoning control remain open.
No runtime patch, reasoning change or further context/output increase was made
to hide this result. The standalone server was stopped cleanly after both tests.

Local gates on the implementation passed: `cargo test --all-targets
--all-features --quiet` (220 library tests, four ignored; one deterministic
acceptance and three release tests), `cargo clippy --all-targets --all-features
-- -D warnings`, `cargo fmt --all --check`, and `git diff --check`. New coverage
checks request allowances, exact input boundaries including system text,
hierarchical partitioning and preserved historical v3 quotas. These gates do
not override the failed live acceptance.

Reproduction: set `DOC_SUM_MODEL_BASE_URL=http://127.0.0.1:11435/v1/`,
`DOC_SUM_MODEL_NAME=doc-sum-muse-glimmer`, `DOC_SUM_OFFICE_PDF` to each public
corpus path below and `DOC_SUM_OFFICE_TRACE_MODEL_RESPONSES=1`, then run
`cargo test --test office_acceptance
office_pdf_live_ollama_summary_has_exact_durable_evidence -- --ignored --exact
--nocapture` from `src-tauri`. Public diagnostic logs are retained locally in
`/tmp/doc-sum-output-budget.1lTgFh/`; the preceding Muse probe is in
`/tmp/doc-sum-muse-eval.MizXyU/`. Raw source/model text is not committed here.

## Latest result: both corpus acceptance gates still fail after per-page selection

**The product blocker remains open.** Contract `2c0bb13` precedes implementation
`75df3bd`. The new single-item page-local analysis succeeds on both public
documents, but NARA still delivers insufficient verified coverage and DOL fails
in synthesis before producing a summary. No validator, floor, timeout, endpoint,
or model was relaxed. PR #30 is ready for review, not draft, and is not merged.

| Run | Delivered claims | Validated evidence | Cited native-text pages | Requests | Completion tokens | Model request time | Test time |
| --- | ---: | ---: | --- | ---: | ---: | ---: | ---: |
| NARA first | Below floor 4; exact count not printed | 8 | Not printed before assertion | 12 | 3,112 | 36,751 ms | 37.56 s |
| NARA diagnostic rerun | 4 (budget 8, floor 4) | 8 | 4/11 (36.36%) | 12 | 3,054 | 31,701 ms | 32.51 s |
| DOL first | Not produced | 67 | Unavailable: no summary | 68 | 3,586 | 43,751 ms | 45.84 s |

All three test commands exited 101; all requests used Primary transport with no
schema fallback. All planned analysis responses passed the unchanged exactness,
192-character, identifier-membership and provenance checks, plus the new
single-item/page-local checks. No invalid or repeated quote ID reached an
accepted analysis. DOL's 67 evidence items follow from its 67 validated one-item
responses and completed analysis checkpoint before synthesis started; there is
no delivered citation artifact for that run.

NARA first passed analysis in eight requests using 360 completion tokens. It
completed synthesis and verification twice, then the live gate rejected the
delivered claim count. The test now prints delivered metrics before quality
assertions rather than hiding them when an assertion fails. A separate public
diagnostic rerun on the same production code delivered four supported claims,
citing four of eight validated evidence items and four of eleven native-text
pages. It failed the evidence-coverage assertion and also falls below the
60-percent page target. The durable warnings included `SEMANTIC_CLAIMS_WITHHELD`
and `SUMMARY_COVERAGE_SHORTFALL`. Both synthesis responses were identical to
each other within each run, as were both verdict responses; a different attempt
seed does not guarantee different output at temperature zero.

DOL completed all 67 analysis requests in 37,394 ms using 2,870 completion
tokens. Its first synthesis batch then failed `MODEL_CLAIMS_RESPONSE_INVALID`:
"Every supplied evidence ID must be cited by at least one synthesis claim."
No verification or re-synthesis ran. The analysis-stage uniqueness and page
coverage blockers are removed; synthesis coverage is a separate live blocker.
Deterministic positional quote selection was not activated: the page loop met
its evidence target on both documents, and positional selection does not itself
enforce synthesis references or semantic support.

The diagnostic also shows that structural validity is not semantic quality:
one bounded NARA claim ended mid-thought, and another described illegible source
content rather than a substantive document finding. Exact quotation/provenance
does not make every paraphrase useful or correct. No quality improvement is
claimed from analysis counts alone.

The selected model remained `qwen3-30b-a3b:latest`, with context 8,192 and the
unchanged 900-second timeout. Both input hashes match the corpus hashes below.
Local gates on this implementation passed: 219 library tests with four ignored,
one deterministic acceptance test, three release tests, strict clippy, and
formatting. Boundary tests cover one/zero/two response items, foreign/mixed IDs,
192/193 characters, page-only enums, tail-inclusive stopping, sparse/dense and
chunk-independent plans, empty unselected chunks, and historical v3 artifacts.
These local gates do not override the failed live acceptance.

## Latest result: the product blocker remains after the bounded-generation fix

Both corpus runs still fail before delivering a summary at implementation
commit `8160be8`, following contract commit `6cf34c4`. The code now preserves
`maxLength` up to and including 192 and explicitly instructs the model to keep
claims within that limit and use each quote ID at most once. All validators,
evidence floors, budgets, the model, and the endpoint are unchanged.

| First run after the change | Claims | Evidence artifact | Cited-page fraction | Requests | Completion tokens | Request time | Test time |
| --- | --- | --- | --- | ---: | ---: | ---: | ---: |
| NARA | Not produced | Not produced | Unavailable; 11 native-text pages | 1 | 125 | 6,510 ms | 7.09 s |
| DOL | Not produced | Not produced | Unavailable; 111 native-text pages | 3 | 995 | 9,840 ms total | 11.68 s |

Both commands exited 101. Every request used Primary transport. NARA passed
the claim-length and quote-ID checks but failed the distinct-page floor in its
first scope. DOL passed two scopes, then failed the unique-ID/bounded-text
check on the third. The complete analysis schema compiled successfully in
Ollama; no schema fallback occurred. No synthesis or verification request ran.
These measurements supersede any expectation that the requested prompt and
projection changes alone close the original complaint.

One diagnostic rerun per public document used the same code with response
tracing enabled. NARA again failed distinct-page coverage: its three different
quote IDs had claim lengths 71, 70, and 126. It used 116 completion tokens in
one request, 1,825 ms model time, and 2.41 s test time.
DOL again failed in its third scope. Its first two scopes each returned nine
items that passed validation (18 transient evidence items, not a persisted
analysis artifact). The third returned nine items but repeated both `q6` and
`q7`; all claim lengths were within 192. That diagnostic used 992 completion
tokens across three requests, 9,839 ms model time, and 11.70 s test time.
Several claims in its second scope reached the 192-character boundary and
ended mid-word; decoder length enforcement alone is not semantic quality proof.

The remaining gap is selection structure: an enum permits repeated members,
and distinct quote IDs can still cite the same page. Explicit prompt wording
did not reliably satisfy either requirement on this corpus. Closing the
product blocker needs a further contract for reliable distinct-page/quote
selection; this change does not silently deduplicate, lower a floor, or relax
the validator. The narrow fix was opened for review, not claimed as a successful
corpus closure, and issue #29 remains open.

Local gates passed: 216 library tests, one deterministic acceptance test,
three release tests, strict clippy, and formatting. The new projection test
failed before implementation and passed afterward; it covers 0, 191, 192,
193, 2,000, and 4,000. Existing negative evidence/provenance tests still pass.
Local success is separate from the failed live acceptance above.

## Corpus result: both large-document live runs fail

The live acceptance runs used the tree of merged `main` at
`ca828dadadedc31c78359915e8798ce41e27a2af` (tree
`9abce419f1ac7db214dd0ef688c7c609cd711393`), Ollama `0.24.0`, and
`qwen3-30b-a3b:latest` digest
`1eda56426671cdf365913097543c2253a73c57e35b12741306689968d7f70292`.
Both local PDFs matched the SHA-256 values in [the office corpus](OFFICE_ACCEPTANCE.md).
Each run used the existing 900-second request timeout without overrides.

| First live run | Claims / accepted evidence | Cited native pages | Requests | Prompt tokens | Total completion tokens | Request time | Test time |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: |
| NARA schedule | No artifact | No artifact; denominator 11 | 1 | 3,179 | 174 | 5,654 ms | 6.25 s |
| DOL deck | No artifact | No artifact; denominator 111 | 1 | 2,602 | 403 | 4,266 ms | 6.34 s |

Both commands exited 101. Every request used Primary transport with no schema
fallback and a 1,024-token output allowance. Both stopped during the first
analysis scope with `MODEL_EVIDENCE_RESPONSE_INVALID`: "Evidence items must
contain unique quote IDs and bounded claims". No analysis artifact, synthesis,
verification, or summary was produced. A supported-claim count or successful
cited-page fraction therefore cannot be reported; user-delivered coverage is
absent. These results do not establish that the original thin-summary complaint
is fixed.

The deterministic checkpoint test passed for both source files. NARA retains
12 pages, 11 normalized native-text blocks, 2 chunks, and visual-only page 9;
the DOL deck retains 111 pages and blocks, 3 chunks, and no visual-only pages.
The source hashes, durable artifacts, and ordered events survived reopening.
That checkpoint run also passed after the test-only cleanup change.

Reproduction from `src-tauri`, using the locally downloaded corpus paths:

```bash
DOC_SUM_OFFICE_PDF=/absolute/path/to/nara-scanned-records-schedule.pdf \
  cargo test --test office_acceptance \
  office_pdf_live_ollama_summary_has_exact_durable_evidence \
  -- --ignored --exact --nocapture
```

Repeat with `dol-workplace-poster.pdf` for the DOL deck. That historical local
filename refers to the 111-page training deck, not the minimum-wage poster.

## Diagnostic reruns and controlling code

One rerun per public document enabled `DOC_SUM_OFFICE_TRACE_MODEL_RESPONSES=1`.
These are separate attempts, not replacements for the first-run measurements:

| Diagnostic rerun | Requests | Prompt tokens | Completion tokens | Request time | Test time |
| --- | ---: | ---: | ---: | ---: | ---: |
| NARA | 1 | 3,188 | 182 | 2,555 ms | 3.14 s |
| DOL | 1 | 2,594 | 408 | 4,335 ms | 6.24 s |

Both exited 101 through the same validator. NARA returned distinct `q1`, `q5`,
and `q9` selections, but its third claim had 334 characters against the
192-character bound. DOL returned overlong claims and repeated `q2` twice.
Neither diagnostic response contained a foreign quote ID.

At the measured tree, `summary.rs::analysis_output_schema` supplies
`maxLength: 192`; `model.rs::decoder_compatible_schema` removes every
`maxLength` and `uniqueItems` recursively. The analysis system prompt asks for
concise text without stating the numerical character limit.
`summary.rs::parse_evidence_response` independently rejects overlong claims
and reused quote IDs. The enum only restricts membership, not uniqueness across
items. The later verification re-synthesis path cannot repair an analysis
failure. Runtime safety holds, but successful large-document coverage remains
blocked at analysis.

A follow-up fix should expose the bound to generation and prove that the actual
analysis schema compiles with its small string limits. Distinct-page selection
also needs proof on a complete scope; silently deduplicating items or relaxing
the evidence floor would not satisfy the contract. No production model adapter,
prompt, validator, parser, or budget was changed in this evaluation.

## Native endpoint capability and memory measurements

The [native chat API](https://docs.ollama.com/api/chat) accepts JSON Schema in
`format` and runtime `options`. Ollama documents per-request `num_ctx` in
[its FAQ](https://docs.ollama.com/faq#how-can-i-specify-the-context-window-size).
Its [OpenAI compatibility documentation](https://docs.ollama.com/api/openai-compatibility#setting-the-context-size)
states that context size cannot be set through that API; its documented
workaround creates a different model configuration.

Small local `/api/chat` probes used the same installed model, `stream: false`,
`think: false`, `options.temperature: 0`, `options.seed: 42`,
`options.num_predict: 96`, and `options.num_ctx` as shown below. The schema
required `quote_id` in `['q1', 'q2']` and non-empty `claim_text`. The user
message supplied two synthetic quotations and requested `q1`. Every response
was HTTP 200, valid JSON selecting `q1`, `done: true`, and `done_reason: stop`,
with 54 prompt tokens and 22 completion tokens. `/api/ps` confirmed each
requested context and reported model size equal to GPU-resident size.

| Requested / observed context | Ollama size_vram (bytes) | Total GPU used (MiB) | GPU free (MiB) | Request time (ms) | Model load time (ns) |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 8,192 | 18,957,908,096 | 19,026 | 5,090 | 442 | 54,847,209 |
| 16,384 | 19,750,631,552 | 19,764 | 4,353 | 5,672 | 5,257,460,405 |
| 32,768 | 21,432,547,456 | 21,375 | 2,741 | 8,128 | 7,717,632,672 |

The device is an NVIDIA RTX 3090 with 24,576 MiB. These are sequential
single-request residency observations, not long-input throughput benchmarks.
Most elapsed time after changing context was model loading. They do not prove
safe concurrency or memory availability while another application uses the GPU.

A final native probe restored `num_ctx: 8192` and added `maxLength: 192` to
the claim string. It also returned HTTP 200 and valid bounded JSON: 6,970 ms
including 6,570,430,437 ns loading; 18,957,908,096 bytes model GPU residency,
18,995 MiB total GPU usage, and 5,121 MiB free. This proves the small bound can
compile in this probe. It does not prove the full multi-item analysis schema
or the two failing corpus documents are fixed.

Model metadata reports 48 layers, 4 KV heads, and key/value head dimensions
of 128. Assuming an FP16 KV cache and one sequence, the payload estimate is
`2 * 48 * 4 * 128 * 2 = 98,304` bytes per token: 768 MiB at 8,192 tokens,
1,536 MiB at 16,384, and 3,072 MiB at 32,768. This excludes model weights,
compute buffers, allocation overhead, and concurrent sequences. It is an
estimate, not a measurement of the running cache type; the residency table is
the observed machine cost.

## Adapter decision

Moving to native chat is technically feasible and would let the application
explicitly request its declared context. The first adapter change should keep
8,192 as the default and retain the deadline, loopback/auth restrictions,
cancellation boundaries, output caps, and Rust validation. Larger contexts
should be an explicit, coherently budgeted follow-up; 16,384 left more memory
headroom than 32,768 in this experiment. Increasing context does not fix the
observed overlong and repeated evidence responses.

The adapter change would map `max_tokens` to `options.num_predict`, move the
seed/temperature into `options`, use `format` for the schema, and read
`message.content`. Native `prompt_eval_count` and `eval_count` map to existing
usage fields; `total_duration`, `load_duration`, `prompt_eval_duration`, and
`eval_duration` offer additional timing breakdowns. It must test failed/truncated
responses, usage absence, transport diagnostics, auth, health, and request
timeouts, and rerun the complete fixture and corpus before claiming adapter
equivalence. The installed model reports completion/tools capability, not
thinking; a probe accepted `think: false`, but that is not evidence for other
models. No model configuration or daemon setting was changed persistently.
