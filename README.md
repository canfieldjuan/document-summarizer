# Document Summarizer

A local-first Tauri desktop application that ingests native-text PDFs, preserves
page provenance through a durable processing pipeline, and produces summaries
with page-linked exact source excerpts using a qualified local Qwen model. The
application also provides the optional local Connect `document.summarize`
capability to installations with an active Connect entitlement while remaining
usable on its own.

## Inference runtime

The application can use the authenticated Local Inference Gateway for
`document.summary.step@1`. The desktop inference selector accepts an HTTPS
gateway origin plus an application-specific bearer-token file and issuing-CA
file. It stores only those file paths, verifies the credential-scoped task
before switching, and never asks the user to select the gateway's private
worker or model. Current gateway selection is enabled on Unix hosts; Windows
continues to use direct runtimes until owner/DACL validation lands.

Existing installations stay on their selected direct runtime. Direct Ollama
and qualified llama.cpp remain available as explicit migration fallbacks while
gateway deployment and release acceptance are completed.

## Direct runtime fallback

The retained default is:

```text
Ollama endpoint: http://127.0.0.1:11434/
Qualified preset: Qwen 3 30B-A3B
Effective context: 8192 tokens per native /api/chat request
```

An opt-in full preset is also qualified for the exact Jack Qwen 3.8 27B Coder
GGUF used by this project. Choose **Add GGUF file** in the desktop model card and
select that existing file. The app records its exact bytes and file identity; it
does not scan model folders, copy weights, use LM Studio, or import the file into
Ollama. On Linux it starts a private, authenticated llama.cpp child and sends
the existing pipeline prompts through direct `/completion` requests with a
pinned minimal Qwen template. The qualified `llama-server` and its llama/ggml
libraries must be the exact tested bundle in one directory, available on
`PATH` or through `DOC_SUM_LLAMA_SERVER_PATH`. Other GGUF or runtime hashes stay
visible but disabled until separately qualified.

Ollama remains managed outside the application. The desktop app checks
readiness, discovers Ollama models, combines them with explicit GGUF
registrations, and shows exact-digest qualified presets with runtime kind, size
and context. Installed but unqualified models remain visible with an unavailable
reason and cannot be selected. The app does not start Ollama or download model
weights. Runtime changes wait for active work to finish. An idle app-managed
direct child is stopped before Ollama use; switching to direct GGUF returns a
recoverable busy result while a qualified Ollama runner remains resident, then
can be retried after its bounded keep-alive expires. The app never unloads an
Ollama runner through a mutable alias. Model-generation requests retain the
900-second deadline; connection, health and direct-child startup keep shorter
fail-fast deadlines.

Direct-runtime deployment overrides remain available through `DOC_SUM_MODEL_BASE_URL`,
`DOC_SUM_MODEL_TIMEOUT_SECONDS`, and optional `DOC_SUM_MODEL_API_TOKEN_FILE`.
The desktop model choice is a persisted application setting, not
`DOC_SUM_MODEL_NAME`. The endpoint admission rule remains exact loopback HTTP
only. Each run durably snapshots its runtime kind, exact model digest, qualified
context and tokenizer version; continuation and retry never silently switch
profiles.

## Connect entitlement

Connect is a paid, failure-isolated capability. Document ingestion, processing,
saved results, and the rest of the standalone application do not require a
Connect entitlement. Connect manifest discovery, new jobs, and job-status API
access require a currently valid signed entitlement containing
`connect.capability_exchange`.

On Linux, the entitlement is read on every Connect request from
`$XDG_CONFIG_HOME/local-connect/entitlement-v1.json`, or from
`$HOME/.config/local-connect/entitlement-v1.json` when `XDG_CONFIG_HOME` is
unset or empty. The directory must be owned by the current user with mode `700`; the
regular, non-symlink entitlement file must be owned by that user with mode
`600`. Expiry is exact and has no hidden grace period. Replacing the file with a
new valid entitlement restores capability availability without restarting the
application.

