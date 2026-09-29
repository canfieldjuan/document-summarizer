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
- Preserve the existing unit-count and source-ID ceilings. Select the largest
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

Contract only; implementation has not started.

## Cold diff audit

Contract and canonical requirements only. No runtime behavior changed.

## Gap audit

NOT DONE. Implementation, regressions and live qualification remain.

## Contract revision: runtime output ceilings

The gateway client has a 4,096-token task output cap, independent of its 8,192
context. Its preflight reports an over-cap request as a protocol error, so
probing solely by context would introduce an avoidable failure. Add a runtime
output-capacity method, defaulting to the effective stage context, with the
gateway override sourced from the existing wire-client cap. Stage dispatch and
test recorders must forward it. Capacity probes include feedback headroom in
context admission but must not count that headroom as generated output against
the transport cap. Prove smaller-cap forwarding and admission without invoking
inference. No gateway protocol, wire cap, snapshot, or server change.

The direct runtimes perform tokenizer preflight. Gateway preflight checks its
existing wire bounds; this work cannot establish token-exact input admission
or the deployed gateway's private model identity. No such qualification is
claimed. The response-size bound and existing failure handling still apply.
