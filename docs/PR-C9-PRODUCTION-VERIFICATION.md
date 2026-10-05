# C9 production integration

## Contract

### Root cause

At base `e9b923df9ae216ac4c1eb29dcdbe7b229a5ca17d`, General coherent
verification transports governing clauses but `summary.rs::verification_request`
still requests a bare model verdict. `parse_verification_response` checks shape
and identities, not whether governing relationships were compared. The retained
F3 live results accept the wrong payment stage, dropped invoice condition, and
weakened working-day/cure condition. Blame attributes the bare verdict schema to
the original verifier, and verification 11 to this arc's F3 commit `af238682`.
C9's experimental request constrains exact source/claim passages and requires
stage, conditions, qualifiers and scope comparisons. Its approved controls have
passed, but the app does not consume this protocol yet.

### Required change surface

- One Rust comparison module owns bounded passage catalogs, the response schema,
  strict decoding and derived verdicts. Port the frozen C9 semantics. Reject
  changed/omitted relationships; withhold uncertain relationships. The model
  still judges relations; exact passages alone do not prove entailment.
- Current General coherent prose uses one claim per request, with complete
  source-owned contexts and request-local identities. Reuse a single plan in
  runtime admission and execution. Preserve cancellation and bounded request
  counts. Reject oversized inputs without shortening clauses; admission uses
  the existing disclosed claim-ledger fallback.
- Version new comparison verification separately. Keep completed historical
  artifacts readable and retain existing storage and citation shapes.
- Share existing entailment/context instructions; remove the bare-verdict
  output instruction from the comparison path instead of stacking instructions.
- Add production regressions, frozen public request/response parity fixtures,
  malformed/foreign-span/identity/budget boundary probes, multi-claim isolation,
  cancellation, saved-result compatibility, and admission/execution parity.
- Update the canonical verification contract in `docs/CONTRACTS.md`; other
  documentation links there for the current rule.

### Explicit non-scope

No model/preset/default changes, new thinking experiments, drafting changes,
Contract extraction changes, Story or Connect direct-ledger behavior changes,
OCR changes, UI redesign, public API changes, database migration, dependency
updates, new retries, keyword exceptions or tuning on unseen documents. Frozen
experiment code, original labels and scores remain unchanged. Existing semantic
guards remain a separate last defense; none is evidence of general fidelity.

### Assumptions/blockers

This work continues the held PR116 branch. Its fidelity hold and PR100's hold
remain. Four operator-approved controls justify integration, not release.
Source without a reconstructed larger clause uses its original exact quotation
as context, matching the existing source ownership boundary. Inputs outside C9's
fixed bounds are unsupported by this protocol and use the existing admission
fallback. No invented independent labels or unseen-batch tuning is allowed.

### Verification plan

1. Fail first: the production captured request must require comparisons and
   forbid a model-supplied verdict. Existing code should fail that assertion.
2. Replay approved C9 controls and retained public failed-case responses through
   the Rust planner/parser. Match schema, instructions, exact catalogs and
   derived verdicts; compare genuine raw model output, not invented judgments.
3. Exercise invalid and valid sides: unknown/duplicate/missing keys and IDs,
   partial/mixed comparisons, wrong-side/non-token spans, empty input, Unicode,
   relation shape, catalog/input/schema limits and complete-source fallback.
4. Exercise production callers for multi-claim isolation, cancellation,
   admission/execution identity, historical saved results and current reopen.
5. Run focused regressions and adjacent summary tests, formatting, strict
   Clippy and a cold diff audit. CI owns duplicated broad/platform suites.
6. Before release: actual production-runtime public controls, full-document app
   proof and locked unseen qualification. Retained Python results alone cannot
   clear that gate. Keep every attempt outside worktrees with owner-only access.

## Implementation summary

