# Document Summarizer

A local-first Tauri desktop application that ingests native-text PDFs, preserves
page provenance through a durable processing pipeline, and produces summaries
with an Ollama-hosted model. The application also provides the optional local
Connect `document.summarize` capability while remaining usable on its own.

## Local runtime

The supported default is:

```text
Ollama endpoint: http://127.0.0.1:11434/v1/
Model: qwen3-30b-a3b:latest
Context: managed by Ollama; the accepted local deployment uses 8192
```

Ollama and the model are managed outside the application. The desktop app checks
readiness and explains when either is unavailable; it does not start Ollama or
download model weights.

Existing deployment overrides remain available through
`DOC_SUM_MODEL_BASE_URL`, `DOC_SUM_MODEL_NAME`,
`DOC_SUM_MODEL_TIMEOUT_SECONDS`, and optional
`DOC_SUM_MODEL_API_TOKEN_FILE`. The endpoint admission rule remains exact
loopback HTTP only.

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

A raw `cargo build --release` is intentionally rejected because it can produce a
desktop executable that points at the development server instead of embedding
`dist`.

Rust tests and checks run from `src-tauri`:

```bash
cargo test
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```
