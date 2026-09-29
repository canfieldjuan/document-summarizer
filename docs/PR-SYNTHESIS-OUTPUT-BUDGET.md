# Synthesis response capacity (#95)

## Root cause

Coherent synthesis permits up to eight text units of 1,200 Unicode characters,
plus source identifiers and JSON, but every request grants only 2,048 output
tokens. The runtime can admit the input while the permitted response does not
fit. The saved B/General reproduction ends with `stop_type=limit`, partial JSON
and no delivery. This is independent of the incomplete sentences and missing
later-page coverage observed in that response. Those are not budget successes.

## Required change surface

- Add one coherent response-budget owner. Initial generation, source-selection
  admission and repairs use it. Keep the existing input-character safety cap;
  exact runtime preflight remains authoritative for the entire request.
- Preserve the existing unit-count and source-ID ceilings. Select an admitted
  text-character ceiling, up to 1,200, whose serialized response capacity fits.
  Count JSON structure and the longest permitted source-ID strings. For the
  qualified byte-level tokenizers, canonical JSON bytes bound token count;
  allow six bytes per text character, including JSON escapes, rather than
  assuming an average characters-per-token ratio. Reserve 256 output tokens
  for formatting and the completion boundary and 512 context tokens for repair
  feedback, in addition to runtime-owned framing. Checked arithmetic fails closed.
- Use bounded exact-preflight search over that character ceiling. The request
  schema and output allowance must come from the same decision. Validate the
  returned text lengths locally as well, including schema-fallback transports.
  Repair cannot silently reduce a previously admitted text ceiling: re-admit
  its actual prompt and current unit count or preserve the existing safe
  fallback/failure behavior. No additional generation or retry loop.
- Canonical JSON is the response-size accounting representation, not a promise
  that arbitrary whitespace or alternative escaping emitted on the wire is
  bounded. An output-limit stop still rejects the entire response, even if its
  prefix is valid JSON. No prefix salvage or truncation-as-success.
- Distinguish output-limit stops from input truncation and unknown stops in the
  direct llama.cpp adapter. Honor Ollama's final `done_reason` and reject absent
  or unknown completion reasons; preserve recorded usage on rejected answers.
- Update the canonical contract. Persisted content remains subject to existing
  completeness, grounding, semantic and coverage gates. Completed artifacts
  keep their existing validation and storage format.

## Explicit non-scope

No model or preset changes, installed settings, new runtime, new dependency,
public API or storage change, prompt wording/tuning, source catalog change,
clipping repair expansion, coverage policy change, analysis/selection/verifier
budget change, OCR change, or #100 push. This does not qualify generated prose.

## Assumptions and blockers

Qualified runtimes use the pinned byte-level tokenizers. Exact preflight includes
each backend's real framing and schema representation. The feedback reserve is
headroom, not a guarantee every correction fits; every repair is re-admitted.
Unsupported or failed runtime admission is never treated as spare capacity.
The ready-to-merge gate still requires independent review and CI. #100 stays
frozen; live comparison composes its profile privately without promoting it.

## Verification plan

1. Fail first on a schema-maximal response exceeding the old output allowance
   and on a llama.cpp limit stop reported as an undifferentiated runtime error.
2. Prove serialized response bounds with the byte tokenizer, including Unicode,
   escaped text, long source IDs, unit-count changes and formatting overhead.
   Probe zero/tiny/exhausted contexts, exact admission boundaries, non-context
   errors, repair readmission, local length rejection and no partial delivery.
3. Run adjacent synthesis and runtime tests, formatting and strict clippy. CI
   owns duplicate broad/platform suites. Do not count setup failures as regressions.
4. Replay private saved request identities and run real A/B in General, Contract
   and Automatic with the same isolated 9B/32k profile, locked exclusive GPU and
   unchanged acceptance gates. Keep every failed attempt and separate raw prose,
   verified prose, disclosed fallback and no-delivery outcomes. Use opaque labels
   and aggregate facts only in public evidence.
5. Cold diff audit, ready PR, exact-head review. No merge claim until those gates
   pass; #95 does not by itself qualify or merge #100.

## Implementation summary

Implemented after the contract-only commit `fbf50cc`, in `ccc93df`, with the
gateway capacity correction in `da63922`. One request-local owner now budgets
canonical response bytes, the exact runtime preflight and the decoder ceiling.
Initial admission reserves feedback headroom; repair consumes that headroom
without shrinking the earlier text ceiling. The existing semantic, sentence,
source, coverage and persistence checks remain authoritative.

Fail-first tests reproduced the schema-capacity mismatch, generic llama.cpp
limit diagnosis, and Ollama acceptance of length-stopped parseable JSON. The
synthesis suite passed 245 tests (10 live tests ignored), Ollama passed 33 and
llama.cpp passed 30 on the initial implementation. After the gateway correction,
its 38-test suite and both new capacity regressions passed. Formatting and strict
all-target/all-feature clippy passed on `da63922`. CI owns the broad platform
suites. The semantic-repair fixtures now explicitly have enough context to test
their existing 1,200-character shapes; real smaller contexts retain capacity
checks and are covered independently.

