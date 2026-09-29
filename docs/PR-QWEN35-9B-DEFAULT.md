# Qwen3.5-9B default preset (#71)

## Root cause

At 922e17e the default selects the 30B Ollama profile. The exact-digest registry
has no 9B entry. Registered GGUF descriptors always report 27.3B/IQ2_M, and the
raw completion prompt ends at the assistant marker without closing thinking.
The server's reasoning option does not apply a chat template to raw completion.

## Required change surface

- model_settings.rs: register Qwen3.5-9B Q4_K_M, SHA-256
  cd76ec205963b3b33350093e6904d9de16c4e666fd104e1f632d25c7f15f2a13,
  as full-qwen35-9b-q4km-v1 and make it the default. Both analysis and
  verification use that same profile. Use 32,768 context tokens for both stages and keep the
  checksum-pinned managed llama.cpp bundle. Profile data owns parameter size,
  quantization and thinking policy instead of generic GGUF assumptions.
- llama_cpp.rs: for this profile, append <think>\n\n</think>\n\n after the
  assistant marker, with special-token parsing only for trusted framing.
  The same framed tokens feed context preflight and generation. Include the
  policy in the runtime cache identity; preserve the older profile's framing.
- office_acceptance.rs: the default live lane registers the explicitly supplied
  9B GGUF in isolated settings and uses the production default-profile factory.
  An explicit settings path or named qualification candidate remains possible;
  never fall back silently to a 30B verifier.
- Live acceptance selection: extract the office helper into test-only shared
  support, used by the existing automatic-profile and selected Story/Contract
  lanes as well. Their prompts and assertions stay unchanged; recording accepts
  any ModelRuntime. Update profile-release-acceptance.sh to invoke the new
  production default rather than requiring a resident 30B Ollama model.
- README.md and docs/OFFICE_ACCEPTANCE.md: describe the new default and existing explicit legacy choices,
  including manual GGUF registration and the Linux-only direct runtime.
- Existing saved selections keep their values. No version bump or migration:
  the operator's issue update says there are no deployed settings to migrate.
  Missing settings use the new default. Missing/wrong model or runtime stays
  unavailable, never falls back to an unselected model.

## Explicit non-scope

No shared host extraction, downloads, installer, new runtime service, Windows
GGUF enablement, further summary prompt/schema/validator changes,
OCR changes, hardware-floor claim or rewriting installed settings. The old
Ollama profile remains selectable and its backend-specific default is retained.
The separate cross-app checks remain the next slice after this one.

## Assumptions and blockers

The exact model is present on the Dev Drive and its hash was verified. The
existing direct runtime remains Linux-only; Windows build checks do not prove
Windows model execution. Live work requires the existing inference lock and
an idle GPU. Model-quality failures must remain visible and cannot be fixed by
weakening acceptance gates in this slice.

## Verification plan

Fail-first default/profile admission and prompt-framing regressions. Test
correct/wrong digest, family and context, legacy selections, full-profile
analysis/verifier identity, cache-policy separation, and trusted framing versus
untrusted text. Run affected model-settings and llama.cpp tests, acceptance
unit checks, formatting, clippy, and frontend build. CI owns broad platform
suites. Run existing live office acceptance lanes on the production 9B profile
with source/model/runtime receipts and all failures retained. Establish the
live generated-output reasoning evidence explicitly; do not infer a server
reasoning-token counter if the raw API does not supply one.

## Contract revision: production 32k qualification

Root cause: the 9B registry entry still supplies 8,192 tokens to every stage.
The operator requested 32,768 on PR #100. Earlier runtime-only experiments did
not remove the application's coherent source cap. Main now owns the shared
coherent request budget, exact token admission and source-catalog repairs; this
branch incorporates those merged changes instead of copying their logic.

Required change surface: change only the 9B profile's safe context to 32,768 in
model_settings.rs. Prove the catalog, analysis/verification snapshot and runtime
stage descriptors carry that value. Test model contexts below, at and above the
new floor; independently retain wrong-digest, family and backend rejection. Keep
legacy profiles at their existing contexts. Update the preset documentation.

