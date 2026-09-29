# Qwen3.5-9B production preset (#71 / #100)

## Root cause

The previous default selected the 30B Ollama profile. The registry had no exact
9B entry; GGUF descriptors assumed 27.3B/IQ2_M, and raw completion framing did
not close the thinking block. A server chat-template option does not change
raw-completion framing.

The operator subsequently requested 32,768 tokens for the 9B. Raising the server
allocation alone had left the coherent source-character cap in place. Main now
owns the shared coherent budget and source-catalog fixes from the separate
reviewed slices. This branch composes those changes and changes the 9B context
at the existing profile owner.

Qualification also exposed two recorder defects: the selected-profile recorder
lost concrete stage identities, and the office recorder inherited permissive
preflight instead of forwarding runtime admission. Both could make tests behave
differently from production. Their regressions reproduce the missing identity
and missing context rejection before the recorder fixes.

## Required change surface

- `model_settings.rs`: make `full-qwen35-9b-q4km-v1` the fresh-settings default.
  Pin Qwen3.5-9B Q4_K_M SHA-256
  `cd76ec205963b3b33350093e6904d9de16c4e666fd104e1f632d25c7f15f2a13`.
  Analysis and verification use the same profile and 32,768-token context.
  Model metadata and thinking policy belong to the registry entry. Preserve
  the existing checksum-pinned managed llama.cpp binary and libraries.
- `llama_cpp.rs`: append `<think>\n\n</think>\n\n` after the assistant marker for
  this profile. Tokenize trusted framing with special-token parsing, keeping
  source text untrusted. Preflight and generation use identical framed tokens.
  Include thinking policy in the runtime cache identity. Preserve old framing
  for the older profiles.
- Shared test-only live selection: register the explicitly supplied GGUF in
  private temporary settings and call the production default-profile factory.
  Keep that directory alive for the run. Explicit settings/candidate selection
  remains possible; never silently substitute the old 30B verifier.
- Office and selected Story/Contract recording wrappers forward preflight and
  stage identities. Existing office coverage, exact-quote, modal, persistence
  and reopen assertions remain unchanged. Record the exact recovered-page count.
- Existing automatic-purpose and selected-profile tests, the release acceptance
  script, README and office instructions use the production default selection.

## Profile routing and persisted settings

Automatic already maps agreements to Contract and narratives to Story.
Informational, mixed, other and unknown map to General. Distributed uncertain
samples get one expanded inspection; invalid responses fail and ask for explicit
selection. An explicit user selection bypasses Automatic and remains authoritative.
This slice qualifies those existing paths; it adds no new classifier or UI.

Saved selections remain unchanged. There is no settings-version bump or migration:
the operator confirmed no deployed settings need migration. Missing/wrong model,
runtime or checksum leaves the selection unavailable instead of substituting an
unselected model. The older explicit presets keep their existing contexts.

## Explicit non-scope

Shared-host extraction, downloads, installer work, runtime replacement, Windows
GGUF enablement, installed settings, summary prompts/schemas/validators, OCR,
acceptance-threshold changes, layout follow-ups and hardware-floor claims.
The broader cross-app checks remain a later slice. No new 30B inference is part
of this requalification; its authorized prior comparisons remain historical.

## Assumptions and blockers

The exact model and runtime must pass production checksum verification without
overrides. Live qualification requires the existing inference lock and an
exclusive GPU. Reuse the owned loaded server where possible. The direct runtime
is Linux-only; Windows compilation does not prove Windows model execution.

A passing fallback proves validated delivery and disclosure, not generated-summary
quality. Keep every failed or incomplete attempt visible. Model-quality failures
cannot be fixed by weakening acceptance gates in this slice.

## Verification plan

- Fail first when fresh settings report 8,192 instead of 32,768; verify catalog,
  analysis/verification snapshots and stage runtime use the profile value.
- Probe absent/zero/below/at/above context limits, and reject wrong digest,
  tokenizer family and backend independently. Preserve old selections.
- Reproduce office preflight rejection being swallowed, then prove both accepted
  and rejected requests pass through without generation. Preserve stage-identity
  rejection and untrusted-token framing checks.
- Run adjacent settings/runtime/routing, coherent-budget and office checks,
  format, strict clippy and frontend build. CI owns the broad platform suites.
- Freeze production source and record the private qualification wrapper patch.
  Reuse the existing unchanged office assertions for contracts A/B in General
  and Contract modes, the structured fixture and public DOL General fixture.
  Run the existing Automatic counterexamples and selected Story/Contract lanes.
- Capture source/model/runtime/binary identities, requests, raw responses,
  request-attempt diagnostics, delivered artifacts, warning/failure reasons and
  durable reopen checks. Verify actual 32,768 allocation and exclusivity. Report
  observed peak GPU allocation and server RSS with sampling limits.
- Inspect rendered outputs separately from mechanical pass/fail. The raw
  completion API has no separate reasoning-token counter; report observed
  response form rather than inventing that counter.

## Implementation summary and cold diff audit

The registry owns the 9B identity, default, metadata and 32k stage context.
The direct runtime owns thinking framing and cache identity. Test-only shared
selection uses the same factory as the app; both recorders preserve its relevant
identity/admission behavior. Documentation and the release script describe that
selection. No production summary behavior is added by this preset diff.

Fail-first context and preflight regressions reproduced their expected failures.
After the fixes: 25 settings checks, 39 runtime/routing checks, seven budget
checks and five office checks passed. Formatting, strict all-target/all-feature
clippy and the frontend build passed. These are local deterministic checks;
the opt-in live checks are separate. Earlier failed live and build attempts are
retained in private evidence, not erased or counted as successes.

## Gap audit

NOT DONE for merge until the current source's live qualification, semantic
review, required CI and current-head PR review are reconciled. Public receipts
use only opaque A/B labels and aggregates. No private source names or paths
belong in Git. Merge this PR by squash only because earlier branch history
contained a subsequently removed private document name.