The isolated 9B/32k matrix on `ccc93df` composed with #100 source `a192521`
passed all eight unchanged delivery gates. Its later gateway-only correction
adds a rejection code direct runtimes do not emit; no exercised direct-runtime
budget/generation branch changed.

| Input | General | Contract | Automatic |
|---|---|---|---|
| A | Ledger fallback, 10/10 pages | Ledger fallback, 10/10 | Contract fallback, 10/10 |
| B | Ledger fallback, 14/14 pages | Ledger fallback, 14/14 | Contract fallback, 14/14 |

The public structured report kept coherent delivery covering 5/5 text pages;
the public regulatory document kept fallback covering 83/111. B/General
previously failed before delivery. Seven of the eight current results are
fallbacks. No real-contract generated prose is qualified: the raw A and B drafts
cite only pages 1-3 and 1-7, and existing content checks reject them. Contract
and Automatic reached their existing early catalog fallback without Contract
synthesis calls. Smaller decoder bounds do not cure incomplete sentences.

Source documents, model, runtime, temperature and framing are pinned. Production
seeds differ; B also has one changed analyzed source_claim, so this is not an
identical-prompt raw-model A/B. Offline replay on `da63922` separately reproduces
all four old/new provider prompt counts with the fingerprint-verified tokenizer.
Old and new requests choose the same bounds: A 441 characters / 22,035 output
tokens, B 217 / 11,347. Their schema-maximal canonical responses tokenize to
17,956 and 9,060 tokens. No model ran for that replay.

The live receipt verifies source integrity, actual 32k context and GPU exclusivity;
358 model responses were retained, with zero thinking markers. Result digest:
`232f32889718f419387c9dd8a87c20662aaf7b972cb34b37b18490382ffa0369`.
Offline replay digest:
`c8230c59675ca8c264d689204dc692c800c7c57172c242e6db1428d3d6f86ca1`.
Raw documents, prompts, responses and databases remain private. The failed
initial source composition ran no inference and is retained separately.

The additional selected Story/Contract and Automatic stage-test batch was
interrupted when a foreign LM Studio process acquired a GPU context. That
attempt remains invalid; these gates require a clean repeat before merge.
The PR records the repeat outcome, without replacing the interrupted receipt.

## Cold diff audit

- `summary/coherent/budget.rs`: checked response-size accounting, bounded runtime
  admission search, frozen repair ceiling and local length validation. Covered by
  escaped/Unicode/maximal-shape, context, transport-cap and repair boundary tests.
- `summary/coherent.rs`: full-catalog admission, source reduction and generation
  consume that owner; generation parses only within its admitted ceiling. Existing
  recovery/presentation gates remain. A production-request regression failed before
  the implementation and passes after it.
- `model.rs` and `llama_cpp.rs`: distinguish exhausted output, reject unqualified
  completion and retain diagnostic counts. Positive stop and negative limit tests
  cover parseable JSON as well as existing truncation/identity checks.
- `gateway_client.rs` and `gateway_runtime.rs`: share the existing output-cap
  constant and classify only over-cap preflight; wire validation stays unchanged.
- `summary.rs`: fixture-only changes keep catalog/verification tests focused on
  their intended seam and allow bounded multiple preflight calls without extra
  generation. No production logic changed in this file.
- `docs/CONTRACTS.md` and this contract record the new behavior and qualification
  limits. No preset, dependency, storage, runtime version or prompt wording moved.

boundary-probe: positive complete stops and fitting shapes pass; exhausted
contexts, over-cap requests, over-length response units and limit stops fail or
reduce before inference. Malformed schema and non-capacity errors remain fatal.
The actual generated request uses the admitted schema and allowance.

effect-trace: output capacity is controlled by `budget::admit`, consumed at the
three production call sites; the failing-before request regression and exact
saved-request token replay prove the changed allowance. Live B now delivers the
independently verified fallback. This does not prove better generated prose.

Concurrency: the budget is request-local and introduces no shared mutable state,
queue, lock, storage write or additional generation. Existing runtime admission,
request ownership and inference serialization remain unchanged.

## Gap audit

NOT DONE for merge at this checkpoint. Implementation, local deterministic
checks and the main live matrix are complete. The clean repeat of the interrupted
stage gates and exact-head CI/review still have to pass. #100 remains frozen;
this change is not a model promotion or generated-prose quality qualification.

## Contract revision: runtime output ceilings

The gateway client has a 4,096-token task output cap, independent of its 8,192
context. Its preflight reports an over-cap request as a protocol error, so
probing solely by context would introduce an avoidable failure. Keep the
existing preflight interface: the gateway runtime reports its wire-client output
cap as `MODEL_OUTPUT_BUDGET_EXCEEDED`, a capacity rejection the shared search
can reduce. Source the limit from the existing wire-client constant. Actual
wire validation remains unchanged. Probe requests also obey that cap, including
reserved feedback; generated requests exclude the reserve. This conservative
admission leaves some of the gateway allowance unused. Prove the capped search
and the gateway's exact boundary without inference. No new runtime API, gateway
protocol, wire cap, snapshot, or server change.

The direct runtimes perform tokenizer preflight. Gateway preflight checks its
existing wire bounds; this work cannot establish token-exact input admission
or the deployed gateway's private model identity. No such qualification is
claimed. The response-size bound and existing failure handling still apply.
