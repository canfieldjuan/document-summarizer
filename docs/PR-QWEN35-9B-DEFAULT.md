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
  verification use that same profile. Keep the existing 8192 context and
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
GGUF enablement, context increase, summary prompt/schema/validator changes,
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

NOT DONE: live acceptance evidence and current-head review/CI remain required.

Shared live selection checks: 61 focused model/runtime/routing tests passed,
1 live test ignored (7.95s); 4 office checks passed, 3 live tests ignored.
All live binaries compile, and all-targets/all-features clippy passed (3.58s).
The second synthetic run executed 13 requests / 433 completion tokens, then
failed the unchanged omitted-page versus cited-page disjointness assertion.
No response contained a thinking marker; the raw completion API provides no
separate reasoning-token counter. This is not a successful acceptance run.
