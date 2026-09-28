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

Pending implementation.

## Cold diff audit

Pending final diff and verification.

## Gap audit

NOT DONE: implementation, deterministic verification, live acceptance evidence,
and current-head review/CI remain required.