The desktop header reports only the license state and whether Connect is active;
it does not expose license claims. **Activate** lets the user choose an acquired
license file. The Rust core reads bounded regular-file bytes without following a
final symlink, requires the same signature/feature/time checks as live Connect,
and writes only to the fixed shared path. Document Summarizer and other Connect
apps serialize installation through `.entitlement-v1.lock`; a synced mode-`600`
temporary file is atomically promoted and the installed file is re-evaluated
before success is reported. Invalid or inactive sources and failures before
promotion leave an existing license and the selected source unchanged. If a
post-promotion durability or final-validation step fails, activation restores
the prior license bytes, or removes the promoted candidate when no prior license
existed, before returning a structured install failure.

Issuer public keys are embedded at build time, never loaded from a runtime
environment variable. Set `LOCAL_CONNECT_ENTITLEMENT_KEYRING_FILE` to the
release key-ring JSON when producing an official Connect-enabled build. If no
key ring is supplied, the standalone build remains healthy but Connect fails
closed, advertises no capability, and does not admit license installation.
Private signing keys must never be placed in this repository or application
package.

## Development

Install JavaScript dependencies, make sure the runtime for the preset you want
to use is available, and launch through the Tauri command:

```bash
npm install
npm run desktop:dev
```

Build the frontend alone with:

```bash
npm run build
```

## Production build

Use one of the desktop build scripts so Tauri embeds the production frontend:

```bash
npm run desktop:build:no-bundle
npm run desktop:build
```

The Email Watcher cross-process acceptance uses a separate local-only release
build:

```bash
npm run desktop:build:connect-proof
```

That command compiles the `connect-proof-runtime` feature. The proof runtime is
selected only when the process also receives
`DOC_SUM_CONNECT_PROOF_MODE=local-fixture-v1`,
`DOC_SUM_MODEL_NAME=connect-proof-model`, and a lowercase 64-character
`DOC_SUM_CONNECT_PROOF_MODEL_DIGEST` reported by the fixture's `/api/ps`
endpoint. The ordinary production commands do not compile the proof runtime and
continue to require an admitted persisted model profile. The proof client keeps
the same exact-loopback URL admission, request bounds, and post-generation
execution-digest verification as the product runtime. Its proof-only UTF-8 byte
counter conservatively upper-bounds Qwen BPE tokens without making the fixture
impersonate a qualified tokenizer vocabulary.

On Linux, `desktop:build` produces the currently supported Debian package. The
base Tauri configuration remains cross-platform, while
`src-tauri/tauri.linux.conf.json` deliberately limits this host to the verified
`.deb` target. Other operating-system bundle formats are deferred until they can
be built and exercised on their target platforms.

A raw `cargo build --release` is intentionally rejected because it can produce a
desktop executable that points at the development server instead of embedding
`dist`.

Rust tests and checks run from `src-tauri`:

```bash
cargo test
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

Run the opt-in provider conformance check against the pinned canonical Connect
v2 corpus from the repository root:

```bash
CONNECT_CONTRACTS_DIR=/absolute/path/to/connect-contracts \
  cargo test --manifest-path src-tauri/Cargo.toml \
  connect::v2::tests::canonical_v2_provider_contract_fixtures -- --ignored --exact
```

The check reads fixtures from canonical Git revision
`4d46af25ef5112f76daf841c7622987f05d25142`; it does not trust or copy the
contracts checkout's working tree. Updating that pin requires an explicit
compatibility change.

Run the independent signed-entitlement conformance check against its separately
pinned canonical revision:

```bash
CONNECT_CONTRACTS_DIR=/absolute/path/to/connect-contracts \
  cargo test --manifest-path src-tauri/Cargo.toml \
  connect::entitlement::tests::canonical_entitlement_v1_fixtures \
  -- --ignored --exact
```

That check reads entitlement fixtures from canonical Git revision
`c5405935bd1354cf6a4c8539425a53dfd7f52949`, which also contains the accepted
activation contract.
