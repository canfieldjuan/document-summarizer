# Connect proof runtime boundary

## Why this slice exists

Email Watcher's real cross-process Connect proof supplies a deterministic loopback
model endpoint and model name to the Document Summarizer release provider. Since
the provider moved to persisted qualified-model selection, `ConnectProvider::start`
always calls `runtime_from_settings`; a fresh proof data directory therefore selects
the default product preset, cannot discover its qualified digest from the fixture,
and returns `MODEL_CONFIG_INVALID` before accepting the job.

### Problem-derived contract

The root cause is the absence of a separate, explicit runtime admission path for a
release-mode external proof process. The correct fix must let a deliberately built
proof binary construct a profiled loopback runtime from proof inputs, preserve the
runtime's immutable per-run identity, and retain execution-digest verification. It
must leave ordinary provider startup on persisted admitted settings and exact
qualified identities. It must not make a fixture model selectable in the desktop,
weaken product profile admission, contact a remote endpoint, or place proof behavior
in a normal production build.

## Scope

- Add an opt-in Cargo feature used only to build the external proof binary.
- Require an exact proof-mode value, explicit exact-loopback model URL, and canonical
  digest before selecting the proof runtime in a feature-enabled binary.
- Build the proof runtime through the existing loopback-only model client and its
  execution-digest check, with a conservative UTF-8 byte token bound and fixed
  context profile snapshot.
- Keep feature-disabled and feature-enabled-without-mode startup on
  `runtime_from_settings`.
- Add boundary tests for absent, exact, malformed, and feature-disabled proof mode,
  plus malformed model identity inputs.
- Add a dedicated release-mode proof build command and document the contract.
- Update Email Watcher's deterministic fixture in a paired PR so `/api/ps` reports
  the exact proof digest and the process supplies the explicit proof inputs.

## Mechanism

`ConnectProvider::start` resolves a runtime source once. Production compilation
always resolves to persisted settings. A binary compiled with
`connect-proof-runtime` resolves to the proof source only for the exact versioned
mode. The proof constructor validates its model name and lowercase 64-character
digest, requires an explicit exact-loopback URL, creates two `OllamaRuntime` stage
clients with conservative UTF-8 byte token admission and digest verification, and
emits one immutable versioned snapshot.

## Intentional

- Proof profiles are not added to the product qualification catalogue or settings
  schema.
- The proof binary remains the real Tauri release provider entrypoint; the feature
  changes only runtime construction after an explicitly versioned proof opt-in.
- The ordinary binary ignores proof variables because the proof constructor is not
  compiled into it.

## Deferred

- Publishing or installing a proof-enabled binary is outside this slice; the build
  is a local acceptance artifact only.
- Product model qualification, selection UI, and gateway-runtime work are unchanged.
- Email Watcher's separate configured-model mode remains on the persisted profile
  path; this slice restores only its deterministic fixture proof.
- Merged PR #59 also changed provider runtime selection and model settings. The
  reconciliation preserves its database-backed gateway/local selection in the
  persisted branch and keeps this slice's fixture route behind the proof-only gate.

## Verification

- `npm ci` and `npm run build` passed.
- `cargo fmt --all -- --check` passed in `src-tauri`.
- Focused runtime-selection, proof-profile, required-endpoint, and Unicode
  byte-counter tests passed.
- The runtime-selection test also passed in the default, proof-feature-disabled
  build, exercising the ordinary persisted-settings decision.
- `cargo test --locked --all-targets --all-features` passed in `src-tauri`: 488
  library tests passed with 13 environment-dependent tests ignored; the external
  acceptance and release-contract test binaries also passed their runnable tests.
- `cargo clippy --locked --all-targets --all-features -- -D warnings` passed in
  `src-tauri`.
- `npm run desktop:build:connect-proof` produced the release provider binary.
- Email Watcher `scripts/connect-local-proof.py` passed against that binary under
  software rendering with the current Connect contract fixtures; its result reported
  `proof_passed=true`, `job_status=completed`, and one interrupted provider submission.
- The paired Email Watcher gate passed Ruff, 1,162 Python tests with 16 skips, 18
  frontend tests, packaged-sidecar smoke, 43 isolated Rust tests, Clippy, formatting,
  and the no-bundle Tauri release build.

## Estimated diff size

The Document Summarizer diff slightly exceeds the 400-line review target because the
indivisible proof boundary spans compile-time admission, immutable runtime identity,
the conservative token counter, boundary tests, and the release build contract. The
paired Email Watcher proof update remains below 300 changed lines; it exceeds the
initial estimate because the current provider requires native health, streamed
generation, execution-identity endpoints, and the durable queue's
restart-reconciliation and inbox metadata contracts.