Profile routing already exists: Automatic maps agreement to Contract, narrative
to Story, and informational/mixed/other/unknown to General; distributed uncertain
samples receive one expanded pass. Explicit user selection bypasses suggestion.
Invalid suggestion output stops with an error. Qualification exercises that
existing path; it does not introduce another classifier or change the UI.

Verification plan: demonstrate the default-stage context regression fails at
8,192 before the fix, then run adjacent model/settings, runtime, profile routing
and coherent-budget checks, formatting and strict clippy. Use the existing live
acceptance tests and isolated production runtime with the pinned GGUF and binary,
thinking disabled, actual 32,768 context and exclusive GPU ownership. Run selected
Story/Contract, automatic-purpose counterexamples, persisted General, and real
contracts A/B in General and Contract modes. Capture full requests/responses,
source/runtime identities, observed peak GPU memory and process memory, fallback
and withholding reasons, and durable reopen results. Retain every failed attempt.
Existing 30B comparison receipts are historical evidence; no new 30B inference.

Non-scope: installed settings, runtime host/downloads, model promotion outside
this preset, Windows inference, profile-routing changes, acceptance thresholds,
summary redesign and the deferred layout follow-ups. A fallback passing exact
citation/persistence gates is not evidence of generated-summary quality.

Assumptions/blockers: GPU exclusivity is required; registration must verify the
existing model/runtime checksums without overrides. Passing mechanical gates is
separate from semantic review. Results that do not meet either requirement keep
this PR NOT DONE for merge; failures are not waived by changing the scorer.

## Implementation summary

The exact 9B entry now owns the default, model metadata and non-thinking
policy. The runtime cache includes that policy and trusted assistant framing
closes the thinking block. Acceptance uses isolated production settings and
keeps the private runtime directory alive for the entire run. Explicit saved
selections and the old Ollama profile are preserved.

## Cold diff audit

- model_settings.rs: default/registry/registered_descriptors/stage_runtime
  implement the profile contract; tests cover identity/context/runtime mismatch,
  unchanged legacy settings, correct model metadata and same-model verification.
- llama_cpp.rs: PromptFraming::load consumes the profile policy, and
  runtime_cache_key distinguishes it. The unchanged prompt_tokens owner supplies
  both preflight and generation. Existing untrusted-special-token checks pass.
- office_acceptance.rs: configured_live_runtime creates an isolated registered
  default profile, retains its temporary socket directory, and removes the
  implicit 30B verifier fallback. No acceptance criterion was changed.

Fail-first: both new default/framing tests failed at their expected assertions.
Final affected model/runtime tests: 54 passed (8.79s). Acceptance offline checks:
3 passed, 3 opt-in ignored. Frontend build, format, diff checks and all-targets/
all-features clippy passed. A legacy test's assumption that default meant Ollama
was corrected to explicitly select the old profile; its original invariant
is still tested. The pinned GGUF template's enable_thinking=false branch exactly
matches the new trusted suffix. Runtime binary and library hashes match the
existing registry. The first live attempt failed before inference because the test helper created
a group-writable settings directory under this shell umask. The helper now
creates it with owner-only permissions; production ancestry checks stay intact.
The failure receipt is retained. Live model acceptance remains pending.

## Gap audit

NOT DONE for merge: existing live acceptance has not passed. Real-document and
selected-profile runs remain pending on the shared inference lock, and CI and
current-head review are required. No minimum-hardware or all-documents quality
claim is made.

Shared live selection checks: 61 focused model/runtime/routing tests passed,
1 live test ignored (7.95s); 4 office checks passed, 3 live tests ignored.
All live binaries compile, and all-targets/all-features clippy passed (3.58s).
The second synthetic run executed 13 requests / 433 completion tokens, then
failed the unchanged omitted-page versus cited-page disjointness assertion.
No response contained a thinking marker; the raw completion API provides no
separate reasoning-token counter. This is not a successful acceptance run.