Implemented in `fa93c8ad14f4567c202287ce5d3c211cd56c7ceb`, with decoder
ordering corrected in `89b5b738e4519b6eebe51418ed210c9c1ee53c10`. This is
production Rust pipeline code on the held branch. It is not an installed-app
update or a model promotion. The canonical rules are in
[General prose comparison verification](CONTRACTS.md#general-prose-comparison-verification).

The General coherent verifier now prepares source-owned passage catalogs, sends
one claim per request, requires every comparison dimension, validates exact
passages and relation shapes, and derives its own verdict. The bare model verdict
instruction is removed from this path. Common entailment and context instructions
remain shared with the legacy verifier. Admission and execution consume the same
planner; oversized complete inputs select the existing fallback. Historical
artifacts remain readable. The native named schema allowance and gateway cap are
consumed before generation. No model/default settings or frozen experiments change.

### Reproduce, isolate, explain, fix, prove, prevent regression

1. **Bare-verdict origin.** The captured production request regression failed at
   the base because `summary.rs::verification_request` requested only a final
   verdict. `comparisons.rs` now owns the schema and strict derived result. That
   regression and recorded public-response parity pass. Frozen failed cases are
   replayed as protocol evidence, not independently relabeled or rescored.
2. **Empty passage catalog.** The new port classified an overlong single token as
   `MODEL_REQUEST_INVALID`. The isolated regression observed that value instead
   of `VERIFICATION_INPUT_TOO_LARGE`; the origin `span_catalog` classification
   now selects admission fallback and the regression passes. This was my new
   implementation defect, corrected before the first native run.
3. **Decoder field order.** The first native run rejected every approved control
   with `MODEL_VERIFICATION_RESPONSE_INVALID`. Captured prompts match the frozen
   experiment byte for byte. The schema's JSON meaning was equal, but sorted maps
   moved relations ahead of source spans and changed dimension order. The local
   qualified llama.cpp converter uses property iteration order to construct its
   generation sequence. A minimal serialized-order test failed before the fix.
   `model.rs::DecoderSchema` now preserves the originating required-field order
   through native, Ollama and gateway request serialization. The parser, labels,
   prompt, bounds and model did not change. Serialized-order, nested/mixed-key,
   legacy-order and actual request-envelope regressions pass. This was my port's
   second implementation defect; value-only schema parity was insufficient.

There is no new response repair, keyword exception or post-hoc rescan. Existing
semantic guards remain separate defenses; this change does not claim that they
prove general entailment. No existing special-case heuristic became redundant.
The removed behavior is the bare-verdict output instruction on the comparison
path, replaced at request construction by one comparison protocol owner.

## Contract revision: transport admission

Code inspection found native `model.rs::response_format` caps schemas at 64 KiB,
while the gateway owns a 250,000-byte task limit. Approved C9 controls exceed the
native cap. The comparison schema therefore needs its named, bounded 1 MiB
allowance in the shared model contract, with all other schema names retaining
their existing limit. Runtime admission reports any stricter transport cap; the
comparison planner consumes it before generation. The gateway's protocol limit
does not change. Required surface adds `contracts.rs`, `model.rs`, gateway limit
visibility and its runtime admission implementation. Test the exact limit, zero,
over-limit, wrong schema name, and planner consumption of the runtime cap.
No new model preset, runtime selection, gateway task version or dependency is
introduced. Model classification is still subject to the release fidelity hold.

## Contract revision: decoder property order

The first native control run at `fa93c8a` failed all four responses at strict
relation/span validation. The prompts match the frozen experiment byte for byte,
and schemas compare equal as JSON values, but Rust's sorted maps serialize the
comparison fields as claim, relation, source, and dimensions alphabetically.
The qualified llama.cpp converter builds its generation sequence from property
iteration order (`common/json-schema-to-grammar.cpp`, `_build_object_rule`).
This port introduced the protocol drift; value-only replay missed it.

Preserve C9's declared required-field order at the schema serialization boundary
used by native and gateway transports, without changing other schema protocols
or the global JSON map feature. Keep canonical schemas and parsing unchanged.
One shared serializer must consume the required arrays already produced at the
origin. Add a failing serialized-wire order regression, legacy-order and nested
schema probes, then re-run the same native controls once on the fixed source.
Retain the failed run; do not relax parsing, change prompts, or tune labels.

## Cold diff audit

The implementation diff from `e9b923d` was read directly. The following table
maps each changed source/test file to its contract and exercised proof. Paths
below are relative to `src-tauri/src/pipeline/` unless stated otherwise.

| File / owner | Actual change and reason | Proof |
|---|---|---|
| `summary/comparisons.rs:24,79,125,191,288` | General applicability, bounded catalogs, strict schema/parser, one-claim planning | C9 parity, invalid-response, caps, isolation, cancellation regressions |
| `summary/comparisons/tests.rs` | Public replay, strict boundaries, transport order, opt-in native controls | Focused C9 run and retained live attempts |
| `summary/comparisons/fixtures/c9-replay.json` | Retained raw model responses and canonical schema hashes; four operator labels only | Replay of all retained responses; original decisions unchanged |
| `summary.rs:55,1562,1938` | Version separation, shared execution, request selection, common instructions; scripted test runtimes understand new protocol | Adjacent summary suite; version/reopen and request regression |
| `summary/coherent/verification.rs:43` | One planner for admission and execution | Whole-source overflow fallback and production request tests |
| `summary/coherent/verification/tests.rs` | Add C9 admission/persistence proofs and adjust scripted runtime responses | Focused C9 and adjacent coherent tests |
| `summary/coherent.rs` | Reexport shared planner instead of separate prompt builder | Compilation and coherent caller tests |
| `summary/identifiers.rs` | Recognize retained verification-11 artifacts | Historical-read and summary tests |
| `summary/legacy_generation.rs` | Retain verification-11 routing in legacy compatibility selection | Adjacent summary tests |
| `contracts.rs:849` | Named schema bounds and runtime cap API | Native and gateway boundary probes |
| `model.rs:1404,1440,1478` | Named cap and ordered schema transport; existing schemas keep byte serialization | Exact bounds, nested/mixed order, legacy bytes, decoder projection, chat envelope |
| `llama_cpp.rs::CompletionRequest` | Carry ordered schema until the actual native request | Native envelope regression and live controls |
| `gateway_client.rs::Generation,request_core` | Share cap visibility and preserve ordered schema through envelope serialization | Gateway envelope and existing projection tests |
| `gateway_runtime.rs::response_schema_byte_limit` | Expose existing tighter task cap to admission | Gateway cap boundary test |
| `model_settings.rs` | Expose existing binary/library pins to the opt-in runtime test | Native identity admission; no pin or setting value change |
| `docs/CONTRACTS.md` | Canonical protocol and compatibility rules | Source comparison; no code rerun for documentation |
| `docs/PR-SUMMARY-CLAUSE-VERIFICATION.md` | Mark F3 record historical and link current rule | Diff read; original results retained |
| `docs/PR-C9-PRODUCTION-VERIFICATION.md` | Contract, revisions, evidence and open release gates | Diff and evidence audit |

**boundary-probe:** valid replay and malformed/missing/foreign/mixed comparisons;
both span sides; empty/Unicode/overlong input; exact size limits and zero runtime
limit; per-claim ownership; cancellation. All focused probes pass. Raw model
output is parsed against the prepared catalogs; it never supplies an admitted
final verdict directly.

**effect-trace:** General coherent admission and verification select the same
comparison plan; the model request carries its schema to runtime transport; Rust
derives supported/unsupported/ambiguous from validated relations. The captured
request regression, saved-result tests and native attempts exercise these
controlling points. Runtime schema ordering is verified at serialized request
boundaries, not by JSON-value equality alone.

### Verification record

- Adjacent summary suite: 348 passed, 16 ignored at the initial integration.
- Final focused comparison/transport suite: 17 passed, 1 opt-in live test ignored.
- Adjacent transport/projection checks: 5 passed.
- Strict all-target/all-feature Clippy, formatting and diff checks: passed.
- Native controls: initial attempt failed all four responses; retained unchanged.
  Corrected-order attempt at `89b5b738e4519b6eebe51418ed210c9c1ee53c10`
  passed all four: three supported, one unsupported, no parser failures.
  Driver exit 0, source hashes unchanged, GPU empty after runtime cleanup.
  Both attempts are retained; this is one fixed-source rerun, not retries until
  acceptance. Frozen system/user prompt bytes and schema values match in every
  case; only the decoder property order was restored.
- Test-harness compile errors (an early enum pattern and a missing enum import)
  and the wrong-directory formatting invocation were reported and corrected;
  they are not counted as product regression evidence.

The source suite is not repeated for documentation-only commits. The live driver
pins the exact code commit, hashes every tracked Rust-project file, holds the
exclusive inference lock, checks GPU availability and records cleanup. It uses
the existing native runtime with the frozen 9B candidate explicitly; installed
model defaults are untouched. It exercises production verification requests and
classification, not the complete import/draft/save/UI document path. Gateway
wire behavior is tested locally; live gateway-model qualification remains unproven.

### Durable evidence receipt

- Alias `c9-production-verification-summary`: SHA256
  `68d7091cc6c172cbf2c4ba674ad4ba4cc6c847740611cbb5df86caf6ccf191be`.
  This indexes the retained failing probes, passing logs, both native attempts,
  request/response bytes, model/runtime identities and cleanup receipts.
- Alias `c9-native-order-fixed-freeze`: SHA256
  `14520c6bb7f327c94a70889b4353049e8c9d82d901304f6e1955e32d1f5953f0`.
- Tested Rust-project Git tree: `5952523bf5a35030c8efa08178c8be935af66d5d`.
  Documentation-only publication reuses this source evidence with a no-source-diff
  check; it does not rerun model inference or change the frozen rules.

## Qualification follow-up contract (2026-10-05)

Root cause: `Prepared::parse` introduced in `fa93c8a` treats absence of changed
or uncertain relations as positive support. Four empty `not_applicable` entries
therefore produce supported without a single passage comparison. The frozen
experimental implementation has the same rule, but its retained results and
labels remain immutable. Correct the production aggregation at this owner:
without a validated preserved comparison, an otherwise non-negative response is
ambiguous. Changed/omitted remains unsupported; malformed shape remains invalid.

Required surface: the production parser, its minimal all-empty and mixed-dimension
regressions, and the canonical contract. No downstream filter, model prompt/schema
change or relabeling. Verify fail-before/pass-after, retained public response
parity, and the complete document path on a new source lock. This is a declared
production semantics correction relative to frozen C9. The old control receipts
remain evidence of their original source only; the release hold stays.

The review's qualification concern remains open. Its assertion that the original
invoice-period and temporary-equipment rows are approved faithful labels is
contradicted by the operator decision linked in the PR: unresolved and withhold,
respectively. Their legacy names and original scores are historical records.

### CI cancellation probe correction

CI run `37258719644` failed after cancellation plus next-root recovery took
1.001851714 seconds. The test introduced in main `62cc1ca` measures both operations
but asserts the combined duration is cancellation latency. Reproduce this false
failure by adding a valid slow status response for the second root after the first
socket closes. Correct only the test: explicitly require closure of the stalled
socket before its read timeout, then verify both roots' terminal/released state
under the existing recovery deadline. Retain the slow second-root case so this
separation cannot regress. No OCR runtime timing or cancellation rule changes.

### Complete-document app-worker proof

Use the existing A/B PDFs as known regression documents, not unseen qualification.
The installed settings select the loopback inference gateway. Copy those settings
unchanged into owner-private durable proof storage, initialize a fresh app database,
and call the ordinary `DesktopJobManager::new/start_pdf` path with General mode.
The opt-in test owns no alternate model or runtime implementation. Record each
stage and final persisted artifacts, then reopen through `workspace::get_persisted_summary`,
the same source-validation/presentation boundary used by the desktop command.
Require coherent output with comparison verification actually exercised; fallback
alone cannot pass the C9 app proof. Retain partial artifacts if a document fails.

This test proves the app worker and saved-view boundary. Visual UI interaction,
blind semantic review and the unseen batch remain separately reported gates.
No installed settings, existing database, model preset or source documents change.

## Contract revision: gateway protocol capability

The installed-gateway app proof at `3e16b95` failed both small synthetic
whole documents with `MODEL_GATEWAY_REJECTED` after drafting. The installed
server's schema validator forbids `$defs`/`$ref` and caps enums at 100 values.
C9 requires these definitions and larger exact-passage catalogs. My production
port exposed only its byte cap, so local admission incorrectly allowed a request
that the task protocol cannot represent. This is an integration defect I introduced.

Correct the runtime capability declaration at the gateway boundary. One named
schema capability must be consumed by both gateway request construction and the
comparison planner. General synthesis on an incompatible runtime must select the
existing claim-ledger path, with a distinct truthful verification-unavailable
warning before drafting. Retain legacy modes and completed artifacts. Do not
expand gateway limits, inline/truncate passage catalogs, switch the installed
model, retry with a weaker verifier, or count fallback as C9 qualification.
Remove the previous test's claim that serialized C9 order proves gateway support;
replace it with admission rejection and supported legacy controls. Keep byte caps
as independent limits. Test fail-before/pass-after at gateway admission, planner
capability consumption, saved fallback validation and live small-document fallback.
Native C9 semantics remain unchanged. Gateway C9 support is a separate protocol
and deployment task; no gateway repository or installed service is changed here.

## Gap audit

NOT DONE for release. Production integration and local structural regressions
are implemented. Full-document app proof, locked unseen fidelity qualification
and exact-head CI/review remain. PR116 and PR100 stay held; the existing fidelity
review thread is not resolved by these development controls. Do not merge or
promote a model on the strength of the public controls.
