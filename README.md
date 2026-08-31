# Document Summarizer

A local-first Tauri desktop application that ingests native-text PDFs, preserves
page provenance through a durable processing pipeline, and produces summaries
with page-linked exact source excerpts using an Ollama-hosted model. The
application also provides the optional local Connect `document.summarize`
capability to installations with an active Connect entitlement while remaining
usable on its own.

## Local runtime

The supported default is:

```text
Ollama endpoint: http://127.0.0.1:11434/v1/
Model: qwen3-30b-a3b:latest
Context: managed by Ollama; the accepted local deployment uses 8192
```

Ollama and the model are managed outside the application. The desktop app checks
readiness and explains when either is unavailable; it does not start Ollama or
download model weights. Model-generation requests default to a 300-second
deadline so the selected model can complete bounded multi-stage document work
without requiring a hidden deployment override. Connection and health checks
retain their shorter fail-fast deadlines.

Existing deployment overrides remain available through
`DOC_SUM_MODEL_BASE_URL`, `DOC_SUM_MODEL_NAME`,
`DOC_SUM_MODEL_TIMEOUT_SECONDS`, and optional
`DOC_SUM_MODEL_API_TOKEN_FILE`. The endpoint admission rule remains exact
loopback HTTP only.

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

Issuer public keys are embedded at build time, never loaded from a runtime
environment variable. Set `LOCAL_CONNECT_ENTITLEMENT_KEYRING_FILE` to the
release key-ring JSON when producing an official Connect-enabled build. If no
key ring is supplied, the standalone build remains healthy but Connect fails
closed and advertises no capability. Private signing keys must never be placed
in this repository or application package.

## Development

Install JavaScript dependencies, make sure Ollama is running, and launch through
the Tauri command:

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
`3851b4c55901ef18470c63b92a99a8348e2f1459`.