The synthetic run's 13 raw responses are all JSON objects, with zero generated
thinking markers (log SHA-256
93582e883fa04d09d67eb587951b4c76ca667e909ab05398c86cde95ae139003).
This proves the observed response form, not a separately measured reasoning
counter. Production model/framing code is unchanged from that tested revision
51d02e4; subsequent changes select it from the other existing acceptance lanes.
A real-contract attempt stopped at lock acquisition, before inference, while
another managed evaluation owned the resource. No overlap was forced.

The omitted/cited-page contradiction is tracked as
[document-summarizer#99](https://github.com/canfieldjuan/document-summarizer/issues/99).
Its existence is a record of this run, not independent corroboration. The preset
change does not resolve it or claim that the old model reproduced it.

Cold audit of the completed surface: the shared acceptance helper is test-only;
the three library lanes retain their prompts/assertions and use the production
factory. Office acceptance retains all coverage gates. The shell entrypoint no
longer asserts Ollama GPU residency; it documents external lock/hardware proof
instead. No new production path bypasses identity, context or completion checks.
Formatting and all-targets/all-features clippy passed after final test-helper
changes (2.69s); frontend sources and dependencies have not changed since the
successful build. Windows runtime execution remains outside this slice.

## Live recording-wrapper correction

The first selected Story/Contract runs on the composed source failed with
MODEL_RESPONSE_INVALID after receiving nonempty JSON. The coherent-test
RecordingRuntime forwards generic identities but inherits stage-specific
identity defaults. QwenProfileRuntime reports its profile name generically and
the selected concrete runtime/model for each stage; validation correctly rejects
the wrapper's mismatched expectation. This is a test-wrapper defect introduced
by broadening live selection to the production profile factory, not yet a model
quality finding. The office recording wrapper already forwards stage identities.

Required surface: coherent.rs test-only RecordingRuntime forwards both
runtime_id_for_stage and model_id_for_stage to its inner runtime. Add an offline
regression using distinct profile and stage identities, verify the generated
response through the existing production identity validator, and retain
rejection of genuinely mismatched identities. No prompt, production runtime,
validator, source selection or acceptance-threshold change. First reproduce
MODEL_RESPONSE_INVALID before forwarding; then rerun the affected tests and
both live selected-profile probes. Keep the failed live artifacts in the record.

The recording-wrapper regression failed with the expected identity error before
forwarding and passed afterward; adjacent coherent tests passed (40 passed,
6 live checks ignored), as did all-target/all-feature clippy. PR101's reviewed
page reconciliation is composed into this branch; its synthetic live replay
passes with 13 identical raw responses. The reviewer's nonblocking count ask is
covered by asserting the recovery warning's exact count in both the offline
production-stage regression and office acceptance, using final citation pages.

Live gap remains explicit: real contract A final presented citations cover 4/10 native
pages despite 9 retained analysis pages, so the unchanged 50 percent raw / 60
percent adjusted coverage gate failed. Both failed selected-profile attempts
remain recorded as test-wrapper failures pending a corrected live rerun. No
quality or coverage threshold is changed, and the production preset is not yet
qualified for merge by these results.

## Qualification recorder contract correction

Code inspection found that office_acceptance.rs RecordingRuntime forwards
inference and stage identity but inherits ModelRuntime's permissive preflight.
The selected-profile recorder already forwards preflight. The office wrapper
can therefore skip exact source-context admission before generation, changing
which source catalog is selected compared with production.

Required surface: forward preflight_request to the inner runtime in the office
recorder; add an offline regression proving both admission and rejection survive
the wrapper with no generation call. Keep the existing office assertions,
source selection, model behavior and production runtime unchanged. Reuse this
corrected recorder in the private A/B qualification export. Reproduce the
recorder swallowing MODEL_CONTEXT_EXCEEDED before forwarding; then rerun the
office checks and strict clippy. No other harness or scorer redesign.
