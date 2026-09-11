# Build Ledger

This document tracks the vertical slices and architectural decisions for the Document Summarizer.

## Qwen 3.8 direct-GGUF runtime (2026-09-06)

**Status**: Implemented and live-qualified locally

- Added explicit GGUF registration and an opt-in full preset for the exact Jack
  Qwen 3.8 27B Coder file. The application uses an exact-manifest,
  descriptor-bound llama.cpp child directly; it does not use LM Studio or
  import the GGUF into Ollama.
- Preserved the Qwen 3 30B-A3B Ollama preset as the default and strongest
  verifier. No smaller candidate changes prompts, validators, output budgets,
  page-coverage thresholds or other shared product behavior.
- Added typed runtime identity to settings and immutable run snapshots, plus
  runtime-profile leases so a selection change cannot unload a model beneath
  active desktop or Connect work. Idle direct children are cleared before an
  Ollama runtime is constructed.
- Direct requests use a pinned minimal Qwen ChatML token sequence, exact
  `/tokenize` admission and `/completion` with one projected JSON schema. The
  embedded template and schema fallback are not admitted. Parent descriptors
  remain close-on-exec, file identity is rechecked after load and at stage
  health, and rejected responses retain any token usage the provider supplied.
  Parent lease breaks target the acquiring thread, lease and identity are
  revalidated immediately before spawn, and the shared inference queue observes
  cancellation before transport. A dedicated process-lifetime supervisor owns
  the entire direct-runtime startup critical section, so Linux parent-death
  signaling is anchored to a durable application thread rather than a
  transient blocking-pool requester. Runtime socket, credential and library
  staging now occurs beneath a verified owner-private root derived from the
  explicit model-settings directory, never ambient `TMPDIR`. Every canonical
  staging ancestor is root/effective-user owned, and child startup restores and
  unblocks `SIGTERM` before installing its parent-death notification.
- NARA delivered 8 supported claims from 8 evidence items across 8/11 raw pages
  in 21 requests and 653 completion tokens. DOL delivered 83/83 supported
  claims across 83/111 raw pages in 180 requests and 6,948 completion tokens.
  Both completed with warnings and zero schema-fallback attempts.
- The first read-lease-enabled direct start exited before readiness with an
  undetermined cause because that revision retained neither child stderr nor
  exit status. Subsequent starts passed after exit-status diagnostics were
  added. The final handoff no longer unloads qualified Ollama runners through a
  mutable alias; it returns recoverable busy until their keep-alive expires.

## Slice 0: Repository and Architecture Foundation
**Status**: Implemented

**Implemented Behavior**:
- Scaffolded Tauri 2 application structure (`vanilla-ts`).
- Established Rust core boundaries (`pipeline` module).
- Created SQLite database schema using `rusqlite` for `pipeline_runs` and `pipeline_events`.
- Created canonical domain contracts (`PipelineRun`, `PipelineEvent`, `PipelineState`, etc.).
- Defined pipeline state transition rules in `StateMachine`.
- Implemented deterministic pure and SQLite-backed state-transition tests.

**Acceptance Tests (Slice 0)**:
- [x] Create project structure.
- [x] Rust state transitions reject invalid transitions (Tested via `cargo test`).
- [x] Rust state transitions accept valid sequence `RECEIVED -> INGESTING -> INGESTED` (Tested via `cargo test`).
- [x] Concurrent modifications blocked (Tested via `cargo test`).

**Proof**:
Rust tests in `src-tauri/src/pipeline/mod.rs` pass and confirm the core state-machine logic. Database initialization executes valid schema creation.

**Known Limitations**:
- Automatic crash recovery/resume is not implemented.
- Work units are simplified and not yet persisted for Slice 0.

**Deferred Items**:
- PDF parsing (Slice 2).

**Architectural Decisions**:
- **Tauri 2**: Used for minimal desktop footprint.
- **SQLite + rusqlite**: Chosen for simple, explicit schema migrations and persistence without heavy ORM layers.
- **State Machine Isolation**: The core state machine logic is pure and independent of database calls, allowing purely logical unit tests.
- **Duplicate-file behavior (Slice 1)**: Every ingestion creates a new unique `document_id` and a new `PipelineRun`, even if the file content hash is identical. This avoids complex deduplication logic and keeps pipeline runs independent.

## Slice 1: File Ingestion
**Status**: Implemented

**Implemented Behavior**:
- **Validation Fix**: Ingestion validates PDF signature bytes (`%PDF-`) directly instead of blindly trusting `.pdf` extension.
- **Ingestion Failure State Behavior**: Proven by implementation and test that an invalid PDF rejects instantly. Because insertion occurs in an atomic SQLite transaction, no `PipelineRun` is created on failure, entirely preventing runs from being falsely stranded in `INGESTING`.
- **Database Reopen Proof**: A Rust test closes the SQLite connection, opens a new connection, and compares document identity, byte size, hash, source path, run state/version, and ordered event identity/history.
- **Tauri Boundary**: The frontend selects/displays and invokes a thin command adapter. The database path is wired to Tauri AppData. This is code/build evidence, not a claim that an interactive close/reopen smoke test was exercised.

**Acceptance Tests (Slice 1)**:
- [x] Valid `.pdf` + PDF signature → accepted
- [x] Missing file → rejected
- [x] Non-.pdf → rejected
- [x] `.pdf` containing non-PDF bytes → rejected
- [x] Ingestion failure state leaves no stranded run in `INGESTING`.
- [x] Persistence works across independent SQLite connection reopen.

**Proof**:
Rust ingestion tests and the frontend production build pass. No full desktop-process restart was recorded for Slice 1.

**Slice 1 Final Verdict**: DONE.

**Deferred Items**:
- Inspecting magic bytes for absolute file verification beyond the 5-byte header.
- Advanced drag-and-drop styling.

## Foundation Audit (2026-08-28)

**Contract violations found**:
- The only expected-version check was on a mutable in-memory run; the SQL update was unconditional and normal code could call it directly.
- Run creation emitted no `RECEIVED` history event, event order depended on timestamps, and SQLite allowed event update/deletion.
- Schema setup used unversioned `CREATE TABLE IF NOT EXISTS`; existing-schema upgrade behavior was undefined.
- Runtime database serialization, lock, and AppData setup paths contained panic/unwrap behavior.
- Documentation claimed automatic crash recovery and full Tauri smoke proof that the implementation/evidence did not provide.
- A pre-existing parser prototype reconstructed incomplete run data and was not valid Slice 2 proof.
- Tauri release selection was ambiguous and initially emitted the prototype `test_min_pdf` binary instead of the desktop application.
- The generic persisted-transition helper remained publicly callable and could have allowed a stable artifact-bearing state to be reached without its artifact.
- The transition primitive assigned the next in-memory state before detecting `state_version` exhaustion.
- Parsing reopened the persisted path without proving its current bytes still matched the ingested document identity.
- Cancellation documentation described orchestration and resume behavior that does not exist.

**Fixes applied**:
- Added one SQLite-backed expected-state + expected-version transition boundary with compare-and-set persistence and atomic event append.
- Added an ordered sequence-zero `RECEIVED` creation event, unique per-run event ordering, and database triggers that reject event update/deletion.
- Made document + run + ingestion history a single transaction and removed public arbitrary run/event mutation functions.
- Added explicit schema version 2 creation and deterministic legacy-schema migration, including preserved legacy events and a reconstructed creation event.
- Replaced expected runtime panics with structured storage/ingestion/command errors and fallible Tauri setup.
- Canonicalized source paths and derived both hash and byte size from the same read-only byte stream.
- Disabled automatic binary discovery, declared the desktop application as the only Cargo binary/default target, and left the legacy probe sources excluded from builds; the repeated Tauri release build emitted `target/release/tauri-appdoc_sum`.
- Kept the generic persisted-transition entry point test-only; production stable parsing transitions are exposed only through artifact-coupled storage operations.
- Compute the next state version before any in-memory mutation and reject version exhaustion without changing the run.
- Re-hash and re-size source bytes before parsing; a changed source persists `SOURCE_CONTENT_CHANGED` and no artifact.
- Restricted ingestion/parsing storage mutations to the pipeline module and corrected cancellation/recovery documentation to match implemented behavior.

**Tests added**:
- Duplicate identity/hash policy and source-unchanged proof.
- Invalid/stale durable transitions, two-connection stale caller, and state-version non-mutation.
- Maximum state-version rejection without any in-memory mutation.
- Injected ingestion/event persistence failures with rollback assertions.
- Ordered, content-free, append-only event history.
- Legacy migration, repeated initialization, and independent connection reopen.
- Same-size valid-PDF source replacement rejected against the original hash and size.

**Tests run**:
- Phase A gate `cargo test --all-targets`: 15 passed, 0 failed.
- Final combined `cargo test --all-targets`: 27 passed, 0 failed.
- `cargo fmt --check`: passed.
- `npm run build`: passed.
- `cargo clippy --all-targets -- -D warnings`: passed.
- `npm run tauri build -- --no-bundle`: passed and built the intended `tauri-appdoc_sum` release target.

**Persistence proof**:
- The automated ingestion proof is a SQLite connection close/reopen and compares the original durable identifiers and history rather than only checking row counts.
- After correcting the default binary, two bounded launches of the real release executable created and reopened the same AppData `summarizer.db`; schema version 2 and SQLite `quick_check` were verified. No file was selected during that smoke check.

**Architecture verdict**: **PASS for Slice 1 foundation.** TypeScript remains interaction-only; Tauri commands remain adapters; Rust domain/storage logic has no UI dependency; persistence has no PDF-parser or model-runtime coupling.

**Remaining deferred hardening**:
- Automatic recovery/resume for a process interrupted in an active state.
- Full interactive Tauri file-select/ingest/close/reopen smoke test. This audit exercised real process startup/reopen but not native-dialog selection.
- Broader file-signature/type support beyond the current PDF candidate check.

## Slice 2: Native PDF Parsing

**Status**: Implemented

**Parser contract**:
- `DocumentParser` accepts an `IngestedDocument` and returns the parser-neutral `ParsedDocument` or a structured `PipelineFailure`.
- `ParsedDocument` records document ID, parser ID/version, ordered pages, and warnings.
- Every `ParsedPage` preserves a one-based page number, text, warnings, and an explicit future visual-processing marker.

**Focused library investigation**:
- [`pdf-extract`](https://github.com/jrmuizel/pdf-extract) 0.12 provides native Rust page-separated text extraction and Unicode font/encoding handling while using `lopdf` underneath. It was already compatible with this Tauri build.
- [`lopdf`](https://github.com/J-F-Liu/lopdf) provides direct page-tree/structure access and active maintenance, but its generic text extraction is a lower-level fit than the existing extraction layer.
- [`pdfium-render`](https://github.com/ajrcarey/pdfium-render) provides mature page text access but requires packaging or locating a platform-specific Pdfium native library, expanding desktop deployment surface.

**Decision**: Keep `pdf-extract` behind `DocumentParser`. The adapter loads the
page tree itself and extracts each known page individually so an error cannot be
mistaken for end-of-document. It catches malformed-page panics from the library
boundary and converts them to structured failures. Encrypted input is rejected
deterministically; password handling is deferred.

**Implemented behavior**:
- `INGESTED -> PARSING` commits before parsing begins.
- Parsing first verifies that the reopened source bytes still match the ingested hash and byte size.
- Successful output is validated for document/parser identity, canonical page order, and empty-page routing markers.
- Parsed artifact + `PARSING -> PARSED` + event append commit atomically.
- Malformed, encrypted, missing-source, invalid-artifact, and extraction failures persist `FAILED` and never persist `PARSED`.
- A thin `parse_document` Tauri command exposes the Rust core without moving parsing logic into TypeScript.

**Tests run**:
- The final combined Rust suite passed 27 tests with no failures.
- Strict Clippy, the TypeScript/Vite production build, and the no-bundle Tauri release build passed.
- The final release executable was launched and stopped twice; the same AppData database reopened at schema version 2 with SQLite `quick_check = ok`. No interactive file selection or parse command was exercised in this process smoke.

**Page provenance and no-native-text proof**:
- Generated multi-page fixtures assert page numbers, order, page-specific text, and representative non-ASCII text.
- Empty and image-only pages remain in place with empty text, `NO_NATIVE_TEXT`, and `requires_visual_processing = true`; image-only documents can reach `PARSED` with warnings.

**Persistence proof**:
- An automated test closes the SQLite connection, reopens the database independently, and compares the complete parsed artifact and ordered event history. This is not labeled a full desktop-process restart.

**Deferred**:
- Semantic structure, OCR, vision, chunking, citations, models, and summarization.
- Password entry/decryption for encrypted PDFs.
- Automatic recovery for process interruption while `PARSING`.
- Parser resource caps and broader real-world corpus evaluation.

## Slice 3: Canonical Document Normalization

**Status**: Implemented

**Git baseline**:
- Initialized the repository before Slice 3 and committed the verified Slice 2 tree as `91dce3cb9039cea7f3e08ac7cbb3060d2f730821` (`chore: establish verified Slice 2 baseline`).

**Normalization contract**:
- `DocumentNormalizer` accepts parser-neutral `ParsedDocument` and returns `NormalizedDocument` or structured `PipelineFailure`.
- Normalization version `1.0.0` preserves page count, order, numbering, warnings, visual-routing markers, and exact substantive text.
- Each non-empty page becomes one conservative `Text` block. Empty pages remain present with no blocks.
- Every block has a deterministic identity and a page-local `NativeText` source span. No PDF-library type crosses this boundary.

**Cleaning rules**:
- CRLF and CR line endings become LF.
- NUL is removed and recorded with `REPRESENTATION_CLEANUP_APPLIED`; other control characters are preserved until corpus evidence proves broader cleanup safe.
- Tabs, repeated spaces, blank lines, Unicode, punctuation, dates, numbers, identifiers, names, email addresses, and negation remain unchanged.
- Repeated headers/footers are neither detected nor removed because current parser output has no reliable layout boundary for that decision.

**Validation and failure behavior**:
- Validation rejects mismatched document/version identity, changed page topology, lost warnings or visual markers, changed text, nondeterministic/duplicate block IDs, nonexistent source pages, cross-page spans, and unsupported section provenance.
- Semantically inconsistent persisted parser output transitions `NORMALIZING -> FAILED`; no normalized artifact or false `NORMALIZED` event is written.
- A fixture parser proves normalization does not depend on `pdf-extract` output types.

**Persistence and migration**:
- Explicit schema version 3 adds `normalized_documents` keyed by run with document association, normalization version, serialized artifact JSON, creation time, and SHA-256 integrity hash.
- Fresh databases create schema v3 atomically. Existing schema-v2 databases migrate additively without rewriting Slice 2 artifacts; legacy databases traverse the existing v2 migration before v3.
- Artifact insertion, state/version update, and immutable event append share one transaction. Retrieval verifies the artifact hash, stored identity/version metadata, and the run-to-document association.

**Tests/proof**:
- Focused normalization suite: 9 passed, 0 failed.
- Combined Rust suite after Slice 3 implementation: 37 passed, 0 failed.
- Tests cover factual fidelity, Unicode, conservative whitespace, determinism, multi-page provenance, empty-page preservation, invalid/mixed boundary inputs, lifecycle, atomic rollback, integrity and run-association tampering, independent reopen, stale two-connection CAS, and v2-to-v3 migration.
- `git diff --check`, `cargo fmt --check`, strict Clippy, the TypeScript/Vite production build, and the no-bundle Tauri release build passed.
- A bounded launch of the exact release executable migrated the existing AppData database from schema version 2 to 3 while SQLite `quick_check` remained `ok`; the normalized artifact columns were present afterward. No interactive normalization command was exercised in this process smoke.

**Known limitations and deferred cleanup**:
- Version `1.0.0` deliberately emits at most one block per non-empty page; paragraph, heading, list, table, and section interpretation belongs to Slice 4.
- No repeated header/footer annotation is attempted without reliable layout metadata.
- Automatic recovery for a process interrupted in `NORMALIZING` is not implemented.
- OCR, vision, chunking, models, summaries, citations, embeddings, RAG, and chat remain deferred.

## Slice 4: Structural Interpretation

**Status**: Implemented

**Structure contract**:
- `StructureInterpreter` accepts only canonical `NormalizedDocument` and returns parser-independent `StructuredDocument` or structured `PipelineFailure`.
- Structure version `1.0.0` uses an empty `Document` root with conservative `Section`, `Subsection`, and `Unstructured` descendants.
- Nodes reference normalized block IDs plus exact source spans. They contain no rewritten source-body text, and every block has one canonical owner.
- `StructurePage` preserves every page's number, warnings, and visual-routing marker, including pages with no native-text blocks.

**Deterministic detection rules**:
- Only the first non-empty line of a block is considered, using numeric forms such as `1`, `1. Introduction`, `1 Introduction`, `1) Introduction`, and `1.1 Purpose`.
- Heading lines are capped at 120 characters, titles at 12 words, hierarchy at six levels, and numbering components at `1..=999`; a title must start uppercase and not end like a sentence.
- Nested headings require their complete active parent prefix. Duplicate, orphaned, malformed, year-like, lowercase-sentence, all-caps-only, short-line, and other ambiguous signals remain ordinary content.
- Document-title, repeated-header/footer, table, list, clause, figure, footnote, appendix, and semantic-role inference are not implemented.

**Coverage, provenance, and persistence**:
- Validation compares canonical normalized block order with depth-first structural ownership; missing, duplicate, foreign, or reordered references fail, so successful coverage is exactly 100%.
- Section titles, levels, unique numbering, and subsection parent prefixes are re-derived from the owning normalized heading block; fabricated labels cannot pass validation.
- Each node's ordered source spans must exactly equal those of its referenced blocks. Page-local spans remain discrete rather than becoming a falsely continuous page range.
- Explicit schema version 4 adds `structured_documents` with run/document association, structure version, artifact JSON, SHA-256 integrity hash, and creation time.
- Fresh databases create through v4 atomically. Existing v3 databases migrate additively without rewriting normalized artifacts; older databases traverse the existing migrations in order.
- Artifact insertion, state/version update, and immutable event append commit together. Retrieval verifies the hash, stored metadata, and run-to-document association.

**Focused proof completed**:
- The focused structural suite passed 16 tests covering simple and nested numbering, plain/ambiguous text, multi-page continuity, visual pages, deterministic output, immutable normalized input, exact coverage/provenance, malformed inputs/outputs, invalid completion-state bypass, atomic rollback, integrity/association tampering, independent reopen, stale two-connection CAS, and the real PDF path.
- Both migration tests passed, including v3-to-v4 preservation of the exact normalized artifact and deterministic reopen.
- A committed six-page realistic PDF was rendered and visually inspected. The real pipeline classified the unnumbered cover as `Unstructured`, detected `Introduction`, retained its continuation across pages 2-3, nested `Purpose`, detected `Findings`, and retained the visual-only page 5 through page metadata. The unnumbered `Operating context` line was intentionally not promoted and remained Section 1 content; no document title was inferred.
- The final combined Rust suite passed 54 tests with no failures. `cargo fmt --check`, strict Clippy, the TypeScript/Vite production build, and the no-bundle Tauri release build passed.
- Bounded launches of the exact release executable migrated the existing AppData database from schema version 3 to 4 and then reopened it at version 4. Direct SQLite verification returned `quick_check = ok`, one `structured_documents` table, and all three structured-artifact columns. No interactive structure command was exercised in this process smoke.

**Known limitations and deferred work**:
- Current normalization emits at most one block per non-empty page, so a page containing multiple headings cannot receive finer ownership until a later normalization version provides reliable finer-grained blocks.
- Detection deliberately misses unnumbered, lowercase, long, deeply nested, or orphaned headings and does not infer a document title from appearance or filename.
- Repeated header/footer classification remains deferred because current one-block-per-page input lacks reliable layout boundaries.
- Automatic recovery for a process interrupted in `STRUCTURING` is not implemented.
- Semantic chunking, tokenization, models, summaries, evidence extraction, verification, citations, OCR, vision, embeddings, RAG, and chat remain deferred.

## Slice 5: Deterministic Structure-Aware Chunking

**Status**: Implemented

**Contract**:
- `DocumentChunker` consumes only canonical normalized and structured artifacts
  and emits `ChunkedDocument`; it has no parser, PDF-library, UI, or model
  dependency.
- Version `1.0.0` groups blocks within their top-level structural owner and
  targets 12,000 characters while splitting only at normalized-block
  boundaries.
- Every normalized block has one canonical chunk owner. Block IDs and exact
  source spans remain ordered and complete; the prompt-oriented joined text
  does not replace authoritative normalized content.
- Deterministic chunk IDs include document/version/order/owner/block/text
  inputs. Oversized individual blocks remain intact with a warning rather than
  being truncated or split without a finer source representation.

**Persistence and state proof**:
- Explicit schema version 5 adds `chunked_documents` with run/document
  association, chunking version, serialized artifact, integrity hash, and
  creation time.
- `STRUCTURED -> CHUNKING -> CHUNKED` uses the centralized expected-state and
  expected-version transition boundary.
- Artifact insertion, state/version update, and event append commit together.
  Injected artifact failure produces `FAILED`, no artifact, and no false
  `CHUNKED` event.
- Independent SQLite reopen returns the identical chunked artifact and
  `CHUNKED` state after integrity verification.

**Tests/proof**:
- The focused chunk suite passed 8 tests covering structural boundaries,
  provenance, deterministic output, exact coverage rejection, empty visual
  input, lifecycle/version/events, atomic rollback, invalid chunker output,
  and independent reopen.
- The combined Rust suite passed 62 tests with no failures after schema-v5
  migration expectations were updated.
- `cargo fmt --check`, strict Clippy, the TypeScript/Vite production build,
  `git diff --check`, and the no-bundle Tauri release build passed. The release
  build emitted `src-tauri/target/release/tauri-appdoc_sum`.

**Known limitations and deferred work**:
- The target is measured in Unicode characters, not model tokens. Tokenizer
  coupling remains deferred to the model-runtime stage.
- Current normalization provides at most one block per non-empty page, so a
  single oversized page remains one oversized chunk with an explicit warning.
- Model inference, summary synthesis, factual verification, citations, OCR,
  vision, embeddings, RAG, and chat remain deferred.

## Standalone Local Summary Checkpoint

**Status**: Implemented for the native-text standalone path

**Contract and implementation**:
- Added a replaceable `ModelRuntime` plus an OpenAI-compatible loopback-only
  adapter. The adapter rejects remote, HTTPS, credential-bearing, query-bearing,
  and redirecting endpoints and uses this application's optional token file.
- Added durable `AnalyzedDocument`, `SynthesizedDocument`, `VerifiedDocument`,
  and `SummaryArtifact` contracts. Ordered chunk identity and source spans are
  preserved through analysis; synthesis and verification require complete,
  ordered source-chunk coverage.
- Added a Rust application service that executes the existing real PDF path
  through completion. The Tauri command remains an adapter and the TypeScript
  frontend renders only returned summary text.
- Mechanical verification explicitly records `SEMANTIC_VERIFICATION_DEFERRED`;
  no factual-verification or citation claim is made.

**Persistence and failure behavior**:
- Explicit schema versions 6 through 9 add separate artifact tables without
  rewriting the schema-v5 chunk checkpoint.
- Each checkpoint artifact, state/version update, and immutable event append is
  atomic. The final artifact has both a persisted-row hash and an internal
  content integrity hash.
- Model/runtime, malformed-chunk, size-limit, validation, and ordinary database
  failures persist `FAILED` without a false next-stage or completion event.
- Visual-only documents fail truthfully with `NO_NATIVE_TEXT_FOR_SUMMARY`; OCR
  and vision remain deferred.

**Focused proof completed before the full-suite gate**:
- Eight summary tests passed, covering lifecycle/events, all durable artifacts,
  runtime failure, malformed input, stale CAS, final-write rollback, tamper
  detection, visual-only behavior, and independent database reopen.
- One application-service test passed through the real PDF parser,
  normalization, structure, chunking, fake runtime, and durable summary.
- One schema-v5 migration test passed, preserving the exact chunk artifact,
  creating all four later tables, and reopening deterministically at the
  current schema.
- After correcting the IPv6 loopback host representation exposed by the first
  full run, the combined Rust suite passed 75 tests with no failures. The model
  boundary suite includes positive IPv4/IPv6 loopback cases and negative HTTPS,
  hostname, adjacent non-loopback IP, URL-credential, query, empty-model, and
  zero-timeout cases, plus bounded-response acceptance/rejection.
- `cargo fmt --check`, strict Clippy, the TypeScript/Vite production build, and
  the no-bundle Tauri release build passed. The release build emitted
  `src-tauri/target/release/tauri-appdoc_sum`.

**Known limitations and deferred work**:
- The live OpenAI-compatible endpoint has not yet been exercised in this
  checkpoint; tests use the replaceable fake runtime.
- One-pass synthesis input and each source chunk are capped at 100,000 Unicode
  characters. Hierarchical synthesis is deferred.
- Semantic fact verification, citations, OCR, vision, embeddings, RAG, chat,
  and workflow automation remain deferred.

## Connect v1 Document Summarizer Provider

**Status**: Implemented and green at the automated provider gate

**Contract authority**:
- The separate local `connect-contracts` repository froze the v1 manifests,
  registration, job request/status, error schemas, positive fixtures, and
  negative protocol/path/remote-endpoint/size/completion fixtures in commit
  `07fa0c4`.
- The provider advertises capability ID `document.summarize`, capability
  version `1.0`, PDF input, and versioned plain-JSON summary output. It does not
  expose a private document ID, run ID, database path, or source path.

**Provider implementation**:
- Tauri starts an optional ephemeral-loopback HTTP provider. An unavailable
  `XDG_RUNTIME_DIR` or any Connect initialization failure leaves the standalone
  app operational.
- Runtime registration is written atomically with owner-only permissions and a
  fresh instance/token. Every route authenticates the token, browser origins
  are rejected, redirects/proxies are irrelevant on the listening side, and
  request/artifact/output sizes are bounded.
- PDF bytes stream into owner-only staging, are checked against declared size
  and SHA-256, flushed, atomically promoted, and then ingested from
  provider-owned storage. Display names are validated but never used as paths.
- Schema v10 adds durable jobs. Accepted job mapping and the normal document/run
  ingestion transaction commit together; a partial unique index enforces one
  active Connect job. Same-request replay is idempotent and different input for
  the same job ID conflicts.
- The worker invokes the same parser-neutral standalone application service.
  Startup marks interrupted jobs retryably failed, and registration is removed
  on normal provider teardown.

**Automated proof**:
- Eight focused Connect tests passed. They cover request/path/version/size
  admission, stable request hashing, output caps, atomic accepted mapping,
  single-active enforcement, Connect-state CAS, restart failure conversion,
  authentication, browser-origin rejection, real multipart PDF streaming,
  provider-owned byte equality, the full real PDF pipeline, polling completion,
  idempotent replay, conflicting job IDs, malformed-PDF failure, digest
  mismatch cleanup, persistence through independently opened connections, and
  registration removal.
- The combined Rust suite passed 83 tests with no failures. Strict Clippy also
  passed.
- The latest TypeScript/Vite build and no-bundle Tauri release build passed,
  emitting `src-tauri/target/release/tauri-appdoc_sum`.
- That exact release executable was launched with an isolated runtime
  directory. While the real Tauri process was running, one mode-`0600`
  registration advertised protocol 1 at an ephemeral `127.0.0.1` endpoint;
  authenticated manifest retrieval returned the same instance ID plus
  `document.summarize` accepting `application/pdf`.
- The process was stopped with `Ctrl-C`, not a graceful desktop close. One stale
  registration remained and its endpoint was unreachable, proving why
  consumers must require a live authenticated manifest rather than trusting
  registration presence. Normal registration removal is separately exercised
  by the provider teardown test. No live model job was submitted in this
  process smoke.
- After that release-process launch, the independently reopened AppData SQLite
  database reported schema version 10, `quick_check = ok`, one `connect_jobs`
  table, and the single-active-job unique index. This is a process-driven
  migration plus connection reopen, not a full GUI close/reopen exercise.

**Deferred**:
- Package install/removal and a real live-model job remain to be exercised; the
  release-process manifest smoke and automated fake-runtime job proof are not
  mislabeled as either.
- Same-user malicious-process impersonation, launch-on-demand, multiple-provider
  choice, alternate transports, callbacks, workflow automation, remote
  execution, and cross-machine discovery remain deferred.

## Cross-App Product Direction: Connect and Shared Inference Are Separate Boundaries

**Status**: Architectural distinction and vLLM-primary/Ollama-fallback direction
accepted; shared on-prem inference remains future work

**Implementation tracking**: Email Watcher issue #72

**Two independent planes**:
- Connect is the typed interoperability plane between independently installed
  applications. It discovers domain capabilities such as `document.summarize`
  and exchanges explicit jobs and artifacts. It does not discover, select, or
  schedule models.
- `ModelRuntime` is an internal inference boundary used by an application that
  owns a model-dependent capability. Its transport may be the current local
  loopback adapter or a future authenticated on-prem gateway without changing
  the Connect capability contract.
- Email Watcher asks Connect for a compatible capability. Document Summarizer
  remains the capability provider and owns ingestion, provenance, pipeline
  state, validation, and results. Email Watcher never selects a model or calls
  the inference service directly.

**Current verified implementation**:
- Connect v1 registration, discovery, and jobs use an exact-loopback endpoint on
  the same computer. Cross-machine Connect discovery is not implemented.
- The current OpenAI-compatible `ModelRuntime` adapter also accepts only exact
  loopback HTTP. It does not support a private-network inference appliance.
- Runtime and model identities are persisted as result provenance. They do not
  form part of the caller-facing Connect capability selection contract.

**Standalone and availability contract**:
- Standalone means each application starts, owns its private persistence, and
  retains its non-Connect workflows without another application or Connect being
  available. It does not mean every model-dependent action can run without an
  available inference runtime.
- Summarization requires a configured, healthy `ModelRuntime`. If no runtime is
  available, the application must report that model-dependent action as
  unavailable or failed while keeping the rest of the application healthy.
- In a future appliance deployment, client computers need no model weights,
  CUDA toolchain, or inference GPU. The appliance is optional to application
  startup but required for inference while that deployment mode is selected.
- The current guarded loopback adapter remains a separate deployment path. A
  future network adapter must be additive rather than weakening its exact-
  loopback admission rule.

**Future on-prem inference direction**:
- The intended deployment is one small business with administrator and user
  roles, not multi-tenant SaaS. Customer content remains on the business's
  private network.
- The appliance would own model/runtime installation, capacity, health,
  authentication, and fair scheduling. Applications would request versioned
  task requirements rather than choose concrete model identities.
- The current prompt-level `ModelRequest` does not yet define the task-profile
  contract required for gateway routing. That contract, multi-model routing,
  and model promotion policy require a separate evidence-driven slice.
- The shared appliance selects vLLM as its primary worker and Ollama as a
  planned fallback that must be qualified independently for each eligible task.
  Applications continue to request task requirements through the authenticated
  inference gateway; they do not select either runtime, a model artifact, or
  fallback order.
- Primary promotion requires pinned vLLM package or container provenance, model
  artifact, dependencies, and serving configuration plus task-specific
  deterministic metrics and validation. Semantic outputs also require blinded
  human review; structural validity alone is insufficient.
- Ollama qualification is symmetric: pin its exact package or container,
  dependencies, model artifact content digest rather than a mutable tag, and
  complete serving configuration. Both workers must be gateway-private and
  unreachable directly from client-network computers.
- LM Studio and llama.cpp are no longer supported production-worker targets.
  Existing deployment files and compatibility evidence remain until accepted
  cutover work replaces their operational use; new application clients must not
  bind to either runtime.
- Fallback is gateway-owned and fail-closed. Ollama is eligible only when it is
  healthy, independently qualified for the same task requirements, uses an
  already-local approved model, runs with cloud access disabled
  (`OLLAMA_NO_CLOUD=1`), and vLLM is known unavailable before admission.
  Ambiguous or in-flight primary failures remain unresolved until the gateway
  recovers the primary result, proves non-acceptance, or confirms cancellation;
  retaining the request identity alone does not permit cross-worker replay.
  Before dispatch, the gateway must durably reserve the authenticated request,
  immutable expiry, canonical digest, and worker attempt so exact repeats join
  active work or return a protected unacknowledged result across gateway
  restarts. The result buffer is encrypted, credential-scoped, excluded from
  logs/diagnostics/backups, and deleted after the application durably persists
  and acknowledges the result or its immutable request expires. A metadata-only
  tombstone remains for a bounded replay-protection period beyond expiry, and
  expired identities never dispatch. Expiry terminalizes any in-flight attempt;
  cancellation is best-effort, and late output is discarded without recreating
  the protected result buffer or changing the terminal tombstone.
  Applications acknowledge through the versioned, same-credential
  `POST /v1/inference/{request_id}/ack` operation defined by the gateway ADR;
  request IDs use canonical URL-segment-safe UUIDv4 text. Exact repeats of an
  already-acknowledged tombstone remain idempotent after expiry, conflicting
  dispositions fail closed, and another credential cannot delete the result.
  Authentication, authorization, malformed-input, unsupported-task, and
  application-validation failures do not trigger fallback.
- An application that rejects a structurally valid gateway result must durably
  mark that request terminal and acknowledge it; it must not retry the same
  retained output indefinitely. Expiry or terminal output rejection stops
  automatic retries, and only explicit requeue creates a new identity.
- The implemented standalone and Connect provider paths still use
  `OllamaRuntime`; this direction does not claim a vLLM or gateway cutover.
  Existing Ollama/Qwen acceptance remains valid evidence for that deployment
  path. A later adapter changes the runtime factory behind `ModelRuntime` while
  preserving pipeline state, deterministic validation, and Connect behavior.
- The current Ollama model artifact is GGUF. Current vLLM documentation calls
  GGUF support experimental and under-optimized, so the primary-worker proof
  must select and pin an appropriate supported artifact instead of assuming the
  Ollama blob is the production vLLM artifact.

**Safety and ownership**:
- Shared inference is infrastructure, not application discovery or workflow
  orchestration. Connect retains the app-capability boundary.
- Models interpret unstructured content and propose results. Deterministic
  application code retains authorization, validation, idempotency, and
  irreversible-effect gates.
- A future network transport must be authenticated and encrypted; prompts and
  document bodies must not be logged by default. Applications must degrade
  safely when the appliance is unavailable.

**Current-slice constraint**:
- This direction does not expand the current Document Summarizer or Connect v1
  proof. The vLLM workload/capacity proof, gateway runtime, Ollama fallback,
  administrator UI, appliance packaging, multi-model routing, capacity policy,
  and cross-machine discovery remain separately scoped follow-up work.

## Connect v1 Two-App Acceptance Checkpoint (2026-08-29)

**Status**: Deterministic and selected Ollama/Qwen cross-process proofs passed

**Exact implementation heads exercised**:
- Document Summarizer provider source: `3e7ff62`.
- Email Watcher consumer: merged `origin/main` commit `ea663e1` from PR #26.

**Cross-process proof**:
- The Document Summarizer release executable was rebuilt from the current source
  and launched by Email Watcher's `connect-local-proof.py` in an isolated runtime
  and data directory.
- A realistic repository PDF was supplied as synthetic Gmail attachment bytes.
  The proof used the real provider process, real consumer discovery/client code,
  real parser-to-summary pipeline, and both applications' real SQLite stores. It
  used a deterministic fixture model and did not contact Gmail.
- Observed capability counts were `0` before provider startup, `1` while live,
  `0` after provider termination, and `1` after restart. No Email Watcher code or
  configuration changed between those availability states.
- The Connect job completed after the provider's streamed size/SHA-256 admission
  checks. Email Watcher's persisted input SHA-256 matched the supplied PDF; its
  database reopened with `quick_check = ok` at schema version 4, and its
  persisted status and summary matched the returned result.

**Selected Ollama runtime acceptance**:
- The merged Email Watcher harness at `13e2e7f` exercised the Document Summarizer
  provider whose latest `src-tauri` code commit was `3e7ff62`. PR #27 subsequently
  merged as `5e0aee8`.
- Ollama `0.24.0` served `qwen3-30b-a3b:latest` (model ID `1eda56426671`) at the
  exact-loopback OpenAI-compatible endpoint. The server process environment was
  verified to use the Dev Drive model store, cloud access disabled, and an 8192-
  token context; `ollama ps` reported the selected model fully on GPU.
- The real configured-runtime proof observed capability counts `0` before
  provider startup, `1` while live, `0` after termination, and `1` after restart.
  The job and persisted job both completed, the persisted input hash and summary
  matched, and the independently opened Email Watcher database returned
  `quick_check = ok` at schema version 4.
- This is an integration acceptance for the selected runtime/model using the
  realistic repository PDF. It does not repeat or replace the separate Email
  Watcher model-quality evaluation.

**Verification gate**:
- Document Summarizer: all 83 Rust tests passed, `cargo fmt --check` passed,
  strict Clippy passed, and the release executable rebuilt successfully.
- Email Watcher: all 214 Python tests and all 15 desktop Rust tests passed. Ruff
  lint passed. The modified proof harness passed its focused formatter check and
  rejected partial configured-runtime arguments and a zero timeout. A focused
  failure probe also proved that unsuccessful provider startup cleans up the
  spawned child process.
- The TypeScript/Vite production build, packaged Python sidecar, optimized Tauri
  desktop executable, and Debian bundle completed successfully.
- The repository-wide Email Watcher Ruff formatter check is not green: the
  installed formatter reports 21 pre-existing files would be reformatted. Those
  unrelated files were not rewritten in this slice.

**Evidence boundary**:
- The proof harness now supports an explicitly configured exact-loopback
  OpenAI-compatible endpoint in addition to its deterministic fixture default.
  That configured path is now accepted against the selected Ollama-hosted Qwen3
  30B-A3B model for the proof fixture.
- No live Gmail OAuth attachment fetch, human Tauri Summarize click, Debian
  package install/uninstall, cross-machine transport, or on-prem gateway was
  exercised. These are not claimed by this checkpoint.

## Slice 6 — Ollama-Backed Standalone Desktop Workflow

**Status**: Implemented and locally accepted

**Runtime and application boundary**:
- The supported default runtime is now Ollama at
  `http://127.0.0.1:11434/v1/` with `qwen3-30b-a3b:latest`. Existing
  `DOC_SUM_MODEL_*` deployment overrides remain supported, and exact-loopback,
  no-proxy, no-redirect, token-file, and bounded-response protections remain in
  force. Connect and standalone processing use the same `OllamaRuntime` through
  the existing `ModelRuntime` abstraction.
- A UI-neutral workspace service exposes Ollama readiness, the 30 most recently
  updated pipeline runs, and integrity-checked persisted-summary retrieval.
  History ordering is deterministic, private source paths are not exposed, and
  incomplete runs cannot masquerade as completed summaries.
- History listing checks summary-row presence without decoding every artifact;
  an individual corrupt artifact therefore remains accounted for but fails
  closed when opened.

**Desktop workflow and build behavior**:
- The template page was replaced with explicit runtime-checking, unavailable,
  empty, processing, completed-summary, warning, failure, and recent-history
  states. TypeScript remains display/interaction code and calls only thin Tauri
  commands; it contains no SQLite, state-transition, parsing, or inference
  logic.
- The production window now opens as `Document Summarizer` at a desktop-oriented
  default size with responsive narrow-window behavior, visible keyboard focus,
  and reduced-motion handling.
- Canonical desktop scripts invoke the Tauri CLI. Tauri configuration explicitly
  enables `custom-protocol`, and a release compilation without that feature is
  rejected at compile time instead of producing a binary that depends on the
  Vite development server.

**Automated proof**:
- Six focused workspace tests passed for ready/unavailable runtime states,
  incomplete-run handling, deterministic tie ordering, corrupt-artifact
  isolation, identical history/summary retrieval after an independent database
  reopen, and the narrow frontend serialization boundary. The selected Ollama
  default test also passed.
- The combined Rust suite passed 90 tests with no failures. `cargo fmt --check`,
  strict all-target/all-feature Clippy, and the TypeScript/Vite production build
  passed.
- `npm run desktop:build:no-bundle` completed and emitted the release desktop
  executable. The opposite boundary probe ran raw release compilation without
  `custom-protocol` and observed the intended compile-time rejection message.

**Production-process and persistence proof**:
- Ollama `0.24.0` was started on `127.0.0.1:11434` with the Dev Drive model
  store, cloud disabled, and an 8192-token context. A bounded request to the
  exact API returned `READY`; `ollama ps` reported model ID `1eda56426671`, 18 GB,
  100% GPU, and context 8192.
- With Vite stopped and port 1420 unserved, the release executable launched from
  isolated runtime/data directories and rendered the full embedded UI. Its real
  Tauri readiness command displayed `Ollama ready` and
  `qwen3-30b-a3b:latest`.
- A realistic PDF was submitted to that production process through its public
  Connect contract. The actual Qwen job completed with a 1,372-byte summary and
  two persisted warnings; the returned input SHA-256 matched the fixture.
- The window was closed normally and its process exited successfully. An
  independent SQLite open returned `quick_check = ok`, schema version 10, one
  summary artifact, and an authoritative `CompleteWithWarnings` run at state
  version 18.
- Reopening the same release executable against the same application-data
  directory displayed `structured_report.pdf` in Recent work. Activating that
  row invoked the integrity-checking Tauri retrieval command and rendered the
  persisted summary without rerunning the job.

**Evidence boundary and deferred hardening**:
- The real file-picker-to-standalone-submit click was not automated in this
  checkpoint; the production-process job entered through Connect. The same
  application service and real model runtime were exercised, while file-picker
  wiring remains covered by TypeScript compilation and visual inspection.
- Runtime-unavailable behavior is covered by a focused Rust boundary test and a
  browser fallback-state visual pass, not by stopping the live Ollama process
  during the production smoke.
- A normal GUI close still left an unreachable Connect registration file in the
  isolated runtime directory. Live authenticated manifest discovery makes that
  stale entry non-authoritative, so it did not block standalone restart or this
  slice, but process-exit registration cleanup remains deferred Connect
  hardening.
- No schema version, pipeline transition, prompt, summary semantics, Connect
  wire contract, OCR, citation, or semantic-verification behavior changed.

## Slice 7 — Evidence-Linked Summaries and Citation Presentation

**Status**: Implemented and locally accepted for native-text PDFs

**Evidence and citation contract**:
- `ModelRequest` now supports either plain text or a named, bounded JSON Schema
  output contract without coupling the generic `ModelRuntime` boundary to
  Ollama types.
- Analysis, synthesis, verification, and summary versions are `2.0.0`.
  Analysis accepts only bounded evidence whose block belongs to the current
  chunk and whose quotation is a contiguous exact substring of the
  authoritative normalized block. Rust derives the source span, chunk binding,
  and deterministic evidence ID.
- Synthesis accepts only bounded claims with known, unique evidence IDs. Rust
  canonicalizes evidence references into source order, derives deterministic
  claim IDs, and renders page labels from the validated source spans. Neither
  model stage can supply page numbers or provenance.
- Mechanical verification now validates the entire
  claim-to-evidence-to-normalized-block chain and deterministic rendered text.
  `SEMANTIC_VERIFICATION_DEFERRED` remains mandatory: citation traceability is
  not a claim that a model-authored sentence is entailed by its excerpt or that
  the source is factually correct.

**Persistence and state integrity**:
- Explicit schema v11 adds an independent `citation_artifacts` table with
  document/run association, citation version `1.0.0`, summary-integrity binding,
  creation time, serialized artifact, and SHA-256 row hash. The artifact also
  carries its own integrity hash.
- The final summary, citation artifact, `VERIFIED -> COMPLETE_WITH_WARNINGS`
  transition, state-version increment, and immutable event append share one
  SQLite transaction. Injecting failure at the citation insert rolls back both
  artifact rows and prevents a completion event before the run transitions
  truthfully to `FAILED`. A separate injected completion-event failure proves
  the same rollback after both artifact inserts have already executed.
- Citation retrieval checks row integrity, internal integrity, document/run
  metadata, creation time, and summary binding. Current summaries fail closed
  when citations are absent or malformed. Additive v10-to-v11 migration leaves
  historical summary rows untouched; version `1.0.0` summaries remain readable
  with an empty citation presentation rather than fabricated evidence.

**Selected Ollama compatibility**:
- The first configured-runtime proof exposed an HTTP 500 from Ollama 0.24.0:
  `failed to load model vocabulary required for format`. A direct request to
  the same `qwen3-30b-a3b:latest` model without grammar enforcement returned
  valid contract JSON, isolating the incompatibility to Ollama's format layer
  for this imported model rather than the task or Connect path.
- The adapter still attempts JSON Schema first. Only that exact HTTP-500 body
  activates a one-time process-local fallback; subsequent requests omit the
  server grammar while retaining explicit JSON shapes in both prompts and all
  strict Rust JSON, identity, quote, provenance, and size validation. Other
  HTTP errors remain ordinary runtime failures.

**Standalone and Connect presentation**:
- The Rust workspace projection exposes only claim text, application-derived
  page labels/ranges, and exact quotations. It withholds private source paths,
  block IDs, chunk IDs, and artifact hashes. Its boundary rejects duplicate
  references, invalid page spans, missing evidence, and inconsistent hashes.
- The desktop renders claim cards and page buttons; selecting one displays its
  exact excerpt. All model/source content is assigned with `textContent` rather
  than HTML interpretation. Legacy summaries retain the previous plain-text
  rendering.
- Connect v1 remains wire-compatible. Its frozen JSON shape still receives the
  summary text, now with deterministic page labels; structured citation output
  is deferred to a future capability/output-media version rather than added to
  v1 implicitly.

**Automated and live proof**:
- The standard Rust run executed 106 tests: 105 passed and the explicitly live
  Ollama test was ignored by default. New probes cover exact valid evidence,
  unknown block IDs, quote mismatch, mixed valid/invalid evidence, unknown and
  duplicate evidence references, malformed model JSON, deterministic IDs,
  normalized provenance, summary/citation lifecycle, citation-insert rollback,
  dual-hash tamper detection, independent database reopen, missing-current-
  citation failure, legacy-summary compatibility, presentation-field
  narrowing, invalid presentation spans/references, schema bounds, and v10-to-
  v11 migration/reopen.
- The ignored live test was then run explicitly against the configured Ollama
  runtime and selected Qwen model. It passed the real PDF path, asserted
  non-empty claims/evidence, exact quotations against normalized blocks,
  application-derived source spans, persisted model identity, both artifact
  hashes, summary binding, SQLite `quick_check`, and
  `COMPLETE_WITH_WARNINGS` after an independent database-connection reopen.
  This is a connection-reopen proof, not a full process restart.
- Strict Rust formatting and all-target/all-feature Clippy passed. The
  TypeScript/Vite production build and no-bundle Tauri release build passed,
  emitting `src-tauri/target/release/tauri-appdoc_sum` with the citation UI
  embedded.
- The rebuilt release provider was exercised by Email Watcher's cross-process
  proof with synthetic Gmail attachment bytes, the realistic repository PDF,
  and the configured Qwen model. Observed capability counts were 0 before
  provider startup, 1 while live, 0 after stop, and 1 after restart. The job and
  persisted caller job completed, the PDF hash and returned/persisted summary
  matched, and Email Watcher's independently opened database returned
  `quick_check = ok` at schema version 4.

**Evidence boundary and deferred work**:
- The new claim cards and exact-excerpt interaction are TypeScript-compiled and
  exercised through the narrow Rust serialization tests, but a human or
  automated click on a citation in a live Tauri window was not performed in
  this checkpoint.
- Semantic entailment/factual verification, PDF-viewer navigation, structured
  Connect citation output, citations for future OCR/vision evidence, OCR,
  vision, embeddings, RAG, chat, workflow automation, and cross-machine model
  transport remain deferred.

## Slice 8 — Desktop Instance Ownership and Interrupted-Run Reconciliation

**Status**: Implemented and locally accepted

**Recovery contract**:
- The official Tauri single-instance plugin is registered before setup and all
  other plugins. A second desktop launch routes its callback to the existing
  process, requests window focus, and exits instead of opening the shared
  application database as a competing workflow owner.
- After ownership and schema migration, desktop startup scans runs in
  deterministic `run_id` order. Implemented active states from `INGESTING`
  through `VERIFYING` map explicitly to their stages and transition to `FAILED`
  with a recoverable `PROCESS_INTERRUPTED` failure.
- Recovery uses the existing expected-state/version transition boundary. Every
  candidate's state, version, failure, and immutable event commit in one batch
  transaction. Stable runs are untouched, any failed event/write rolls back the
  full batch, and repeated startup is idempotent.
- Recovery preserves source/document identity, existing checkpoint artifacts,
  warnings, and event history. It performs no parser or model calls. This slice
  does not add backward state edges, same-run resume, or automatic model replay;
  retry means resubmitting the document.

**Automated proof**:
- Five focused Rust tests passed for the complete active/stable state mapping,
  connection-close/reopen recovery, stable-run/source/document preservation,
  idempotency, mixed/invalid candidate rejection, batch rollback on the second
  event insert, and a two-connection stale recovery snapshot losing to a newer
  failure.
- The full Rust run executed 112 tests: 111 passed and the opt-in live Ollama
  test was ignored. Strict formatting and all-target/all-feature Clippy passed.
- The TypeScript/Vite production build and no-bundle Tauri release build passed
  with `tauri-plugin-single-instance` 2.4.3 locked.

**Real process proof**:
- Two release executables were launched against the same isolated runtime and
  data directories. The second exited with status 0 while the first remained
  alive, proving the single-instance gate on the exercised Linux desktop.
- An isolated application database was seeded with a coherent `PARSING` run at
  version 4 and four ordered events. Launching the release executable invoked
  the real Tauri setup path and persisted `FAILED` at version 5 with a
  stage-`Parse`, recoverable `PROCESS_INTERRUPTED` failure and sequence-4
  `PARSING -> FAILED` event. A second independent process launch left version 5
  and the five-event history unchanged.
- A read after both processes exited returned SQLite `quick_check = ok` at
  schema version 11.
- The process probe seeded an interrupted state after the first app process was
  stopped; it did not kill a parser while native parsing was executing. The
  automated connection-reopen test creates its active `PARSING` state through
  the real ingestion and `start_parsing` boundaries.

**Known boundaries and deferred work**:
- `init_db` deliberately does not reconcile work; non-Tauri/headless entrypoints
  must acquire equivalent exclusive ownership before calling the explicit
  recovery service.
- Same-run resume/retry, cancellation recovery, visual-analysis recovery,
  startup recovery notices in the UI, and Snap/Flatpak DBus packaging rules for
  single-instance behavior remain deferred.
- The second-process probe proved clean exit and survival of the owning process;
  it did not visually assert that the existing window took focus.
- The existing stale Connect registration-file cleanup and live UI citation
  click remain separate deferred hardening.

## Slice 9 — Explicit New-Run Retry and Recovery Visibility

**Status**: Implemented and locally accepted

**Retry and lineage contract**:
- An eligible `FAILED` run can create one direct child attempt from the durable
  `INGESTED` checkpoint. Eligibility requires `resumable = true`, a structured
  recoverable failure from a post-ingestion stage, the caller's current source
  state version, and no existing direct child.
- Retry never transitions a terminal run backward. The child shares the
  durable `document_id` but receives its own `run_id`, state versions, events,
  downstream artifacts, and final outcome. A failed child may become the
  source of another attempt, preserving the full retry chain.
- Explicit schema v12 adds `pipeline_run_retries`, including source uniqueness,
  same-row source/child rejection, foreign keys, and update/delete rejection
  triggers. Reads validate that both attempts belong to the same document and
  that lineage time matches child creation time.

**State, source, and transaction behavior**:
- Child creation reuses the ordinary centralized state transition boundary to
  persist `RECEIVED -> INGESTING -> INGESTED -> PARSING`. Its creation event is
  labeled `retry_run_created`; both checkpoint transitions are labeled
  `retry_checkpoint_reused`, and parser admission is labeled
  `retry_processing_started`.
- The child run, four immutable events, active parser state, and lineage
  relation commit in one immediate SQLite transaction. Injected lineage failure
  rolls all of them back. Committing an active state also ensures a crash before
  parser work is reconciled on restart instead of stranding an inaccessible
  `INGESTED` child. The source run and its existing event history are never
  written.
- Processing after atomic parser admission uses the existing parser-neutral
  pipeline. The parser rereads the persisted source, validates its exact byte
  size and SHA-256, and durably fails only the child when the source is missing
  or has changed. Restoring the bytes permits a new attempt sourced from that
  failed child; no duplicate document row is created.

**Desktop behavior**:
- Recent-work projections expose only retry lineage IDs and an application-
  derived `canRetry` decision. The frontend does not choose a checkpoint or
  mutate state; it supplies source run ID plus expected version to one thin
  Tauri command.
- Startup-reconciled attempts render a durable recovery notice and an
  `Interrupted · retry available` history state. Opening an eligible failure
  offers `Retry as new attempt`; an already-retried parent points the operator
  back to Recent work. Runtime-unavailable state disables execution without
  changing either attempt.

**Automated proof**:
- Focused service probes cover parent immutability, child completion, durable
  lineage and summary retrieval after independent database reopen, stale CAS,
  non-failed and non-recoverable rejection, one-child enforcement, lineage-
  insert rollback, crash-before-parser recovery, exact source-mutation failure,
  byte restoration, and chained retry. The schema migration probe covers
  v11-to-v12 preservation, immutable lineage, repeated initialization, reopen,
  and SQLite `quick_check`.
- The standard Rust suite executed 118 tests: 117 passed and the opt-in live
  Ollama test was ignored. Strict Rust formatting, all-target/all-feature
  Clippy, the TypeScript/Vite production build, and the no-bundle Tauri release
  build passed.

**Real process and UI proof**:
- A schema-v12 database in a fresh isolated application-data directory was
  seeded with a coherent `PARSING` run at version 4 and four ordered events.
  The release executable's real Tauri setup path reported one reconciliation;
  an independent SQLite read observed `FAILED` at version 5, recoverable
  `PROCESS_INTERRUPTED`, the sequence-4 `PARSING -> FAILED` event, and
  `quick_check = ok`.
- The release executable was then reopened against the same durable database
  through the X11 desktop path. A captured 1180-by-780 application window
  displayed the recovery notice and marked the source item
  `Interrupted · retry available`.

**Evidence boundary and deferred work**:
- Retry execution is proven through the application service with deterministic
  parser and runtime boundaries; this checkpoint did not click the live Tauri
  retry button or rerun the selected Ollama model. The runtime and full summary
  pipeline were unchanged by this slice.
- Slice 9 deliberately reuses only `INGESTED`, so parsing and every downstream
  stage are recomputed. It does not copy parser/model artifacts, mutate a failed
  attempt, auto-replay work, add cancellation, or change Connect's job retry
  contract.
- A retry still requires the privately owned source path to remain readable
  with its original bytes. Background job execution, cancellation UI, visual
  analysis, OCR, and stale Connect registration cleanup remain deferred.
- Stable inter-stage checkpoints reached just before a later stage starts are
  still preserved as truthful incomplete runs rather than auto-replayed on
  startup. A future explicit checkpoint-resume policy may make those runs
  actionable; this slice closes only the retry-child creation-to-parser handoff.

## Slice 10 — Explicit Stable-Checkpoint Continuation

**Status**: Implemented and locally accepted

**Continuation contract**:
- Stable `INGESTED`, `PARSED`, `NORMALIZED`, `STRUCTURED`, `CHUNKED`,
  `ANALYZED`, `SYNTHESIZED`, and `VERIFIED` runs expose a core-derived
  continuation plan. The caller supplies the run ID and exact state version but
  cannot select a checkpoint.
- Continuation keeps the same run/document identity and follows only existing
  forward transitions. It neither rewrites prior artifacts/events nor creates
  retry lineage. `FAILED` remains terminal and continues to use Slice 9's
  separate child-attempt contract.
- Summary processing now has durable entry points at `CHUNKED`, `ANALYZED`,
  `SYNTHESIZED`, and `VERIFIED`. Completed analysis is not requested again;
  completed synthesis and verification can finish without constructing an
  Ollama runtime.

**Desktop behavior**:
- Recent-work projections expose `canContinue`, the stable checkpoint, and the
  runtime requirement. Opening an incomplete stable run offers a contextual
  `Continue processing` action. The frontend sends only run ID and expected
  version; Rust owns checkpoint selection and all state/artifact work.
- Ollama readiness gates checkpoints through `ANALYZED`. `SYNTHESIZED` and
  `VERIFIED` remain actionable while Ollama is unavailable because their
  remaining operations are deterministic.

**Focused proof completed**:
- All eight stable checkpoints continued to one durable summary on the same
  run while preserving their checkpoint artifact and immutable event prefix.
  Runtime call counters proved `ANALYZED` invokes only synthesis and
  `SYNTHESIZED`/`VERIFIED` invoke no model operations.
- Stale versions, missing required runtime, active runs, failed runs, and
  completed runs were rejected. A deliberately corrupted normalized artifact
  produced no next artifact, state mutation, version increment, or event.
- An injected final citation-write failure rolled back both final artifacts and
  the completion transition/event, then durably recorded failure. A synthesized
  checkpoint survived connection close/reopen, completed without a runtime,
  and its summary, citations, original artifact, and event prefix survived a
  second independent connection reopen with SQLite `quick_check = ok`.

**Verification boundary and deferred work**:
- The full Rust suite executed 122 tests: 121 passed and the unchanged opt-in
  live Ollama test was ignored. Strict formatting and all-target/all-feature
  Clippy passed. The TypeScript/Vite production build and no-bundle Tauri
  release build passed, producing the release executable with the new command
  and UI embedded.
- The release executable was launched with an isolated application-data
  directory through the available X11 desktop path, remained active, and its
  rendered standalone workspace was captured and inspected. Ollama was
  unavailable in that isolated smoke run, so the ordinary new-PDF action was
  correctly disabled. No stable checkpoint was seeded into that application
  database, so the live `Continue processing` button was not clicked; its
  rendering/dispatch path is TypeScript-compiled and its Rust read/command
  contracts are covered by the focused tests above.
- Startup remains non-replaying. Automatic scheduling, background jobs,
  cancellation, visual/OCR routing, Connect retry/resume, and cross-process
  continuation races beyond SQLite CAS remain deferred.

## Slice 11 — Model-Assisted Claim Verification

**Status**: Implemented and locally accepted

**Verification contract**:
- Analysis and synthesis remain version `2.0.0`. Verification and summary are
  version `3.0.0`; citation artifacts are version `2.0.0`.
- Verification now calls the replaceable `ModelRuntime` with each canonical
  synthesized claim and only its validated exact quotations. The bounded
  responses contain application-issued claim IDs plus `supported`,
  `unsupported`, or `ambiguous`; Rust rejects malformed, partial, duplicate,
  foreign, or oversized responses and restores evidence IDs from durable
  application state. The complete catalog is size-checked before runtime
  health, then classified in deterministic batches of at most 16 claims with a
  4,096-token output budget.
- `VerifiedDocument` persists runtime/model identity and a verdict for every
  synthesized claim. Supported claims alone are rendered into the final
  summary. Unsupported and ambiguous claims remain durable for audit, are
  omitted from the displayed result, and produce `SEMANTIC_CLAIMS_WITHHELD`.
- This is evidence-entailment screening by the selected model, not independent
  verification that the source document itself is factually true.

**State, atomicity, and compatibility**:
- `SYNTHESIZED -> VERIFYING -> VERIFIED` continues through the existing
  expected-state/version boundary. Verdict artifact insertion,
  `VERIFYING -> VERIFIED`, version increment, and immutable event append share
  one transaction. An injected event failure rolled all of them back before a
  truthful failure was recorded.
- A result with supported claims completes as `COMPLETE` when warning-free or
  `COMPLETE_WITH_WARNINGS` when inherited or withheld-claim warnings remain.
  If no claims are supported, the complete verdict artifact is retained at the
  `VERIFIED` checkpoint before `VERIFIED -> FAILED` records
  `NO_SEMANTICALLY_SUPPORTED_CLAIMS`; no summary or citation artifact exists.
- `SYNTHESIZED` continuation now requires a runtime and performs exactly the
  verification call; `VERIFIED` continuation remains runtime-free. A stale
  verification caller cannot change state/version or append an event.
- No SQLite migration was required. Existing versioned artifact JSON and row
  hashes carry the added fields. A synthesized legacy mechanical verification
  `2.0.0` fixture survived independent database reopen and completed without a
  runtime as paired summary `2.0.0` plus citation `1.0.0`, retaining
  `SEMANTIC_VERIFICATION_DEFERRED`.

**Automated and live proof**:
- The full Rust suite ran 133 tests: 132 passed and the opt-in Ollama test was
  ignored by default. Focused verification tests cover full and reordered
  verdict coverage, malformed/partial/duplicate/foreign verdict rejection,
  input bounds before generation, mixed supported/unsupported/ambiguous
  filtering, zero-supported failure, runtime failure, stale CAS, injected
  event-write rollback, deterministic output, and artifact reopen equality.
- Strict all-target/all-feature Clippy passed. The TypeScript/Vite production
  build and no-bundle Tauri release build passed, producing
  `src-tauri/target/release/tauri-appdoc_sum`.
- The opt-in live test passed against Ollama on `127.0.0.1:11434` with
  `qwen3-30b-a3b:latest`. Ollama reported the already-documented structured
  grammar incompatibility, so the exact-error fallback ran while the same
  strict Rust contract remained mandatory. The test persisted runtime/model
  identity and complete verdict coverage, then independently reopened SQLite
  and matched the verdict, summary, citation hashes, terminal state, and
  `quick_check = ok`. This proves a database-connection reopen, not a full
  desktop-process restart.
- PR review identified and corrected two boundary-order/capacity defects before
  merge: permanent oversized input now wins over runtime health failure, and
  the accepted 64-claim catalog no longer depends on one 2,048-token response.
  Focused tests exercise both corrected sides directly.

**Cold architecture audit and deferred work**:
- Verification consumes only canonical synthesized claims, validated evidence,
  chunk metadata, and normalized provenance. It exposes no PDF-library type,
  adds no frontend verification logic, and changes no Connect wire contract.
  Ollama remains behind `ModelRuntime`; a replacement runtime requires no
  pipeline-state or persistence redesign.
- The live proof used the application service and real model but did not launch,
  close, and reopen the Tauri UI process. Independent source-fact checking,
  adversarial verifier-model evaluation, hierarchical synthesis, OCR/vision,
  visual-page citations, background jobs, cancellation, and Connect protocol
  changes remain deferred.

## Slice 12 — Background Execution and Cooperative Cancellation

**Status**: Implemented; locally verified

**Execution contract**:
- Standalone summarize, retry, and stable-checkpoint continuation now return an
  accepted run projection after an application-owned worker starts. The worker
  performs the existing parser-to-summary service flow on its own SQLite
  connection while exact-run status reads use independent short-lived
  connections. The former application-wide connection mutex is gone.
- `PipelineRun` remains the durable job identity; no parallel job table or
  schema migration was introduced. A per-process registry prevents duplicate
  desktop workers and ensures the UI does not claim ownership of Connect work.
- Existing synchronous core and Connect service entry points retain their
  behavior through an explicit no-op execution control. The background manager
  is an application adapter around the same parser, normalizer, structure,
  chunking, runtime, verification, and persistence boundaries.

**Cancellation, state, and atomicity**:
- A fresh run/version request atomically commits the current cancellable state
  to `CANCELLING`, sets the durable request marker, increments state version,
  and appends an immutable event. Worker acknowledgement atomically commits
  `CANCELLING -> CANCELLED` with its next version and event.
- Cooperative checkpoints exist between pipeline stages, before and after
  runtime operations, for every analysis chunk, and for every verification
  batch. Ollama requests and deterministic work units are not killed midway.
  If cancellation wins, the existing state/version CAS prevents unfinished
  artifacts and success events from committing; completed checkpoints remain.
- Startup recovery now converts a coherently marked `CANCELLING` run to
  `CANCELLED` without replay. A boundary test corrupts only the marker and proves
  that recovery rejects the inconsistent row without changing version or event
  history.
- The failure/cancellation race is resolved inside an immediate transaction: a
  cancellation that already won is acknowledged as `CANCELLED`, while a stale
  worker that lost CAS ownership cannot fail a newer state. Paired tests prove
  stale finalization is non-mutating and a genuine non-stale worker error still
  becomes durable `FAILED`.
- Worker-start failure after new/retry admission, panic, or unexpected
  nonterminal return becomes a structured recoverable failure when SQLite
  remains writable. A continuation that cannot spawn retains its stable
  checkpoint. A cancelled run is terminal and is not presented as an ordinary
  processing failure.

**Desktop behavior and proof**:
- The TypeScript UI polls `get_run_status` by exact run ID, displays the durable
  stage, exposes `Cancel processing` only for a registered cancellable desktop
  worker, and resumes monitoring after a webview reload within the same process.
  Explicit ephemeral `backgroundActive` status prevents terminal cancellation
  history from masquerading as live work and stops polling if worker ownership
  disappears. Frontend code does not set cancellation state or write
  persistence.
- A blocking-runtime race test proves the initial command returns while model
  work remains active, a second database connection remains readable, stale
  cancellation changes nothing, fresh cancellation wins during generation, no
  analysis/summary artifact persists, both cancellation events remain ordered,
  and the terminal result survives independent SQLite reopen with
  `quick_check = ok`.
- A separate background completion test proves a normal accepted run reaches a
  durable completed summary and survives independent SQLite reopen. Focused
  database tests inject event-write failure and prove state, version, marker,
  and event rollback together.
- The final all-target Rust suite ran 143 tests: 142 passed and the opt-in live
  Ollama test was ignored by default. Strict all-target/all-feature Clippy, the
  TypeScript/Vite production build, and the no-bundle Tauri release build
  passed. The release executable was emitted at
  `src-tauri/target/release/tauri-appdoc_sum`.
- Release builds were launched three bounded times against the same isolated
  application-data profile through the available desktop display, including a
  launch after the final rebuild. The rendered 1180-by-780 window reported
  `Ollama ready` and `qwen3-30b-a3b:latest`; while each process was live, the
  same SQLite database reported schema version 12 and `quick_check = ok`. This
  proves real release-process startup and reopen of an empty application
  database. It does not prove a human file-selection/cancellation interaction:
  no GUI input driver was installed, so the native picker and cancel button
  were not clicked during the process smoke. The worker/cancellation race is
  automated below the Tauri adapter, and the frontend dispatch path is
  TypeScript-compiled.

**Deferred**:
- Cancellation remains cooperative rather than forced mid-request. Progress is
  stage-level; there is no persisted percentage/work-unit counter beyond the
  existing pipeline metadata. Automatic replay, scheduler/queue infrastructure,
  cross-process cancellation, Connect cancellation, OCR/vision, and hierarchical
  synthesis remain deferred.

## Slice 13 — Bounded Hierarchical Synthesis

**Status**: Implemented; locally verified

**Synthesis contract**:
- Synthesis version `3.0.0` preserves the existing small-document direct path
  when the ordered evidence catalog fits one request. Each synthesis request is
  capped at eight items and 16,000 Unicode characters.
- Larger catalogs are partitioned deterministically in source order. Each
  evidence batch emits at most four intermediate claims; non-final candidate
  batches must reduce by at least half. The conservative plan and runtime guard
  cap one synthesis attempt at 256 model requests.
- Intermediate candidate IDs derive from stable document/version/round/batch,
  output-order, text, and evidence inputs. Model responses may cite only IDs
  supplied in that exact request. Rust expands candidate references to unique,
  canonical original evidence IDs and rejects any claim exceeding the existing
  16-evidence provenance bound.
- Only final cited claims are materialized in `SynthesizedDocument`. Candidate
  artifacts remain ephemeral and never become a checkpoint. Any later synthesis
  attempt must recompute them under the existing failure/retry policy rather
  than treating partial reduction work as complete. Historical synthesis
  version `2.0.0` artifacts remain readable with their original deterministic
  claim identities.

**State, cancellation, and persistence proof**:
- Cancellation checkpoints surround every evidence and candidate model request.
  A focused token test requests cancellation after the first response and proves
  that no second request begins.
- The existing completion transaction remains authoritative: final artifact
  insertion, `SYNTHESIZING -> SYNTHESIZED`, state-version increment, warnings,
  and immutable event append commit together. An injected event failure rolls
  back the artifact and success transition before recording `FAILED`.
- A hierarchical artifact produced from a catalog exceeding the former
  100,000-character aggregate limit survived an independent SQLite connection
  close/reopen with the same artifact and run identity. This is a database
  connection-reopen proof, not a full desktop-process restart.

**Boundary and regression proof**:
- The summary module ran 36 tests: 35 passed and the opt-in live Ollama test was
  ignored by default. It covers the one-request direct path,
  deterministic over-limit hierarchy, exact per-request size/item bounds,
  original-evidence provenance expansion, known-but-cross-batch evidence
  rejection, foreign-candidate rejection, cancellation, malformed output,
  success-event rollback, independent reopen, resource-limit boundaries, and
  synthesis-version compatibility alongside all prior summary/citation tests.
- The all-target Rust suite ran 153 tests: 151 passed, no test failed, and the
  two opt-in live Ollama tests were ignored. Rust formatting, strict
  all-target/all-feature Clippy, the TypeScript/Vite production build, and the
  no-bundle Tauri release build passed.
- The new opt-in hierarchical test forced nine real extracted source lines
  through the candidate-reduction schema using `qwen3-30b-a3b:latest`. With a
  test-only 300-second per-request override it passed in 86.00 seconds and the
  final claims referenced only original evidence IDs. The unchanged 60-second
  default timed out under the observed shared-machine load, where a separate LM
  Studio process occupied most GPU memory and Ollama reported predominantly CPU
  execution. This is live model-contract proof with an explicit timeout
  override, not proof that the default timeout tolerates concurrent GPU-heavy
  workloads.

**Known limitations and deferred work**:
- Request bounds use deterministic Unicode-character and item limits rather than
  runtime-specific tokenization. The selected runtime still enforces its own
  context and response limits.
- Intermediate reductions are intentionally recomputed by any later attempt;
  persisted per-batch progress and same-stage replay remain deferred.
- OCR/vision, citations for visual-only pages, finer progress counters,
  scheduler/queue infrastructure, cross-process or Connect cancellation, and
  independent source-fact checking remain deferred.

## Slice 14 — Reproducible Linux Release Package and Desktop Acceptance

**Status**: Implemented; locally verified

**Release contract and fixes**:
- Replaced scaffold package metadata with the `Document Summarizer` product,
  `document-summarizer` Cargo/npm/binary identity, Juan Canfield publisher, a
  native-text PDF description, and a productivity desktop category. The Tauri
  application identifier remains unchanged, preserving the existing private
  application-data location.
- Retained the cross-platform base bundle configuration and added a Linux
  override selecting the currently supported Debian bundle. The canonical
  `npm run desktop:build` therefore no longer requests an AppImage from this
  host, where the inspected GTK bundling plugin required unavailable
  `librsvg-2.0` development metadata.
- Moved both legacy PDF probe sources intact from Cargo's conventional
  `src/bin` directory into `src-tauri/tools/legacy`. A clean target can no
  longer fail because Tauri expects an excluded probe binary, and a dirty
  target can no longer leak a stale diagnostic executable into the package.
- Added a focused release-contract test for product metadata, the Linux bundle
  boundary, and both sides of the probe-discovery rule: no Rust probes under
  `src/bin`, while both retained legacy sources still exist.

**Package and regression proof**:
- `npm run desktop:build` was run with a newly created empty Cargo target. It
  completed and emitted one Debian bundle. `dpkg-deb` reported package
  `document-summarizer`, version `0.1.0`, architecture `amd64`, maintainer
  `Juan Canfield`, and the intended short/long descriptions.
- Package-content inspection found one executable,
  `/usr/bin/document-summarizer`, plus the desktop entry and application icons;
  neither legacy probe executable was present. The regenerated desktop entry
  reported `Name=Document Summarizer`, `Exec=document-summarizer`, and a
  nonempty `Office` category.
- The all-target/all-feature Rust library suite ran 153 tests: 151 passed and
  the two opt-in live Ollama tests were ignored. The release-contract
  integration target ran 3 tests and all passed. Rust formatting, strict
  all-target/all-feature Clippy, and the TypeScript/Vite production build also
  passed.

**Real desktop, failure, and restart proof**:
- The exact fresh-target release executable was launched with an isolated
  application-data profile. Its rendered Tauri window reported `Ollama ready`
  and `qwen3-30b-a3b:latest`. The real `Choose a PDF` control opened the native
  file chooser, which selected `tests/fixtures/structured_report.pdf`; the UI
  then reported cancellable background processing.
- Under the unchanged 60-second request timeout and concurrent GPU load from a
  separate LM Studio `llama-server`, the first run durably failed during
  synthesis at state version 15. It showed a recoverable retry action and had no
  summary row, proving the release UI did not masquerade runtime failure as
  success. That separate process was not stopped or modified.
- The application was independently relaunched against the same profile with
  the existing `DOC_SUM_MODEL_TIMEOUT_SECONDS=300` deployment override. The
  failed record reappeared unchanged, its UI retry created a second run for the
  same document identity, and live Ollama processing reached
  `CompleteWithWarnings` at state version 18.
- The completed UI rendered 5 cited claims. Activating claim 1's evidence
  control displayed `EXACT SOURCE EXCERPT`, page 1, and the corresponding
  source text. After another independent release-process reopen, Recent Work
  restored both attempts and reopened the same completed summary.
- Before and after reopen, the completed artifact remained summary version
  `3.0.0` with SHA-256
  `7b36913e8b0a4362e1d06a57f96e75454d03e53a15d48d6557619a75f319a5db`;
  SQLite `quick_check` returned `ok`. This is a real native-picker, live-model,
  packaged-release, process-restart proof, not merely a database-connection
  reopen.

**Known limits and deferred work**:
- The successful live proof used the documented 300-second timeout override
  after the default-timeout failure. An unrelated LM Studio test remained
  GPU-resident throughout, but the generic runtime failure did not independently
  prove contention was its sole cause. The default timeout's behavior under an
  idle, fully GPU-resident model was not re-measured in this slice.
- Linux Debian packaging is the only installer format verified here. AppImage,
  RPM, macOS, and Windows packaging/install lifecycle tests remain deferred to
  their target environments. OCR/vision and additional document formats remain
  outside the native-text PDF v1 boundary.

## Slice 15 — Selected-Runtime Default and v1 Acceptance Closure

**Status**: Implemented; locally and cross-process verified

**Runtime contract and fix**:
- The selected Ollama/Qwen deployment previously shipped a 60-second generation
  deadline even though Slice 14's real desktop run required the documented
  300-second override. The supported default is now 300 seconds, so bounded
  multi-stage document work does not require hidden launch configuration.
- `DOC_SUM_MODEL_TIMEOUT_SECONDS` remains a deployment override. A pure boundary
  test proves the default, accepts the minimum positive value and `u64::MAX`,
  and rejects empty, zero, negative, fractional, non-numeric, and past-`u64`
  values.
- The change affects only model-generation requests. The separate three-second
  connection timeout, five-second health timeout, loopback-only endpoint rule,
  response-size limits, prompt bounds, and cooperative cancellation contract are
  unchanged.

**No-override live proof**:
- With `DOC_SUM_MODEL_TIMEOUT_SECONDS` explicitly absent, the ignored real
  Ollama service test ran the repository's native-text PDF through the complete
  pipeline using `qwen3-30b-a3b:latest`. It passed in 169.53 seconds, persisted
  the summary and exact citations, closed the database connection, and verified
  them after an independent reopen. This closes the exact default-timeout gap
  observed in Slice 14.

**Fresh package and current-consumer proof**:
- `npm run desktop:build` used a newly created empty Cargo target and completed
  in release mode. It produced the `document-summarizer` executable and one
  Debian bundle, `Document Summarizer_0.1.0_amd64.deb`.
- Email Watcher `origin/main` was fetched and exercised at commit
  `2a1596b200c339c4457a0358ac5e9a4e7748ed1b`. Its exact archived
  `connect-local-proof.py` and current source modules drove the extracted binary
  from that fresh Debian package. A launch wrapper removed the harness-provided
  timeout variable immediately before `exec`, forcing the packaged provider to
  use its shipped 300-second default.
- The synthetic Gmail-attachment proof observed capability availability
  `0 -> 1 -> 0 -> 1` across provider absence, launch, stop, and restart without
  changing Email Watcher. The live Qwen Connect job completed; Email Watcher's
  input SHA-256, persisted completed status, and persisted summary matched the
  response, and a separate connection to its schema-v5 database returned
  `quick_check = ok`.

**Provider persistence and integrity proof**:
- A second run retained the packaged provider's private application-data
  directory across process stop/restart. A new SQLite CLI process reopened
  schema version 12 with `quick_check = ok` and found one completed
  `CompleteWithWarnings` run at state version 18, all 18 ordered lifecycle
  events, and one summary plus one citation artifact.
- The stored summary-artifact SHA-256 recomputed exactly as
  `e3fc40ad24b368e729a13b31036684b1005ab7b4a0b7b8d27ad83f9ff0076fd9`;
  the citation-artifact hash recomputed exactly as
  `1ba9fab544758fc1c8d9539098dd2c0afd840f4b7abb5607c3f497bc284b34e4`.
  The provider-owned PDF and repository fixture were both 5,443 bytes and both
  hashed to
  `34aa217d7a21a0ec31abd90d0d7700d42924f334ce107912850398ac3f23d7ea`.

**Regression gate**:
- `cargo test --all-targets --all-features` ran 154 library tests: 152 passed,
  none failed, and the two opt-in live Ollama tests were ignored at this gate;
  the release-contract target then passed all three tests. The full real Ollama
  pipeline test passed separately with the default timeout.
- `cargo fmt --all -- --check`, strict all-target/all-feature Clippy, the
  TypeScript/Vite production build, `git diff --check`, and the fresh-target
  Debian package build all passed.

**Evidence boundary and deferred work**:
- Slice 14 already exercised the real native file picker, rendered citations,
  retry, and process reopen. This slice exercised the current cross-app consumer
  with synthetic Gmail attachment bytes; it did not perform live Gmail OAuth or
  install/uninstall the Debian package through the system package manager.
- A cooperative cancellation request may still wait for the current bounded
  model HTTP request to return; request preemption/streaming is deferred. Linux
  Debian remains the only verified bundle target. OCR/vision, additional source
  formats, remote inference, workflow automation, and cross-machine Connect
  remain outside the native-text PDF v1 release boundary.

## Office Document Acceptance and Paid-Connect Boundary (2026-08-30)

**Status**: Deterministic public-corpus and one-page live completion proofs
passed; full multi-page live completion remains blocked by concurrent GPU load

**Commercial distinction**:
- "Optional Connect" means failure isolation: Document Summarizer and Email
  Watcher retain their standalone behavior when Connect or a peer application
  is unavailable. It does not mean Connect is a free feature or a user setting.
- The intended sellable behavior requires a paid Connect entitlement before an
  application advertises/discovers cross-app capabilities or admits Connect
  jobs. Current v1 automatically advertises when provider startup succeeds, so
  entitlement enforcement is not implemented and remains a required product
  slice.
- Google OAuth remains exclusively owned by Email Watcher. It retrieves the
  selected attachment and hands Connect explicit PDF bytes plus bounded
  metadata; Document Summarizer receives no Gmail token, mailbox access, or
  Email Watcher database path.

**Real-document harness and deterministic proof**:
- Added ignored, path-configured integration tests for deterministic checkpoints,
  isolated live analysis, and complete live Ollama summary/citation persistence.
  Raw model and summary content is hidden unless a dedicated trace variable
  opts in; default reports expose only lengths and hashes.
- Downloaded but did not commit five public documents: IRS Form W-9, a rotated
  DOL minimum-wage poster, Grand County independent-contractor agreements, a
  scanned/OCR NARA records schedule, and a 111-page DOL training deck. Their
  source URLs, hashes, purposes, and reproducible commands are recorded in
  `docs/OFFICE_ACCEPTANCE.md`.
- All five passed ingestion through chunking with unchanged source bytes, page
  topology, exact normalized-block accounting, provenance, ordered events, and
  equal artifacts after an independent SQLite connection reopen. The NARA
  schedule retained page 9 as visual-processing-required with
  `NO_NATIVE_TEXT`; no page or source block was invented or silently dropped.

**Defects exposed and fixes applied**:
- The first W-9 analysis failed because Qwen normalized a PDF line-wrap newline
  to a space in an otherwise verbatim quotation. A contractor agreement also
  exposed `non-\nbreaching` becoming `non-breaching`. Analysis now reconciles
  only layout whitespace, including whitespace immediately after an existing
  hyphen, then stores the literal substring from the authoritative normalized
  block. Punctuation, case, missing or reordered words, ordinary fused words,
  foreign IDs, duplicates, and over-limit output still fail closed.
- The original analysis contract allowed 64 generated evidence items and 2,048
  output tokens. A dense W-9 page drove a request past the 300-second deadline.
  An initial eight-item/768-token cap then truncated the poster JSON during its
  sixth item. The aligned generation contract now requests at most five items,
  shortest sufficient quotations, and a 1,024-token budget while the persisted
  artifact validator retains its wider historical compatibility.
- The selected imported model rejects the full server-side schema with its
  documented vocabulary error. The exact-error fallback now uses Ollama JSON
  mode rather than unconstrained output. All generation requests use
  temperature zero, fixed seed `42`, and no reasoning effort. Rust validation
  remains authoritative.
- Dense contractor text exposed occasional invalid JSON, altered quotations,
  and a cross-page quote falsely associated with one block. Analysis now allows
  one complete replacement request under a stricter one-block/one-passage
  prompt. Rejected evidence is never partly committed; successful replacement
  is warned, while a second invalid response or runtime failure fails the stage.

**Tests and live evidence**:
- The final full Rust run completed with 158 passing library tests, no failures,
  and two opt-in live Ollama tests ignored; all three release-contract tests
  passed. The office harness added one passing default-log privacy probe while
  its three external-document tests remain intentionally opt-in. Strict Clippy,
  the frontend production build, and the release-mode Tauri no-bundle build also
  passed.
- Focused tests cover exact, whitespace, and post-hyphen line-wrap quote
  resolution; changed/fused-text rejection; source-exact persistence; duplicate
  variants; empty/max/max-plus-one evidence counts; JSON fallback shape; fixed
  seed/no-reasoning payloads; successful whole-response repair; persistent
  malformed output; and no retry for runtime failure.
- The five-document deterministic harness passed again on the final code. It
  preserved 6, 1, 8, 12, and 111 pages respectively, retained the NARA visual
  page 9 with `NO_NATIVE_TEXT`, accounted for every normalized block, and
  reproduced all artifacts/events after independent SQLite connection reopen.
- With the final bounds, the public minimum-wage poster completed the full live
  pipeline in 395.07 seconds with five evidence items, five supported claims,
  state version 18, and 18 events. Exact citation and summary artifacts survived
  independent connection reopen.
- A seeded two-chunk contractor analysis completed in 349.67 seconds with ten
  exact evidence items. A later full contractor run failed safely after 406.48
  seconds: the first response was contract-invalid and its one repair request
  returned `MODEL_RUNTIME_UNAVAILABLE`; no full contractor or W-9 completion is
  claimed.
- These timings are not release performance proof. During the latest failure,
  Ollama reported 78% CPU / 22% GPU while the separate LM Studio evaluation
  retained most GPU memory. No unrelated process or file was stopped, moved, or
  deleted.

**Remaining gate**:
- After the separate GPU evaluation ends, rerun complete W-9 and
  contractor-agreement live tests with uncontended Ollama; inspect coverage,
  citations, and latency. The poster is already a full live completion proof.
- The next product slice is entitlement-gated capability discovery/admission,
  with provider-present, provider-absent, entitlement-denied, and restoration
  proofs. Billing UX, workflow automation, OCR/vision, and broader document
  formats remain deferred.

## Real-Document Acceptance and Desktop Launch Repair (2026-08-30)

**Status**: Verified locally

**Violations found and repaired**:
- A real `npm run desktop:dev` attempt from a clean worktree failed before its
  window opened. Application configuration forced the production
  `custom-protocol` feature during development, so Tauri selected embedded
  assets and rejected the absent ignored `dist` directory.
- Removed the mode override from `tauri.conf.json` and restored
  `custom-protocol` as a Cargo default feature. Tauri development can now invoke
  Cargo with `--no-default-features` and use `devUrl`, while Tauri release builds
  retain the feature and embed `frontendDist`. A regression test pins both sides
  of that contract.
- A realistic long attachment filename overflowed the processing workbench and
  displaced the recent-work column. The workbench children may now shrink, and
  filename fields wrap without changing the authoritative stored filename.

**Real application and document proof**:
- The external deterministic office harness passed for a public W-9, a rotated
  minimum-wage poster, contractor agreements, a scanned NARA PDF, and a
  multi-page DOL slide deck. The scanned fixture retained its visual-routing
  marker instead of inventing text.
- A private ten-page PDF selected through the real file picker reached live
  model analysis. Closing the application during analysis and reopening it
  created the documented retryable `PROCESS_INTERRUPTED` failure rather than a
  false completion; the UI exposed recovery and retry.
- A protocol-v2 job submitted the public DOL poster with the extensionless
  display name `minimum-wage-poster`. The live
  `qwen3-30b-a3b:latest` runtime completed it after the provider's bounded JSON
  fallback. Connect returned one summary artifact whose declared byte size and
  SHA-256 matched the decoded bytes, while SQLite held a `Complete` pipeline run
  and its summary artifact.
- After closing and independently restarting the Tauri process against the same
  isolated application-data directory, the prior Connect job remained
  `completed` and returned the identical output bytes and hash.
- Email Watcher's protocol-v2 discovery returned one capability while the
  provider was live, zero after it stopped, and one from the new instance after
  restart, with no Email Watcher code change. Its focused Connect test module
  also passed.

**Build and boundary proof**:
- A clean development launch opened through `npm run desktop:dev` without a
  pre-existing `dist`; command output showed Tauri using
  `cargo run --no-default-features`.
- `npm run build`, `cargo fmt --check`, the full Rust suite, and strict Clippy
  passed. The Rust suite reported 169 passed and two live-runtime tests ignored;
  the default office suite reported one passed and three opt-in tests ignored;
  all three release-contract tests passed.
- `npm run desktop:build:no-bundle` completed and emitted the optimized desktop
  executable. The opposite boundary probe ran
  `cargo build --release --no-default-features` and observed the intended
  compile-time rejection with Cargo exit 101.
- Visual inspection of the fresh-process retry view confirmed the long
  filename remained within the workbench and did not cover the recent-work
  column.

**Known limitation / deferred hardening**:
- Terminal-driven Tauri shutdowns left owner-only but unreachable provider
  registration files. Email Watcher's live authenticated-manifest discovery
  ignored every dead entry, so capability removal/restoration remained truthful.
  Proactive stale-file scavenging remains deferred Connect lifecycle hardening.
- The combined Email Watcher Connect/engine test invocation was not exercised in
  the detached v2 worktree because that interpreter lacked `google.auth`; the
  self-contained Connect tests and live discovery path were exercised instead.

## Connect Registration Lifecycle Cleanup (2026-08-31)

**Status**: Verified locally

**Lifecycle contract and implementation**:
- Provider startup examines only exact Document Summarizer registration names
  with UUIDv4 instance IDs in the v1 and v2 directories. Reads, responses, and
  loopback probes are bounded. A registration is considered live only when its
  bearer-authenticated protocol-specific manifest returns the same protocol,
  app ID, and instance ID.
- Startup refuses to replace a matching live provider. If no candidate proves
  live, exact app-owned stale or malformed registrations are removed and both
  directories are synced before the new endpoint is published. Foreign and
  merely similar filenames remain untouched.
- Tauri now invokes idempotent unregister logic on final `RunEvent::Exit`.
  Publication, scavenging, and cleanup share a bounded owner-only lifecycle
  lock; deletion then requires the on-disk protocol, instance ID, endpoint, and
  bearer token to match the exiting process. This closes the replacement race
  at protocol v2's stable registration path. `Drop` retains the same guarded
  cleanup for non-Tauri provider ownership.
- No Connect wire shape, job behavior, entitlement behavior, pipeline state,
  database schema, summary semantics, frontend behavior, or model runtime
  changed.

**Boundary and automated proof**:
- Focused tests passed for dead v1/v2 and malformed owned registration cleanup,
  foreign/similar filename preservation, a live authenticated provider blocking
  replacement, cleanup blocking behind replacement publication, replacement
  lease protection, and repeated unregister calls.
- The full Rust run completed with 174 passing library tests and two opt-in live
  Ollama tests ignored. The default office suite passed its privacy test with
  three external-document tests ignored; all three release-contract tests
  passed. Strict all-target/all-feature Clippy, Rust formatting, the frontend
  production build, and the no-bundle Tauri release build passed.

**Real process proof**:
- Before launch, the real runtime registry contained 14 dead v1 and five dead
  v2 Document Summarizer files. The patched Tauri startup reported removal of
  all 19, then published exactly one mode-`600` registration per protocol.
  Each PID was live and each authenticated manifest matched its registration's
  instance ID and `document-summarizer` app identity.
- Closing the real `Document Summarizer` window through the desktop window
  manager produced a successful process exit. Independent inspection found zero
  Document Summarizer registrations in both protocol directories afterward.
- After the concurrency repair, the final head repeated the live launch/close
  path with an owner-only mode-`600` lifecycle lock. Both authenticated
  registrations matched while running, both were absent after exit, and the
  non-advertising lock file remained for future process coordination.

**Known boundary**:
- An uncatchable termination or power loss cannot execute process-exit cleanup.
  Such residue remains non-authoritative to discovery and is deterministically
  reclaimed on the next provider startup. No signal daemon, broker, or generic
  third-party registration garbage collector was added.

## Slice 16 — Paid Connect Entitlement: Provider Boundary (2026-08-31)

**Status**: Provider implementation, deterministic tests, and test-authority
two-process proof complete; official production issuer-key provisioning and
release-package proof remain release work

**Commercial and trust contract**:
- Standalone document ingestion, processing, saved results, and recovery remain
  available without Connect. Connect is not a free toggle: manifest discovery,
  job submission, and job status require a signed entitlement containing
  `connect.capability_exchange`.
- The provider verifies an Ed25519 signature over the exact payload bytes using
  an issuer key ring embedded only at build time. A missing key ring fails
  Connect closed without preventing standalone startup. Runtime configuration
  cannot substitute a new trust root, and no private issuer key is committed.
- The verified Linux file boundary is the current user's private
  `local-connect/entitlement-v1.json`. Owner, mode, regular-file, no-symlink,
  size, schema, canonical base64url, UUID, feature, signature, and exact UTC
  interval checks all fail closed. There is no grace period.

**Provider behavior**:
- Bearer authentication precedes entitlement evaluation. Both protocol-v1 and
  protocol-v2 manifest, submission, and status routes return the same bounded
  `CONNECT_ENTITLEMENT_REQUIRED` error while denied, before parsing an artifact
  or opening the job database. Malformed multipart metadata cannot bypass that
  ordering.
- The provider process and its registration remain live while denied. Startup
  liveness probing recognizes only the authenticated entitlement-required
  envelope as proof of ownership, preventing a second instance from scavenging
  a live denied provider. Discovery still omits the capability because no
  manifest is returned.
- Replacing an expired entitlement with a valid signed file restores the
  capability without provider restart. Expiry does not delete or mutate
  pipeline artifacts; standalone access to earned results is unchanged.

**Automated proof recorded before commit**:
- Canonical entitlement conformance passed against the pinned
  `connect-contracts` revision, including active, expired, not-yet-valid,
  missing-feature, unknown-key, bad-signature, signed duplicate-claim-member,
  and malformed-base64 fixtures.
- Focused Rust tests passed for exact time boundaries, signature tampering,
  feature denial, private-file requirements, symlink rejection, key-ring and
  path boundaries including empty-XDG fallback and an empty build-keyring
  variable, both wire-version route gates, malformed-multipart ordering, denied
  submission/status, expiry inside the write transaction with no committed
  job/import, live-registration ownership, and in-process entitlement restoration.
- A release-mode provider compiled with only the canonical test public key ran
  against Email Watcher's real Connect/persistence code and a deterministic
  local model fixture. A PDF job completed; replacing the entitlement with the
  canonical expired fixture removed consumer discovery and made the live
  provider return `CONNECT_ENTITLEMENT_REQUIRED`; the completed caller-owned
  result remained readable without Gmail access; restoring the active signed
  fixture returned the capability without restarting either process. This was
  a test-authority process proof, not an installed production-package or live
  Gmail/UI proof.
- The exact final provider and consumer heads repeated the two-process proof
  through the real Ollama endpoint with `qwen3-30b-a3b:latest`. The job
  completed, the active/expired/restored and removal/restart checks passed, and
  the proof reported `proof_passed: true`.

**Known limits and deferred work**:
- The provider proof uses an injected test authority. An official package must
  embed the production public key and receive a separately issued entitlement
  before Connect can be sold; issuer private-key custody and license delivery
  remain outside this repository.
- The current secure entitlement-file reader is implemented for the verified
  Linux/Unix boundary. Windows ACL/path hardening and target-host packaging
  proof remain required before a Windows release.
- Device binding, online revocation, billing/account UI, clock-rollback defense,
  shared Gmail authorization, workflows, OCR, and model/runtime changes remain
  deferred.

## Slice 17 — Connect Entitlement Activation: Provider (2026-08-31)

**Status**: Document Summarizer activation boundary, desktop presentation,
deterministic rollback probes, and final Debian-bundle launch proof complete;
cross-app package-manager installation and production issuer custody remain
deferred

**Activation contract**:
- The implementation is pinned to the accepted Connect activation decision and
  entitlement fixtures at canonical `connect-contracts` revision
  `c5405935bd1354cf6a4c8539425a53dfd7f52949`.
- The core exposes a claim-free status containing only the stable license state
  and active boolean. The Tauri commands and file picker remain adapters; no
  signature, filesystem, or commercial decision moved into TypeScript.
- Install reads at most the bounded entitlement size from a regular source file
  without following a final symlink, applies the live signature/claim/feature/
  time verifier before mutation, and derives the shared destination internally.
  Only a currently active entitlement is admitted; the selected source is not
  modified.
- All Unix participants coordinate on the persistent owner-private
  `.entitlement-v1.lock`. Under a non-blocking exclusive lock, the provider
  rechecks the candidate time boundary, writes and syncs a unique mode-`600`
  same-directory file, atomically replaces the entitlement, syncs the
  directory, and re-evaluates the installed file before reporting active.
- Stable structured failures distinguish unavailable authority, invalid source,
  inactive source, unsafe storage, held activation lock, and install failure.
  Expected failures before replacement leave an existing entitlement
  byte-for-byte unchanged and remove temporary output.

**Desktop behavior**:
- The header now presents Ollama and Connect as separate availability cards.
  Connect shows active, missing, invalid, future, expired, feature-missing, and
  no-authority states without exposing claims. A user can activate or replace a
  license through the existing least-privilege open-file permission.
- A real 1180-by-780 virtual-display capture found and corrected an intrinsic
  grid-width defect that initially pushed the activation button outside its
  card. The final capture showed both cards, contained ellipsis, and the visible
  activation action without changing the document workbench or history rail.

**Verification exercised**:
- Focused installer tests passed for valid exact-byte installation, source
  preservation, mode-`600` output and lock, claim-free serialization, stable
  errors, independent gate reopen, invalid/expired/not-yet-valid/missing-feature
  rejection, source-symlink rejection, insecure-destination rejection,
  cross-process lock contention, exact modes under a restrictive process
  `umask`, injected pre-replacement failure, rollback, and temporary-file
  cleanup.
- The full Rust suite passed in both default-feature and featureless modes.
  Strict Clippy, Rust formatting, the TypeScript compiler, and the production
  Vite build passed. The opt-in canonical entitlement fixture test passed
  against the pinned revision after fetching that exact contract commit.
- The final Connect-enabled no-bundle release and Debian bundle both built with
  only the canonical test public key ring. A scan of the final packaged binary
  found the expected test key ID and public key and no private-key PEM marker.
- The final Debian package was extracted to an isolated root and its packaged
  executable was launched with isolated XDG data, config, and runtime paths.
  It registered a live provider and an authenticated manifest request returned
  `CONNECT_ENTITLEMENT_REQUIRED` with no license. The process was stopped with
  `Ctrl-C`; this is an extracted-package interrupted-process proof, not a
  package-manager install/uninstall or graceful GUI-close proof.

**Known limits and deferred work**:
- The package contains a canonical test authority, not a production public key.
  Production issuer private-key custody, customer license acquisition, and
  payment/account UI remain separate release work.
- The matching Email Watcher activation boundary and a two-installed-app proof
  are required before this commercial activation slice is complete.
- Windows ACL/path semantics, advance-dated renewal staging, online revocation,
  machine binding, clock-rollback defense, and cross-machine Connect remain
  deferred.

## Slice 18 — Connect Entitlement Activation Consistency Hardening (2026-08-31)

**Status**: Provider activation durability, race resistance, boundary probes,
and signed-package process smoke complete at the local release gate

**Violations found and repaired**:
- Candidate, installed-entitlement, and activation-lock opens did not include
  `O_NONBLOCK`. A metadata/open race could therefore replace a checked regular
  file with a FIFO and block the activation path. Those opens now combine
  non-following, close-on-exec, and non-blocking flags, then verify descriptor
  type, identity, owner/mode where required, and bounded size after open.
- A candidate was already visible after atomic rename when directory sync or
  final entitlement validation failed, but the installer returned failure
  without restoring the prior durable state. Activation now snapshots the
  existing bytes while holding the shared lock and tracks whether promotion
  occurred. Every detected post-promotion failure atomically reinstalls and
  verifies the prior bytes, or removes the new candidate and syncs the directory
  when no prior entitlement existed. A rollback failure remains a stable
  `CONNECT_ENTITLEMENT_INSTALL_FAILED`, never success.
- Installation accepted owner-private directories that lacked owner read or
  search permission, including mode `0300`, and recursively created directory
  entries were not explicitly made durable. The destination directory now
  requires exact permission bits `0700`; each missing entry is created and
  validated at that mode, then both it and its parent are synced before use.

**Failing-before and boundary proof**:
- Before the repair,
  `existing_private_directory_requires_exact_0700` failed because activation
  created a destination under mode `0300` instead of rejecting it.
  `final_validation_failure_restores_existing_entitlement` also failed because
  the replacement bytes remained after activation returned `InstallFailed`.
- After the repair, the focused entitlement module passed 18 tests with the
  canonical external-fixture test intentionally ignored at that gate. Probes
  cover valid exact-`0700` creation and invalid `0300`, pre-promotion failure,
  final-validation failure, and post-promotion failure, with both prior-license
  and no-prior-license cases. A descriptor-level `fcntl(F_GETFL)` probe confirms
  guarded reads and the lock retain `O_NONBLOCK`.
- The canonical entitlement-v1 fixture test passed separately against revision
  `c5405935bd1354cf6a4c8539425a53dfd7f52949` and its test key ring.

**Regression and release proof**:
- Both default-feature and featureless Rust suites passed. Each library run had
  194 passing and four ignored tests; the office target had one passing and
  three ignored tests; all three release-contract tests passed. Strict Clippy
  passed in both modes, Rust formatting passed, and the TypeScript/Vite
  production build passed.
- Connect-enabled no-bundle and Debian release builds passed using only the
  canonical test key ring. The resulting
  `Document Summarizer_0.1.0_amd64.deb` was 8,597,772 bytes with SHA-256
  `ffa336e8761ecc3bac892e094f151014a5ba999e556aedbab640f14306a1fe5a`.
- The package was extracted under isolated XDG data, config, cache, and runtime
  roots with the canonical active entitlement. Its packaged executable
  registered protocol v1 and returned app ID `document-summarizer` with
  capability `document.summarize` through an authenticated manifest request.
  The process was then interrupted with `Ctrl-C`; this proves packaged startup
  and the signed provider boundary, not package-manager installation or graceful
  GUI close.
- Email Watcher's matching activation hardening was reviewed and merged in PR
  #67 at merge commit `816b6c2d93566cac73d790bd2eeeac6d37c4b044` after its
  exact-head local gates and installed-package boundary proof passed with no
  unresolved review threads.

**Unchanged boundaries and deferred work**:
- No Connect wire shape, job behavior, pipeline/database state, frontend
  behavior, standalone entitlement independence, summary semantics, or Ollama
  runtime behavior changed.
- The package still contains the canonical test authority rather than a
  production issuer public key. Production issuer custody and license delivery,
  package-manager install/uninstall, live Gmail/human-click acceptance, Windows
  ACL/path and package proof, online revocation, machine binding, clock-rollback
  defense, and cross-machine Connect remain deferred.

## Slice 19 — Installed Two-App Connect Acceptance (2026-08-31)

**Status**: Real Gmail attachment, installed-package discovery, live Ollama
summarization, durable caller result, provider removal, and provider restoration
were exercised end to end on Ubuntu

**Installed two-app proof**:
- Email Watcher and Document Summarizer were installed as independent Debian
  packages and launched with an isolated owner-private XDG profile. Email
  Watcher reused its configured Gmail authorization only inside its own process;
  neither the credential store nor mailbox access was exposed through Connect.
- A real message containing the existing structured-report PDF fixture was sent
  through the configured Gmail profile and then collected through Email
  Watcher's normal check path. The inbox displayed the PDF attachment and added
  `Summarize` only after generic discovery found an entitled provider for
  `document.summarize` accepting `application/pdf`.
- Invoking that contextual action transferred the explicitly selected PDF
  artifact through Connect v2. Document Summarizer imported it into provider-
  owned storage and ran the existing parse-to-citation pipeline against the
  loopback Ollama runtime with `qwen3-30b-a3b:latest`.
- The final installed-package run reached `CompleteWithWarnings` at state
  version 18 with analyzed, synthesized, verified, summary, and citation
  artifacts present. Its provider job and Email Watcher caller job both reached
  `completed`, and the Email Watcher UI displayed the returned `summary.json`
  output. The native-text warning was preserved rather than hidden.

**Defect found and repaired**:
- The former 300-second per-generation default was too short for the accepted
  model and realistic multi-stage document work. A live synthesis request
  exhausted that deadline and returned the existing structured retryable
  `MODEL_RUNTIME_UNAVAILABLE` failure; neither app crashed and neither caller
  nor provider falsely recorded success.
- Re-running with an explicit 900-second deployment override completed the same
  workflow. The supported built-in generation deadline is now 900 seconds, and
  the README and runtime contract state that value. Connection and health
  checks remain independently fail-fast.
- A rebuilt installed package was then launched without
  `DOC_SUM_MODEL_TIMEOUT_SECONDS` and completed a fresh end-to-end job. That
  final run did not itself exceed 300 seconds in either synthesis or
  verification, so it proves no-override installed behavior but is not claimed
  as a live boundary-duration test. The focused configuration test proves the
  default selected by the environment loader is 900 seconds.

**Removal, restoration, and standalone behavior**:
- Stopping the provider caused capability discovery to return no compatible
  provider while Email Watcher's inbox and previously completed result remained
  usable. Removing only the `document-summarizer` Debian package left the
  installed Email Watcher process healthy and the contextual action absent.
- Reinstalling the rebuilt Document Summarizer package and relaunching it caused
  the same unchanged Email Watcher build to rediscover exactly one compatible
  `document.summarize` capability and restore the action. No caller database or
  code change was used to manufacture either state.

**Durability and verification**:
- Both database-owning desktop processes were closed and independently
  relaunched after the fixed run. Document ID, source size and hash, pipeline
  run ID, state/version, all five durable artifact hashes, provider result hash,
  caller result hash, and both completed statuses were identical after reopen.
  This is an installed-process reopen proof, not a machine reboot proof.
- The complete Rust suite passed in default-feature and featureless modes;
  strict Clippy passed in both modes, Rust formatting passed, and the frontend
  production build passed. Both ignored canonical Connect entitlement/provider
  fixture tests passed explicitly against the external contract checkout.
- A Connect-enabled Debian package rebuilt successfully with only the canonical
  test authority. The final bundle was 8,597,766 bytes with SHA-256
  `f37738be749548df2d018da71c70944a318f11c1809bd220c1e71360f25d5783`.
  The installed executable was byte-identical to the exact executable extracted
  from that package; Debian's packaging transformation means it was not
  asserted to match the pre-bundle target byte for byte.
- The final UI proof ran on the host X display after the temporary Xvfb harness
  exited. Actions were driven through the native desktop UI with automation;
  this is not described as a human-manual click test.

**Known limits and deferred work**:
- The package still embeds the canonical test authority, not a production
  issuer public key. Production issuer custody and customer license delivery
  remain release work.
- Windows/macOS package, ACL, and cross-app acceptance; machine reboot recovery;
  larger and scanned-document corpus runs; and Connect polling/log-volume
  hardening remain deferred. The polling noise did not change the completed
  result or either application's standalone behavior.
- No Connect protocol shape, pipeline transition, summary schema, Gmail
  ownership boundary, frontend product role, or reverse-discovery behavior was
  changed in this slice. `docs/PIPELINE_STATE_MACHINE.md` therefore remains
  unchanged.

## Slice 20 — Real Office Corpus Live Acceptance Closure (2026-09-01)

**Status**: The previously unproven public W-9 and contractor packets, plus the
partial-text NARA fixture, completed the full local Ollama summary/citation path
with durable reopen proof

**Contract violations found**:
- A contractor repair response altered a source sentence. Repeated W-9 repairs
  also dropped a qualification or attached familiar text from another page to
  the current block. The exact-quote validator rejected every case before an
  analyzed artifact or success state could be committed.
- Repeating the source-copy prompt was not a reliable repair boundary. An
  initial deterministic quote catalog serialized to 51,063 user-prompt
  characters and exceeded the practical budget of the configured 8,192-token
  context, causing a contract-invalid model response.
- When Ollama's schema grammar was unavailable, one intermediate synthesis
  response returned eight claims although the application schema and parser
  allowed four. The per-request bound existed in the schema but was absent from
  the JSON prompt seen by fallback mode. Rust rejected the oversized response.

**Fixes applied**:
- Primary evidence extraction and its strict source validator are unchanged.
  The one permitted repair now selects from an application-built catalog of
  exact normalized-source quotations. Each candidate has a deterministic quote
  ID and fixed block provenance; the model returns only quote IDs and claim
  text. Rust rejects foreign or duplicate IDs and materializes quotation bytes,
  block/page provenance, and evidence identity itself.
- The repair catalog is bounded to 48 candidates and 12,000 quotation
  characters. On the W-9, final repair requests measured 13,752 and 13,903
  serialized user-prompt characters and passed. A successful replacement keeps
  the existing durable `MODEL_EVIDENCE_RESPONSE_REPAIRED` warning.
- Both evidence and candidate synthesis envelopes now include the exact dynamic
  `maximum_claims` represented by their JSON Schema. The Rust bounds were not
  raised, and malformed, oversized, foreign-ID, or duplicate-ID output still
  fails closed.
- The office harness now records request schema/size metadata without exposing
  prompts, and reports visual-page and cited-page sets. It asserts that a
  visual-only page is never cited as native text.

**Tests and live evidence**:
- The summary module ran 44 tests: 43 passed and the explicit live hierarchical
  Ollama test was ignored. New probes cover deterministic/bounded source quote
  catalogs, exact source containment, foreign and duplicate selections, the
  one-repair persistence path, and prompt/schema synthesis-limit agreement.
- Default-feature and featureless all-target Rust suites each passed with 198
  library tests and four intentional external/live ignores; the office target
  passed its privacy test with three opt-in ignores, and all three release-
  contract tests passed. Strict Clippy passed in both modes, Rust formatting
  passed, and the TypeScript/Vite production build passed.
- Both ignored canonical Connect tests passed explicitly against the mounted
  external entitlement-v1/provider-v2 contract checkout.
- The final five-document deterministic corpus run passed. It preserved 6, 1,
  8, 12, and 111 pages; every run reached `CHUNKED`, state version 11, with 11
  events and equal artifacts after an independent SQLite connection reopen.
  NARA page 9 remained visual-only with `NO_NATIVE_TEXT`.
- The six-page W-9 completed the full live pipeline in 60.85 seconds with 6
  supported claims, 6 cited evidence items, state version 18, 18 events, and
  `MODEL_EVIDENCE_RESPONSE_REPAIRED`. The eight-page contractor packet completed
  in 34.71 seconds with the same claim/evidence/state/event counts and repair
  warning.
- The twelve-page NARA fixture completed in 32.42 seconds with 6 supported
  claims and 6 cited evidence items. Page 9 remained visual-only; accepted
  citations referenced pages 1 and 5. `NO_NATIVE_TEXT` and the repair warning
  survived reopen.

**Persistence and unchanged boundaries**:
- Every live acceptance run reloaded equal summary, citation, and immutable
  event artifacts through a new SQLite connection and re-read source bytes
  unchanged. Failed model contracts never reached `ANALYZED`, `SYNTHESIZED`, or
  a terminal success state. This is database-connection reopen proof, not a
  desktop-process or machine-reboot claim.
- No artifact schema/version, database migration, pipeline state transition,
  Connect contract, frontend behavior, parser, model choice, OCR/vision path, or
  Email Watcher code changed. `docs/PIPELINE_STATE_MACHINE.md` remains accurate
  and unchanged.

**Deferred**:
- The live timings are warm-model correctness observations, not performance
  benchmarks. OCR/vision and citations for visual-only content remain deferred;
  visual-only pages are retained and explicitly excluded from native-text
  citations.
- Production issuer custody/customer license delivery, Windows/macOS package
  acceptance, and machine-reboot recovery remain outside this corpus closure.

## Slice 21 — Constrained-Decoder Structured-Output Compatibility Hardening (2026-09-02–03)

**Status**: vLLM and Ollama decoder compatibility, application validation, and
live pipeline persistence proof complete

**Defect and boundary repair**:
- vLLM 0.28.0 rejected the former synthesis response schema before generation
  with HTTP 400 and `Unimplemented keys: ["uniqueItems"]`. The unsupported
  keyword appeared on both evidence-ID and hierarchical candidate-ID arrays.
- Ollama 0.24.0 rejected the analysis schema because 2,000- and 4,000-character
  `maxLength` values expanded into grammar repetitions beyond its sane limit.
  Its exact HTTP-500 compatibility response moved the runtime to generic JSON
  mode. A real four-page office PDF then failed once when synthesis returned an
  empty claim set and once when analysis returned a non-source quotation even
  after its bounded repair request.
- The runtime adapter now derives a decoding copy of every canonical schema and
  recursively removes only schema-keyword occurrences of `uniqueItems` and
  `maxLength`. Property names and ordinary metadata with those spellings remain
  intact. Required fields, object closure, enums, non-empty strings, and array
  count bounds still reach the decoder. The caller's canonical schema is not
  mutated.
- Rust remains authoritative after generation. Direct and hierarchical parsers
  still reject oversized strings, non-source quotations, duplicate, foreign,
  empty, cross-batch, and over-limit references before an artifact or success
  transition can persist. This is a decoder vocabulary accommodation, not a
  relaxation of the durable application contract.

**Live vLLM effect proof**:
- A direct request using the former `uniqueItems` schema reproduced HTTP 400.
  The equivalent bounded schema without that keyword returned HTTP 200 and
  valid structured JSON from `qwen3.8-27b-awq`; the same patched shape returned
  HTTP 200 and valid structured JSON from `ministral-3-14b-awq`.
- The ignored real-pipeline test ran the repository PDF through ingestion,
  parsing, normalization, structure, chunking, analysis, synthesis,
  verification, summary/citation persistence, and independent SQLite reopen.
  It passed against Qwen in 1059.52 seconds and against Ministral in 37.26
  seconds. These are correctness observations from one warm local machine, not
  comparative performance claims.
- The local vLLM runtime initially failed during FlashInfer sampler JIT because
  its CUDA/CUB headers did not provide the expected `FlagHeads` member. The
  vLLM-supported `VLLM_USE_FLASHINFER_SAMPLER=0` setting selected the native
  sampler; both models then started and completed the proofs. No package or
  application dependency was changed to hide that deployment constraint.

**Live Ollama failure and recovery proof**:
- The installed application preserved both failed `output_test.pdf` attempts as
  immutable failures. Both retained parse, normalization, structure, and chunk
  artifacts; the first retained its valid analysis artifact but no synthesis,
  while the retry retained no invalid analysis or later artifacts.
- A direct request using the projected analysis schema returned HTTP 200 rather
  than Ollama's grammar error. The patched ignored office acceptance test then
  processed the same source through the full pipeline against
  `qwen3-30b-a3b:latest`, committed five supported summary claims with five
  exact evidence quotations, reached `CompleteWithWarnings` at state version
  18, and retrieved identical summary, citation, event history, and unchanged
  source bytes after an independent SQLite reopen. All patched-run schema
  requests returned HTTP 200 without activating generic JSON fallback.
- The patched Debian package was installed and its desktop binary retried the
  same `output_test.pdf` source through the user-facing application path. That
  run reached `CompleteWithWarnings` at state version 18 with 18 immutable
  events, seven claims, seven evidence records, and both summary and citation
  artifacts. After terminating and independently relaunching the installed
  process, the same run, version, event count, artifact hashes, and claim and
  evidence counts remained retrievable. The source file SHA-256 still matched
  the durable document hash.

**Regression and architecture proof**:
- The adapter projection regression covers nested object, array, and composition
  schemas, preserves domain property names and metadata that resemble removed
  keywords, proves the canonical input is unchanged, and retains required
  structural bounds. Existing negative boundary tests continue to lock strict
  application rejection behavior.
- The focused runtime-adapter suite reported 10 passing tests. Default-feature
  and featureless all-target suites each reported 199 passing and four
  intentionally ignored library tests; the office target reported one passing
  and three opt-in ignores, and all three release-contract tests passed. Rust
  formatting, strict Clippy in both feature modes, the TypeScript/Vite
  production build, and `git diff --check` passed.
- No prompt, persisted artifact schema/version, database migration, pipeline
  transition, Connect contract, parser, frontend behavior, default Ollama
  deployment, or model selection changed. `docs/PIPELINE_STATE_MACHINE.md`
  remains accurate and unchanged.

**Deferred**:
- The exact-loopback vLLM experiment reused `OllamaRuntime` identity, so it is
  historical compatibility evidence only and not truthful production runtime
  provenance. Current product direction remains a gateway-owned Ollama worker
  with the shared Qwen profile. Provider-neutral runtime provenance, if another
  runtime is supported later, remains separate work.
- The vLLM/FlashInfer/CUDA setting proves only that the historical local
  experiment could run on this machine. This slice does not select vLLM, add a
  vLLM packaging requirement, or establish a cross-machine runtime path.

## Slice 22 — Coherent Source-Aware General Summary (2026-09-07)

**Status**: implementation, automated gates, and public-fixture live acceptance
complete in PR #38

**Verified defect and change boundary**:
- Current production synthesis copied every analyzed paraphrase into a rendered
  claim ledger. Exact citations survived, but no stage used the ordered
  normalized source to compose a reader-facing overview.
- This slice adds General synthesis only. Story, Contract, profile selection,
  automatic routing, parser/OCR behavior, model selection, Connect wire
  contracts, and unrelated UI redesign remain outside the boundary.

**Implemented behavior**:
- Current synthesis builds an ordered exact-source catalog from normalized
  chunks in canonical chunk, block and within-block segment order, then asks for
  a bounded coherent overview with request-local source IDs. Rust owns durable
  evidence, claim IDs, canonical source order and page labels. The response
  ceiling grows by one paragraph per three source segments and caps at eight,
  preventing a source-sized paragraph inventory.
- The complete source catalog, prompts and serialized source-ID response schema
  must fit one synthesis request. Incomplete or over-budget source context makes
  no prose request and persists the explicit `claimLedgerFallback` mode. The
  selected qualified runtime then preflights the exact generation request:
  Ollama tokenizes the complete serialized payload and llama.cpp counts its
  framed prompt. A runtime context rejection selects the same fallback before
  inference. After a coherent draft validates, synthesis also materializes and
  exact-preflights every downstream coherent-verification batch using the same
  request constructor, request-local schemas, ordinals and seed as verification.
  A planner or runtime context rejection discards that draft and persists the
  verified-ledger fallback before synthesis completes. Other admission failures
  still fail. Fallback records `COHERENT_SUMMARY_SOURCE_CONTEXT_TOO_LARGE`;
  invalid runtime or model output still fails rather than masquerading as fallback.
- Connect delivery-policy runs retain the versioned direct claim-ledger
  synthesis path. This preserves their distributed page coverage
  contract and existing wire result while standalone summaries use coherent
  General presentation.
- The analyzed claim ledger remains durable for audit and fallback. Reader-facing
  prose is verified separately against the exact normalized source segments it
  cited. Unsupported prose cannot ship merely because ledger claims pass, and
  supported coherent prose remains present when every independently verified
  ledger paraphrase is withheld.
- A deterministic modal guard catches a strong predicate where cited source
  states the same predicate with weaker modality in the matching bounded subject,
  object and negation context. Strong wording about a different actor or object
  does not suppress the check. It permits one context-bounded regeneration with
  application feedback, then fails closed. Current-artifact reload reruns the
  same source-backed check. Require-family normalization detects both `should
  require` to `requires` and infinitive strengthening while preserving
  already-strong source wording. Tests prove mixed-actor, mixed-object and
  opposite-negation boundaries, corrected second-request behavior,
  repeated-invalid rejection and reload rejection.
- Current artifacts are synthesis 6.0.0, verification 7.0.0, summary 6.0.0 and
  citation 4.0.0. Historical direct and hierarchical artifacts keep their
  versioned validation. The desktop renders coherent prose first with exact
  citation controls and keeps any supported claim ledger in supporting detail;
  fallback opens that ledger explicitly.

**Verification and representative output**:
- `cargo fmt --all --check` and strict all-target/all-feature Clippy passed. The
  locked Rust suite passed 400 library tests with 6 intentional ignores; the
  office target passed 3 local tests with 3 opt-in ignores, and all 3 release
  contract tests passed. The TypeScript/Vite production build transformed 7
  modules and completed successfully.
- The coherent-summary boundary suite passed 8 tests. It covers canonical order
  for split segments across multiple blocks; incomplete, exact-fit and
  over-limit source admission with the serialized response schema included;
  exact and limit-plus-one paragraph counts; foreign and duplicate IDs; mixed
  weak and independently supported strong modality; infinitive and
  non-infinitive require-family strengthening; a corrected retry; and rejection
  after one repeated violation.
- Runtime-admission tests count a structured Unicode Ollama request from the
  complete serialized payload, reject max-plus-one before transport and admit the
  exact adjacent limit. The full summary fallback test covers both the early
  application bound and an exact runtime context rejection, proving neither path
  makes a synthesis request. A separate end-to-end boundary test generates a
  coherent draft, rejects its exact verification preflight, and proves the
  persisted and rendered result uses the verified ledger; its opposite side
  proves a non-context verification admission error fails synthesis.
- A focused application-service test passed the Connect delivery path through
  direct claim-ledger synthesis and the actual delivered page-coverage
  predicate, preventing source-selective coherent prose from failing after
  otherwise successful model work.
- A mode-boundary regression withheld every ledger paraphrase while supporting
  the coherent prose, then proved completion, citation creation, SQLite reopen
  and workspace projection all retained the reader-facing summary. Existing
  opposite-side tests still require coherent prose itself, and fallback or
  historical ledger presentation, to have at least one supported claim.
- The first public `structured_report.pdf` live trace exposed five source-sized
  paragraphs and an unsupported claim that architecture verification would
  "ensure consistency and accuracy in system design." After compression
  hardening, source comparison exposed `should retain` rewritten as `must
  retain`; prompt-only verification still marked it supported. Those findings
  produced the dynamic paragraph cap, exact modal instruction and deterministic
  repair guard rather than being accepted as model-verified truth.
- The final current-code live run passed in 15.56 seconds against
  `qwen3-30b-a3b:latest`. It delivered one 937-character overview citing native
  text pages 1, 2, 3, 4 and 6, excluded visual-only page 5, retained all 5 ledger
  claims, reached `CompleteWithWarnings` at state version 18, and survived the
  harness's independent SQLite reopen checks. Its central wording was: "The
  2027 OPERATIONS REPORT serves as a realistic fixture for deterministic
  structure testing, prepared for internal architecture verification." It then
  connected the planning context, authoritative source identity, cross-page
  Section 1 rule, hierarchy requirements and final findings in one paragraph.
- Human comparison against the extracted public-fixture text found the final
  prose preserved `should close` and `must be owned`, introduced no unsupported
  purpose or benefit, and maintained the source's actors and ordering. This is
  one semantic acceptance example. Valid IDs, exact quotations, integrity hashes
  and the model's supported verdicts are mechanical checks, not a general
  guarantee that generated wording is faithful.

**Deferred**:
- Manual profile identity and selection, Story behavior, Contract behavior, and
  uncertain/mixed automatic suggestions remain later slices. Automatic routing
  must not delay evaluating General summaries on a broader representative
  corpus.
- Documents whose complete source catalog cannot fit the bounded request receive
  the explicit verified-ledger fallback. A source-aware long-document synthesis
  strategy is future work and must not rephrase an already-lossy claim list.

## Slice 23 — Explicit General Summary Profile Identity (2026-09-07)

**Status**: implementation, automated gates and arc-owner review complete in
PR #39

**Root cause and change boundary**:
- General summary behavior existed only as an implicit synthesis branch. Runs
  did not record an application-level summary profile, so a later manual choice
  could not be carried independently through acceptance, retry, continuation,
  history and synthesis.
- This slice establishes only the minimum profile contract and explicit General
  selection. It does not add Story or Contract behavior, automatic routing,
  document-type classification, prompt changes, model adapters, long-document
  sampling or a summary-quality change.

**Implemented behavior**:
- The typed `SummaryProfile` contract currently admits only `general`; invalid
  values fail instead of selecting an arbitrary profile. The desktop exposes an
  explicit General selector, sends it with new-run admission and displays the
  persisted value in accepted history and reopened results.
- SQLite schema version 16 adds one immutable summary-profile row per run. New
  runs persist it atomically with ingestion, version-15 databases backfill
  existing runs as General, retries copy it atomically with their lineage, and
  continuations reuse the existing row.
- Synthesis reads the stored value before selecting its existing standalone or
  Connect delivery-policy path. A missing value fails; no synthesis path infers
  a replacement from the document or current interface state. Connect assigns
  General explicitly and retains its existing direct delivered-summary
  behavior.

**Verification**:
- The focused profile suite passed 11 tests covering schema backfill,
  immutability, explicit persistence and invalid-value rejection. Focused
  desktop, retry and workspace tests passed, including reopen and continuation
  identity.
- The Rust library suite passed 402 tests with 6 intentional ignores, and the
  office acceptance target passed 3 tests with 3 opt-in ignores. All 3 release
  contract tests passed. Strict all-target/all-feature Clippy, Rust formatting,
  the TypeScript/Vite production build and `git diff --check` passed.

**Deferred**:
- Story and Contract behavior remain separate usable-output slices. Automatic
  suggestions, uncertain/mixed routing and bounded classification sampling
  follow working manual profiles and must not delay them.

## Slice 24 — Manual Story Summary Profile (2026-09-07)

**Status**: implementation, automated gates and live-model quality acceptance
complete in PR #40

**Verified root cause and change boundary**:
- Slice 23 persists an immutable application summary profile, but its typed
  boundary, desktop selector and synthesis dispatch admit only General. The
  coherent synthesizer also uses one General-specific prompt and schema name.
  No Story behavior is hidden by parsing, storage or rendering.
- This slice extends the existing standalone source-aware synthesis path with an
  explicit Story choice. It does not add Contract behavior, automatic routing,
  document-type classification, long-document sampling, model adapters,
  ingestion changes, Connect behavior or a UI redesign.

**Required behavior**:
- A new standalone run may explicitly persist `story`; General remains the
  initial desktop choice. Retry, continuation, history and reopen retain and
  display the stored selection through the existing immutable profile contract.
- Story synthesis receives the same ordered exact source catalog as General and
  returns the same bounded cited prose shape. Its instructions preserve sourced
  characters, motivations, conflict, causal relationships, chronology, major
  events and resolution, while forbidding invented motivations, internal states
  and causal links inferred from sequence alone.
- General continues to receive only its existing General instructions. Story
  cannot change Connect's General-only delivery path. Unknown profiles and a
  specialized profile presented to a Connect delivery policy fail closed.
- Existing source-ID validation, exact citations, semantic verification,
  modal-strengthening repair, context admission and verified-ledger fallback
  remain shared and unchanged.

**Acceptance and minimum verification**:
- Opposite-side prompt tests prove General and Story select distinct named
  schemas and instruction sets while using the same source payload and output
  schema. Unknown profile JSON remains rejected.
- Persistence tests prove Story admission and immutability, plus General/Story
  mismatch rejection. Desktop tests prove explicit Story acceptance and
  persisted history/reopen identity; retry and continuation continue to inherit
  the stored value.
- A focused source-aware pipeline test proves a Story run reaches Story
  synthesis, keeps citations bound to exact normalized source, passes semantic
  verification and renders coherent mode. A synthetic narrative example is
  assessed for chronology, causality, character identity, sourced motivation,
  conflict, major events, resolution and unsupported additions.
- Required Rust, frontend and repository gates pass. The PR is complete when
  this end-to-end manual Story behavior is usable and documented; Contract and
  automatic suggestions remain deferred.

**Implemented behavior**:
- The typed profile now admits `story`; the existing immutable run row carries
  it through desktop admission, retry, continuation, history and reopen. General
  remains the initial selector value and Connect remains explicitly
  General-only.
- Standalone dispatch passes the stored profile into the existing coherent
  synthesizer. General retains its prior prompt. Story selects a distinct named
  schema and instructions for sourced character identity and motivation,
  conflict, causality, chronology, major events and resolution. Both profiles
  retain the same exact-source payload, response shape, request admission,
  modal repair, semantic verification, persistence, citations and fallback.
- A Story profile paired with a Connect delivery policy fails synthesis with
  `SUMMARY_PROFILE_DELIVERY_UNSUPPORTED`; it cannot silently enter the direct
  General delivery path.

**Automated verification and representative contract output**:
- `cargo test --all-targets` passed 406 library tests with 7 intentional
  ignores, 3 office tests with 3 opt-in ignores, and all 3 release-contract
  tests. Strict all-target/all-feature Clippy, Rust formatting, the
  TypeScript/Vite build and `git diff --check` passed. Focused tests also prove
  General/Story prompt separation, Story persistence and mismatch rejection,
  exact-source citation binding, retry inheritance, continuation identity and
  the Connect opposite side.
- The checked synthetic six-event story produces this authored contract
  example through the same bounded response parser and citation materializer:

  > Mara wants to reopen the mountain pass so winter medicine can reach Ivo,
  > but a destroyed bridge and Soren's prohibition block the crossing. After
  > she finds an old footpath, Ivo's worsening fever leads her to ask Len to
  > test it before the next snowfall. [p. 1; p. 2; p. 3; p. 4]
  >
  > When a rockslide blocks their return, Len secures a rope, which lets them
  > reach the clinic and bring the medicine back. Soren then reopens the marked
  > path under a guide requirement, Ivo recovers, and Mara archives her mother's
  > map. [p. 5; p. 6]

  Manual comparison confirms that example retains the named characters and
  relationships, stated motivation, conflict, event order, explicit causal
  links and resolution without adding an internal state or invented motive.
  The parser mechanically proves only bounded prose, valid source IDs, exact
  source retrieval and modal preservation; it does not prove semantic support.
- An ignored live Story harness uses the same non-private narrative and real
  Ollama adapter. Its first completed model response passed the mechanical
  contract but failed manual semantic review: it added `Determined` and `fearing
  for Ivo's life`, and joined Ivo's worsening condition to the storm with an
  unsupported `as` relationship. The Story prompt was narrowed to keep external
  reasons external, reject invented emotional labels and prevent temporal or
  causal connectors between events the source does not relate.
- After the competing LM Studio model was unloaded, Ollama loaded
  `qwen3-30b-a3b:latest` at 100% GPU. The corrected live harness passed in 5.86
  seconds and generated:

  > Mara, the village mapmaker, seeks to reopen the mountain pass to deliver
  > winter medicine to her brother Ivo, who is ill; a storm has destroyed the
  > only bridge, and council leader Soren forbids any attempt to cross, but Mara
  > finds an older footpath on her late mother's map, though it crosses unstable
  > cliffs, and as Ivo's fever worsens, she asks guide Len to help her test the
  > route before the next snowfall. [p. 1; p. 2; p. 3; p. 4]
  >
  > Len secures a rope after a rockslide blocks their return, enabling them to
  > reach the far-side clinic and bring the medicine back; Soren subsequently
  > reopens the marked footpath under a guide requirement, Ivo recovers, and Mara
  > keeps her mother's map in the village archive. [p. 5; p. 6]

  Source comparison confirms this model-produced synopsis retains character
  identity, Mara's stated goal, the bridge and Soren as obstacles, the explicit
  worsening-fever reason for asking Len, the rockslide-to-rope-to-clinic causal
  chain, the reopened path and Ivo's recovery. It preserves event order and adds
  no unsupported emotion, internal state, motive, action or term.

**Remaining limit**:
- This is one semantic acceptance example from one synthetic narrative and one
  qualified local model. Mechanical validation and one accepted sample do not
  establish general Story reliability across narrative styles or documents.

## Slice 25 — Manual Contract Summary Profile (2026-09-07)

**Status**: implementation, automated gates and live-model quality acceptance
complete

**Verified boundary and implemented behavior**:
- The existing immutable profile identity now admits `contract`, and the
  desktop offers Contract beside General and Story before source admission.
  Persistence, retry, continuation, history and reopen continue to use the
  shared stored profile rather than the selector's current value.
- Contract uses the existing standalone source-aware synthesis, exact evidence,
  semantic verification, citation, fallback and persistence pipeline. Its named
  schema and prompt preserve parties and roles, scope and term, obligations,
  recipients, conditions, exceptions, deadlines, dates, amounts,
  confidentiality, termination and remedies while rejecting invented legal
  advice or evaluations.
- Clause references come from cited exact source headings. The application
  appends one canonical source-derived section suffix before materializing
  deterministic claim identity and does not duplicate an identical suffix.
  Persisted validation recomputes that suffix from the cited evidence instead
  of parsing model-authored title punctuation. Admission requires a number token
  ending in a period and a clause-title period immediately before a line break.
  This deterministic structure supports simple and dotted identifiers,
  lowercase titles and wrapped initialisms such as `U.S.` while leaving
  ambiguous same-line decimal prose unclassified. A suffix is emitted only when
  the cited segment contains exactly one heading and it is the leading heading;
  multi-clause evidence keeps page provenance without a potentially incorrect
  section number.
- A complete catalog of at most six distinct numbered clauses must retain
  evidence from every supplied clause. Each source segment must contain exactly
  one numbered clause at its beginning; multi-clause segments are detected
  across whitespace or punctuation boundaries. Those segments, mixed,
  incomplete and larger catalogs keep materiality-based selection. Contract
  gets at most two bounded repair attempts for an incomplete short result and
  then fails closed. General and Story retain their existing repair behavior.
- Verification rechecks short-contract clause coverage after unsupported or
  ambiguous summary units are withheld. If filtering removes a required clause,
  the run fails at verification rather than publishing the remaining partial
  Contract overview.
- Evidence IDs alone do not satisfy short-contract coverage. Verification checks
  each cited summary-unit and required-clause pair in isolation and requires an
  operative fact, rather than a clause number, title or topic. A non-material or
  ambiguous pair downgrades that summary unit before final coverage is rechecked.
  This adds one bounded model request per cited pair for qualifying short
  Contracts; longer, mixed, incomplete, General, Story and fallback results do
  not enter this material-coverage path.
- Connect remains explicitly General-only. A Contract run paired with a Connect
  delivery policy fails with `SUMMARY_PROFILE_DELIVERY_UNSUPPORTED` rather than
  silently using General behavior.

**Acceptance evidence so far**:
- Focused tests cover typed admission and unknown-value rejection, immutable
  persistence, source-aware profile dispatch, exact source binding, Contract
  refusal on Connect, authoritative clause-suffix attachment and validation,
  short-contract coverage boundaries, decimal and heading false positives,
  wrapped and lowercase titles, isolated material-term verdicts,
  post-verification filtering, and the bounded repair limit.
- `cargo test --all-targets` passed 412 library tests with 9 intentional
  ignores, 3 office tests with 3 opt-in ignores, and all 3 release-contract
  tests. Strict all-target/all-feature Clippy, Rust formatting, the
  TypeScript/Vite production build and `git diff --check` passed.
- The authored six-clause example passes the real response parser, deterministic
  identity, modal-force checks, exact evidence materialization, clause-reference
  validation and short-contract coverage validation.
- A dedicated live materiality probe rejects the topic-only statement `The
  agreement addresses services and fees.` while accepting the sourced service
  and payment terms. The isolated four-pair probe passed in 6.07 seconds.
- The ignored live harness used `qwen3-30b-a3b:latest` through Ollama at 100% GPU
  and passed synthesis plus clause-by-clause materiality in 10.20 seconds. The
  run produced:

  > Northstar Bakery LLC, referred to as Client, has engaged Rowan Lee, referred
  > to as Consultant, to provide services from October 1, 2026, through March
  > 31, 2027. [Section 1] [p. 1]
  >
  > Consultant must deliver monthly inventory reports to Client by the fifth
  > business day of each month. Client must pay Consultant $2,400 per month
  > within 15 days after receiving an accurate invoice. Client will reimburse
  > Consultant for travel expenses that are pre-approved and do not exceed $500
  > per month; meals are not eligible for reimbursement. Consultant must not
  > disclose Client recipes during the term or for two years after it ends,
  > unless disclosure is required by law. Either party may terminate the
  > agreement with 30 days of written notice; however, Client may terminate
  > immediately if Consultant fails to cure a material breach within 10 days
  > after receiving written notice of the breach. [Section 2; Section 3; Section
  > 4; Section 5; Section 6] [p. 2; p. 3; p. 4; p. 5; p. 6]

  Manual source comparison confirms that the model output preserves every
  party, actor, recipient, condition, exception, deadline, date, amount,
  permission and prohibition in the fixture without adding a legal conclusion
  or unsupported term. The parser mechanically proves bounded prose, valid
  source identity, exact reference construction and modal preservation; those
  checks alone do not prove semantic support.

**Non-scope and remaining limit**:
- This slice does not add document-type inference, automatic suggestions,
  uncertain or mixed routing, long-document sampling, trained adapters, a
  parallel synthesis framework, Connect specialization or a UI redesign.
- The live result is one synthetic six-clause contract on one qualified local
  model. Paragraph organization remains prompt-driven and manually assessed;
  it is not represented as a measured reliability score.

## Slice 26 — Automatic Summary Profile Suggestions (2026-09-08)

**Status**: implementation, automated gates and live-model acceptance complete

**Verified boundary and implemented behavior**:
- Automatic suggestion is an opt-in desktop selection. Manual General, Story
  and Contract choices bypass classification and retain precedence.
- The pre-ingestion command uses the shared PDF parser, normalizer and structure
  interpreter in memory. It does not create or mutate a pipeline run. Only its
  effective General, Story or Contract result enters the existing immutable
  profile persistence path.
- Purpose is separate from profile. Agreement maps to Contract and narrative to
  Story. Informational, mixed, recognized-but-unserved `other`, and uncertain
  `unknown` map to the General fallback. Invalid model output fails visibly and
  unavailable source text remains an input failure.
- Sources containing at most 6,000 normalized characters use complete native
  text. Longer sources use up to 16 heading hints and bounded excerpts from five
  distributed non-empty pages. An initial mixed or unknown result receives one
  expanded inspection of up to nine pages; no other result creates a second
  classification request.
- The suggestion returns the classified source's content hash. Admission
  recomputes it and rejects a mismatch before persistence, preventing a replaced
  local file from being summarized under a profile chosen from earlier content.

**Acceptance evidence so far**:
- Six deterministic classifier tests pass with one opt-in live test ignored.
  They cover purpose-to-profile mapping, complete short input, exact structured
  output, bounded distributed and expanded sampling, empty and limit boundaries,
  one expanded request, malformed and extra output, and the required dominant-
  purpose counterexamples.
- The source-identity admission test proves both sides of the guard: a mismatch
  leaves document, run and event tables empty, while the matching hash admits
  the run.
- The TypeScript/Vite production build passes with the Automatic selector wired
  to the effective-profile start path.
- `cargo test --all-targets` passed 419 library tests with 10 intentional
  ignores, 3 office tests with 3 opt-in ignores, and all 3 release-contract
  tests. Strict all-target/all-feature Clippy, Rust formatting and `git diff
  --check` also pass.
- With LM Studio reporting no loaded models, the ignored live harness loaded
  `qwen3-30b-a3b:latest` through Ollama at 100% GPU. It passed all five synthetic
  cases in 9.27 seconds: an agreement selected Contract; an article explaining
  contracts selected General; a story containing legal language selected Story;
  an analytical report opening with an anecdote selected General; and a coequal
  mixed collection selected General.

**Non-scope and remaining limit**:
- This slice does not persist purpose or sampling metadata, measure classifier
  reliability, add trained adapters, specialize Connect, change synthesis
  prompts or schemas, or redesign result rendering.
- Automatic mode parses the source once for classification and the ordinary
  pipeline parses it again after admission. The content-hash guard preserves
  source identity across those reads, but the extra local parsing and one or two
  model calls add latency before a run is created.
- The five live examples demonstrate the intended decisions on one qualified
  local model. They do not measure classification reliability across documents,
  and the application intentionally exposes no confidence percentage.

## Slice 27 — Long General Summary Synthesis (2026-09-08)

**Status**: implementation, automated gates and live-model quality acceptance
complete

**Verified root cause and implemented behavior**:
- The 111-page public DOL fixture did not reach coherent General synthesis. Its
  complete exact-source catalog exceeded one synthesis request, so the existing
  admission rule returned the verified claim ledger. An analysis quote-boundary
  omission also made the catalog incomplete. General can now continue from a
  nonempty safe catalog while preserving the omission warning; an empty catalog,
  and incomplete Story or Contract catalogs, still use the existing fallback.
- Long General sources reuse the existing normalized exact-source catalog. The
  application partitions it into bounded ordered windows, asks the model to
  select a fixed number of known source IDs from each window, restores canonical
  source order, and sends only those exact segments to the existing coherent
  synthesis path. Selection must strictly shrink, use unique known IDs, and fit
  both the application character budget and the runtime's exact preflight.
- A matching extraction claim may accompany an exact quotation as drafting
  guidance. Exact quotation remains authoritative; unmatched source segments
  carry no duplicate claim field. The guidance is prompt-only; persisted
  synthesis evidence retains the historical v6 exact-quote representation, so
  existing `6.0.0` artifacts reconstruct unchanged. Selection requests never
  receive extraction claims, and Story and Contract prompts do not receive this
  General-only guidance, so their existing context admission sizes remain
  unchanged.
- Selected sources retain their original selection-window identity. A General
  summary unit cannot combine windowed and unwindowed sources or sources from
  two windows. One bounded structural repair is allowed independently of the
  existing modality repair. If that repair remains invalid, already-valid
  original units are retained and the unsafe units are omitted with
  `COHERENT_SUMMARY_CROSS_WINDOW_UNITS_WITHHELD`; an all-invalid or otherwise
  malformed response still fails closed.
- The General and shared semantic-verification prompts explicitly reject topic
  expansion, actor or program transfer, broadened enumerations, generic
  conclusions, changed endpoints and strengthened modality. Mechanical source
  ID validation remains distinct from semantic support judgment.

**Acceptance evidence so far**:
- Focused tests pass for exact source-selection count and character boundaries,
  canonical ordering, duplicate and foreign IDs, zero and over-limit inputs,
  partial-catalog policy, extracted-claim matching, selection-window admission,
  a successful window repair, safe-unit retention after a failed repair, and a
  separate subsequent modality repair. The coherent test group passed 19 tests
  with 2 opt-in live tests ignored; the low-context end-to-end test proves a
  complete catalog can be rejected while its bounded reduction reaches coherent
  synthesis.
- `cargo test --all-targets` passed 424 library tests with 10 intentional
  ignores, 3 office tests with 3 opt-in ignores, and all 3 release-contract
  tests. Strict all-target/all-feature Clippy, Rust formatting, the
  TypeScript/Vite production build and `git diff --check` passed.
- Before the long-document path was repaired, the public 111-page fixture
  completed with 81 rendered claims, 17,283 summary characters, 176 model
  requests, and
  `COHERENT_SUMMARY_SOURCE_CONTEXT_TOO_LARGE`. The final seeded run used
  `qwen3-30b-a3b:latest` through Ollama at 100% GPU and completed in 115.91
  seconds with 186 requests. It delivered six cited paragraphs and 2,744 summary
  characters without the source-context fallback warning.
- Manual comparison against the cited pages confirms that the retained result
  preserves the FLSA definition and enterprise conditions, overtime rules and
  examples, separate MSPA coverage conditions for FLCs versus AGERs/AGASs,
  payroll and vehicle-insurance requirements, and field-sanitation quantities.
  Earlier candidates changed `did not use more than 500` to `fewer than 500`,
  broadened five named relationships to `family member`, transferred the FLC
  consideration condition to AGERs/AGASs, and changed an outbound-transport
  endpoint. The final retained paragraphs contain none of those changes; the
  cross-window H-2A paragraph was withheld rather than published.

**Non-scope and remaining limits**:
- This slice does not change Story or Contract overflow behavior, automatic
  classification, document-type modeling, ingestion, OCR, Connect delivery,
  persistence schema, result rendering, dependencies, or model training.
- Selection is a bounded relevance judgment, not exhaustive long-document
  coverage. A failed structural repair reduces coverage and exposes a warning.
  The delivered phrase `payroll must be processed at least semi-monthly` is a
  plain-language interpretation of the source slide's `Payroll` heading and
  `Pay at least semi-monthly` bullet, rather than mechanically proved
  entailment.
- The shared verifier returned supported for several earlier statements that
  manual source comparison rejected. Valid IDs and model verdicts therefore do
  not establish semantic support; representative output still requires the
  documented human comparison. This is one public document on one qualified
  local model, not a measured reliability result.

## Slice 28 — Semantic Support Hardening (2026-09-08)

**Status**: implementation, automated gates and focused live-model acceptance
complete

**Verified root cause and implemented behavior**:
- The shared semantic prompt was already explicit, but the qualified
  `qwen3-30b-a3b:latest` verifier still returned `supported` for isolated probes
  that changed `no more than 500` to `fewer than 500`, replaced five named
  relationships with `family member`, and transferred the FLC consideration
  condition to AGERs/AGASs. It correctly rejected a changed transportation
  endpoint. Model verdicts alone therefore do not enforce these exact
  relationships.
- Current coherent verification now applies a deterministic post-verdict veto
  before filtering or rendering to either coherent summary units or the claim
  ledger when the current coherent fallback mode presents that ledger. Historical
  legacy-list verification paths retain their existing versioned behavior. It checks numeric comparison inclusivity,
  broadened family enumerations, mechanically identifiable `from`/`to`
  reversals or recombinations, conditions transferred across source-defined
  actors, literal evaluative concepts transferred across named subjects or
  polarity, and modal strengthening. The
  check can only retain or downgrade a model verdict; it never promotes an
  unsupported or ambiguous verdict.
- Actor matching supports both source-defined acronyms and their full labels.
  This matters because the live synthesizer sometimes writes `farm labor
  contractors` instead of `FLCs`. Actor qualifiers are bound to their source
  relationship inside compound sentences and when a condition leads the main
  actor clause. Evaluative conclusions are bound to every named subject when
  both claim and source expose the same literal concept; nonliteral evaluative
  paraphrases remain for the model verifier rather than being mechanically
  rejected. Numeric relations retain
  signs, normalize bounded English cardinal wording, compare exact normalized
  decimal thresholds, bind ordinary following unit tokens and explicit
  temperature-scale qualifiers when both source and claim expose them, retain
  `%` as the canonical `percent` unit, attach common prefix or suffix currency
  symbols to their number, and retain structurally marked compound units using
  `square`, `cubic`, or `per`, including a compound denominator, while
  excluding grammatical continuations, accept only logically entailed weak-bound
  paraphrases, and bind an explicit constraint to its nearby subject/predicate
  when cited text exposes the same context. Plain copular values are treated as
  exact equality, allowing only logically entailed weaker bounds. A contradictory relation on the same
  numeric value is considered only when source and claim share that established
  context or the same complete normalized subject anchor; an unrelated
  same-valued constraint therefore cannot veto an otherwise
  supported claim. Restrictive `immediate family`
  wording must remain explicit in a family-member claim when every cited family
  occurrence is restricted, while singular and plural `member` forms are
  equivalent and an explicit unqualified source occurrence remains usable. Directional parsing
  accepts both `from … to …` and `to … from …` syntax and binds a route to its
  mechanically known local actor independently of predicate wording when the
  claim prefix identifies an actor already attached to a cited route, and an
  explicit passive `by` agent owns its route. Conjunctive source actors are
  retained as individual owners of their shared route. A disjunctive actor does
  not establish which alternative owns the route, so the deterministic check
  leaves that verdict to the model. An `and` starts a new route clause only
  after a prior route predicate or endpoint. Evaluative relations retain negation while treating additive
  `not only … but also` wording as affirmative. The kinship backstop recognizes
  clear noun uses such as `relative of` without treating comparative `relative
  to` as a family enumeration. Passive numeric wording binds a leading bound to
  an explicit trailing `by …` subject, and actor qualifier matching retains
  negation polarity. Shared negation normalization covers expanded and contracted
  auxiliary forms, and numeric comparator context ignores optional articles.
  Exact numeric subject anchors recognize ordinary and contracted linking verbs,
  explicit requirement and local-use predicates, negative modal forms and
  symbolic comparator tokens. Conditions bind to the local subject after their
  connector; shared words outside or inside distinct multi-token subjects are
  not sufficient.
  Relation parsing stays within sentence or clause boundaries so a negation or
  endpoint in one sentence cannot affect the next. Leading decimals normalize
  to their zero-prefixed value. Numeric subject comparison accepts a shared
  prefix only when both following tokens begin the predicate, so auxiliaries do
  not hide a named-plan transfer and entities with shared name prefixes remain
  distinct; the same predicate check admits a mechanically known one-token
  subject, matches ordinary predicate words across an optional auxiliary only
  when their normalized words agree, and rejects different predicates or name
  components. Explicit negation inverts inclusive word or symbol comparators,
  `under`/`over` forms, exact equality, and one-word `cannot`. Modal comparisons bind force and
  polarity to the same local subject and object context, including contracted
  weak-modality negation and one-word `cannot`; an already-strong source must
  match the strong claim's polarity in both directions.
  Leading conditions accept either a comma or `then`, and compound claims check
  every completed actor-condition relation; the declared exclusion connectors
  (`unless`, `except`, `excluding`, `absent`, and `without`) remain inside the
  parsed condition and contribute exclusionary polarity when a qualifier is
  compared. A restrictive `only if` remains bound to the same actor and
  compensation condition: neither adding nor removing `only` can pass as plain
  `if`. Directional
  checks reject a known reversed endpoint even when the other endpoint is new
  and reject a route transferred to an actor named at the start of another cited
  clause, including when the claim uses an unlisted route predicate. A
  mechanically unknown actor absent from cited clause starts remains for model judgment,
  shared-copula evaluations inherit their prior concrete subject, transitive
  `ensure`/`guarantee` evaluations bind directly to their subject, and a
  coordinated evaluation whose full subject phrase is not cited is retained
  only when every individually cited component has the same supported
  evaluation and polarity. A literal evaluation otherwise requires the exact
  cited subject or an explicit conjunctive subject component; it cannot move
  from a qualified subject such as a procedure's review to the shorter procedure
  name. The lexical negatives `unimportant`, `ineffective`, `unsafe`,
  `unhealthy`, `unnecessary`, and `nonessential` carry negative polarity when
  compared with their positive evaluation concepts. A source-side temporal
  route restriction introduced by `during`, `until`, or `throughout` must
  survive in a mechanically compared route claim; otherwise that broader claim
  is downgraded. A temporal phrase found only in a claim still remains for the
  model verifier. Plain copular numeric equality recognizes an intervening
  `not`, so both equality-to-inequality and inequality-to-equality changes are
  rejected.
- Verification artifacts now use version `8.0.0`. The previous coherent
  `7.0.0` contract remains readable, while new results cannot be mistaken for
  artifacts produced without the deterministic veto. Synthesis, summary,
  citation, database and UI schemas are unchanged.

**Acceptance evidence so far**:
- Paired deterministic probes preserve equivalent `at most 500` wording,
  spelled-out cardinal values, symbolic `≤`/`≥` bounds, comma/decimal and signed
  number formatting, `maximum of`/`minimum of` wording, source-supported actor
  grouping, a correctly qualified single actor, and a correctly bound leading
  condition, including a source-supported contracted negative condition. They
  also preserve a contracted inclusive numeric bound, entailed same-value and
  cross-value weak-bound paraphrases with the same known unit, a weak bound
  entailed by a plain copular equality, an active bound paraphrased
  with its correct passive `by …` subject, a harmless auxiliary change on the
  same named plan, distinct plans with a shared name prefix, a leading decimal,
  a one-token numeric subject across an auxiliary change,
  comma- and `then`-delimited conditions, multiple correctly qualified actor
  relations in one claim, an unchanged `only if` restriction,
  synonymous transport endpoints including `home`, inverted source-route syntax,
  a route retained on its source actor across carry and unlisted-predicate paraphrases,
  a route claim for a mechanically unknown actor left to model verification,
  either member of a coordinated source actor retaining the shared route, a
  disjunctive actor alternative left to model verification, a retained
  source-side temporal route restriction,
  an unchanged Celsius qualifier, a generic unchanged mass unit, a square-unit
  spelling variant including a nested compound denominator, an ordinary
  predicate across an inserted auxiliary, a symbolic percent source retained as `percent`, a prefix
  dollar source paraphrased as `dollars`, a suffix euro source paraphrased as
  `euros`, explicit
  `immediate family` scope across singular/plural wording, and an unqualified
  family claim when cited evidence contains both restricted and unrestricted scopes,
  comparative `relative to`, additive, subject-bound, shared-subject,
  shared-copula, transitive, compound, coordinated, qualified-subject, explicit
  and lexical negative evaluations including `unimportant`, `unnecessary`, and
  `nonessential`, a nonliteral
  safety paraphrase left to model verification, and qualified modality including
  a negative strong source retained with the same polarity.
  Opposite probes reject an exclusive
  `fewer than 500` boundary, symbolic `>`/`<` opposites, a known numeric unit
  changed from dollars to percent, percent or a prefix dollar value changed to
  the other unit, Celsius to Fahrenheit, kilograms to pounds, or square meters
  to square feet,
  a stronger numeric
  threshold, a removed negative sign,
  a same-value boundary reversal scoped to its matching complete subject while
  a same-value constraint for a different subject sharing only its first word is
  ignored, a strict boundary not entailed by a plain equality,
  a bound moved between named plans, broader kinship label, acronym and full-name actor transfer (including
  distinct rules in one item or compound sentence, and a single actor borrowing
  another actor's qualifier, including a leading condition), an active or
  passive bound moved between named plans, the same transfer hidden by an
  optional comparator article, contracted numeric or actor negation reversed to
  affirmative, an `unless`, `except`, or `only if` condition weakened to `if`,
  plain `if` strengthened to `only if`, a contracted or `cannot`
  weak modal strengthened to a strong modal, a `cannot be more than` numeric
  constraint reversed to affirmative `more than`,
  explicitly negated inclusive, `under`, and exact comparators, both polarity
  changes around a negated copular equality, a leading-decimal
  bound reversal, a named-plan transfer hidden by an auxiliary, ordinary
  predicate, or shared name prefix, comma-less and compound actor-condition transfers, endpoint reversal from
  either source-route syntax even with one newly worded endpoint, a route moved
  from one mechanically known actor to another even when the route predicate is
  paraphrased with an unlisted verb or when the target actor is named only in a
  cited non-route clause, including both conditions together, a passive route assigned to the wrong explicit agent,
  or a coordinated route assigned to a cited non-owner, omission of a cited
  temporal route restriction,
  removal of the restrictive
  `immediate` family modifier, an evaluation
  moved between named procedures including shared-copula, transitive and
  coordinated wording or from a qualified evaluation subject to its shorter
  subphrase, lexical negative evaluations including `unimportant`,
  `unnecessary`, and `nonessential` reversed to positive, removed
  evaluation negation, `should`-to-`must` strengthening, strong modality
  transferred between named plans, and either polarity reversal of an existing
  strong modal. Separate probes
  reject partial verdict coverage, mismatched identities and unknown evidence,
  and preserve an already-ambiguous verdict.
- An end-to-end fixture makes the model return `supported` for a mechanically
  unsupported broadened family category. The production verification boundary downgrades it,
  persists the auditable failed attempt and creates no summary artifact.
- A separate end-to-end fallback fixture makes the model return `supported` for
  a broadened ledger claim, forces the verified-ledger presentation, and proves
  the guard downgrades and omits that claim while rendering the remaining
  supported ledger claims. An ordinary fallback control still retains its
  supported source claims.
- `cargo test --all-targets` passed 429 library tests with 10 intentional
  ignores, 3 office tests with 3 opt-in ignores, and all 3 release-contract
  tests. Strict all-target/all-feature Clippy, Rust formatting, the
  TypeScript/Vite production build and `git diff --check` passed.
- A temporary 25-page public DOL excerpt completed through Ollama on the
  qualified model. The pre-fix run published the transferred FLC consideration
  condition. The post-fix run withheld that unit, retained the supported FLSA
  and H-2A prose, cited 24 of 25 native-text pages, and completed with the
  existing `SEMANTIC_CLAIMS_WITHHELD` and `SUMMARY_COVERAGE_SHORTFALL` warnings.
  It used 59 bounded model requests and reported a 0.96 supported-evidence
  fraction.

**Non-scope and remaining limits**:
- This is a high-precision backstop for relationships that can be compared
  mechanically. It is not a general entailment proof; manual semantic review
  remains necessary for representative output, and future failure classes need
  their own evidence before expanding the guard.
- Two attempts to rerun the complete 111-page fixture produced the same
  unfinished synthesis response and failed before verification with
  `MODEL_SUMMARY_RESPONSE_INVALID`. The captured synthesis response hash was
  `063ce498b825257f26d8a4ed5599abc66076daac4e86e6780ccbddce762d0cb8`.
  That pre-verification output-budget behavior is deferred rather than folded
  into this slice.
- Long Story and Contract synthesis, automatic routing, ingestion, OCR,
  persistence schema, UI, model training and unrelated synthesis repair remain
  outside this slice.

## Slice 29 — Long General Decoder-Cap Recovery (2026-09-08)

**Status**: implementation, automated gates and live-model regression acceptance
complete

**Verified root cause and implemented behavior**:
- Long General synthesis uses a structured schema whose unit text has a
  1,200-character maximum. In the public 111-page DOL run, the model's repair
  response was valid JSON but its final unit ended at exactly that maximum on
  the letter `o`, without terminal punctuation. The response used only 855 of
  the configured 2,048 completion tokens, so increasing the output-token budget
  would not address this decoder boundary.
- The parser now classifies this exact condition only for General catalogs whose
  selected sources all carry long-document selection windows. The capped unit
  must otherwise be canonical and cite a nonempty, bounded set of unique known
  source IDs. Shorter incomplete text, malformed metadata, short General input,
  Story and Contract retain their existing invalid-response behavior.
- One bounded repair asks the model to shorten only incomplete capped units and
  preserve complete units and their source IDs. A valid repair is accepted only
  if it contains every validated original sibling's exact text and normalized
  evidence IDs in order and covers every clipped unit's source evidence, with
  repeated references counted separately. Safe siblings are consumed before
  replacement coverage is checked. Each modal-strengthened or mixed-window
  complete sibling keeps its own source-evidence group; candidate claims are
  reserved whole only when their combined evidence exactly matches that group.
  Clipped coverage is checked afterward, so one repaired sibling cannot also
  count as replacement for a clipped unit through shared or added evidence.
  Ordinal-derived claim IDs are intentionally excluded because
  repairing an earlier unit can move a later sibling. If the repair remains
  invalid, omits a sibling, rewrites one or drops clipped material, the
  application may retain complete sibling units from the original response only
  after normal source, window, completion, modality and evidence validation.
  Modality is evaluated per sibling: one strengthened sibling is excluded from
  the preservation baseline without erasing a separate safe sibling or its
  canonical evidence. Retained siblings are rematerialized after filtering so
  their deterministic claim IDs match their new fallback ordinals. A separate
  mixed-window sibling is likewise withheld without erasing structurally valid
  siblings, regardless of whether that unit precedes or follows the clipped
  unit. A later window fallback may omit that mixed unit while it must still
  preserve safe siblings, corrected modal evidence and recovered clipped
  evidence. That newer validated fallback takes precedence if the window repair
  is invalid or incomplete. Validated window and modality snapshots replace an
  older snapshot of the other kind, so fallback selection follows repair order
  instead of a fixed warning-type preference. A fully recovered response that
  still strengthens source modality likewise snapshots its individually safe
  units before the modality retry. If that retry fails, the newer safe content
  is delivered with
  `COHERENT_SUMMARY_MODAL_STRENGTHENED_UNITS_WITHHELD`; this also gives an
  all-clipped original a deliverable fallback when at least one repaired unit is
  modality-safe. Other structural errors still fail closed.
  It records `COHERENT_SUMMARY_CLIPPED_UNITS_WITHHELD`, plus
  `COHERENT_SUMMARY_CROSS_WINDOW_UNITS_WITHHELD` when the delivered fallback
  also excluded a mixed-window sibling, and
  `COHERENT_SUMMARY_MODAL_STRENGTHENED_UNITS_WITHHELD` when a complete sibling
  was separately excluded for stronger modality. An all-clipped response
  carries its source-coverage requirement into repair but cannot supply an
  original fallback; if its repair omits required evidence or remains invalid,
  it fails closed. A clipped unit produced by the window repair may use the same
  single decoder-repair attempt when its complete siblings still contain the
  prior safe window baseline. Successful recovery delivers the newer content;
  a repeated clip or omitted baseline returns the prior fallback with both
  window and clipped warnings. If feedback would exceed the configured input
  or runtime context, the best already-validated fallback is returned with its
  warning; the size error remains for responses with no deliverable fallback.
  The schema and its 1,200-character limit are unchanged.

**Acceptance evidence so far**:
- Paired tests accept a complete sentence at exactly 1,200 characters and prove
  that an incomplete capped unit gets one repair. A corrected repair returns
  both validated units; a repeated defect returns only the fully validated
  sibling and reports the clipped-unit fallback. Repairs that omit or rewrite
  the complete sibling also return the unchanged validated sibling, while a
  repaired leading clip preserves and accepts a later sibling across its
  ordinal-derived claim-ID change. Dropping only the clipped material returns
  the warned safe fallback. A mixed response with one safe sibling and one
  modal-strengthened sibling proves that a repair cannot omit the safe one after
  correcting the other. Clipped-plus-mixed-window probes cover both unit orders
  and both warning outcomes. A nested clipped-then-window repair probe proves
  that a newer window fallback cannot replace the original safe sibling
  baseline. Shared- and added-evidence probes prove that correcting a modal
  sibling does not also satisfy a clipped replacement, including when the same
  candidate claim adds a clipped source ID. A repair-budget probe returns the
  warned safe fallback when feedback cannot fit and keeps the size error for an
  all-clipped response. A leading modal-invalid sibling probe validates the
  rematerialized ID of the later retained safe claim. Two nested window probes
  prove that a recovered clipped unit survives both an invalid repair and a
  structurally valid but incomplete repair. Two nested modality probes prove
  that recovered safe units survive a repeated strengthening failure for both a
  mixed original and an all-clipped original. The all-clipped source probe
  rejects unrelated replacement evidence and accepts the required evidence.
  Withholding-state probes cover clipped-only, clipped-plus-window,
  clipped-plus-modality and all three combined. Window-then-clipped probes prove
  successful recovery, repeated-clip fallback and rejection of a repair that
  omits the prior safe window baseline while retaining a newer complete sibling.
  A clipped-then-modal-then-window probe proves the newest validated window
  snapshot retains the corrected modal sibling when the window retry fails.
  Negative probes reject a 1,199-character
  fragment, empty, duplicate, foreign and nine-source metadata, unwindowed
  General catalogs, and Story catalogs.
- `cargo test --all-targets` passed 431 library tests with 10 intentional
  ignores, 3 office tests with 3 opt-in ignores, and all 3 release-contract
  tests. Strict all-target/all-feature Clippy, Rust formatting, the
  TypeScript/Vite production build and `git diff --check` passed. The final
  parser-ordering refinement was rerun through the focused tests and strict
  Clippy.
- With LM Studio empty, `qwen3-30b-a3b:latest` ran through Ollama at 100% GPU.
  The full 111-page DOL acceptance fixture passed in 106.46 seconds with 186
  requests, 81 claims, 83 synthesized evidence items and citations across 84
  pages. It delivered six complete cited paragraphs and 2,744 summary
  characters. The representative summary preserved the governing agriculture
  laws, FLSA coverage and overtime qualifications, MSPA actor distinctions,
  payroll and vehicle-insurance requirements, and field-sanitation quantities.

**Non-scope and remaining limits**:
- The live regression run used the pre-existing cross-window safe-unit fallback
  from its first synthesis response; it exposed the decoder-capped repair
  response but did not need the new clipped-unit fallback. The deterministic
  runtime tests exercise both the successful repair and repeated-defect paths.
- Earlier ledgered attempts failed before verification with an unfinished
  synthesis response, but that historical initial-response failure did not
  recur in the acceptance run. The current trace verifies the decoder-cap
  mechanism, not that every historical failure had the same cause.
- This slice does not change output or context budgets, structured schema
  limits, Story or Contract behavior, automatic routing, persistence, UI,
  ingestion, OCR, model training or semantic-verification policy.

## Slice 30 — Bounded Long Story Synthesis (2026-09-09)

**Status**: implementation, automated gates and live-model acceptance complete

**Verified root cause and implemented behavior**:
- A complete Story source catalog used coherent synthesis only while the full
  request fit the model context. The oversized-request branch admitted bounded
  source selection for General alone, so an otherwise complete long Story
  returned the verified claim-ledger fallback before a Story synthesis request.
- Story now reuses the existing bounded, ordered source-selection pipeline with
  its own prompt and schema. General keeps its established prompt, one-array
  response contract and reduction target; Contract remains outside this path.
- Small Story catalogs retain three quarters of their sources, rounded toward
  preserving context, while catalogs above the existing 16-source target use
  that target. Every selection iteration must still strictly shrink, stay
  within the existing request count and candidate limits, preserve canonical
  source order and fit both the calculated and provider preflight bounds.
- The Story selector separates ordinary choices from typed conflict,
  turning-point and ending source arrays without increasing the requested total.
  For multiple windows, the first window reserves the conflict source and the
  final window reserves the ending plus a turning point when its quota permits.
  A single window reserves the ending first, then the conflict, then the turning
  point as one-, two- and three-source budgets permit. Missing arrays, wrong
  counts, duplicate or foreign IDs, role overlap and role fields returned under
  General fail closed.
- Selected Story sources carry their originating window into synthesis. The
  Story prompt forbids combining different selection windows in one unit, and
  response parsing plus bounded repair enforce the same rule. Incomplete Story
  catalogs still use the verified claim ledger, and Story does not inherit the
  General-only decoder-cap recovery policy.

**Acceptance evidence so far**:
- A deterministic low-context end-to-end test forces a complete Story beyond
  one synthesis request and proves that the persisted result remains in
  coherent Story mode, uses the Story selection and synthesis schemas, reaches
  provider preflight and completes verification and citation rendering without
  the source-context fallback warning.
- Boundary probes cover zero and over-limit counts, exact and one-character-low
  request budgets, the maximum selection request count and one request beyond
  it, one-, two- and three-slot Story role allocation, first/final multi-window
  roles, duplicate, missing, overlapping and foreign IDs, canonical ordering,
  mixed selected/unselected sources, cross-window summary units, General
  isolation and Contract rejection.
- `cargo test --all-targets` passed 432 library tests with 11 intentional
  ignores, 3 office tests with 3 opt-in ignores, and all 3 release-contract
  tests. Strict all-target/all-feature Clippy, Rust formatting, the
  TypeScript/Vite production build and `git diff --check` passed.
- The ignored live harness ran `qwen3-30b-a3b:latest` through Ollama at 100% GPU
  with an 8,192-token context. Forced compression selected five of six synthetic
  sources: Mara's goal, the destroyed bridge and Soren's prohibition, the old
  footpath and unstable cliffs, Len's rope and clinic consequence, and the
  reopening/recovery/archive resolution. The generated two-paragraph synopsis
  cited pages 1-3 and 5-6, preserved those stated identities, obstacles, causal
  link and resolution, added no unsupported motivation, and required no unit
  withholding. The omitted source stated that Ivo's worsening fever caused Mara
  to ask Len for help; this is a bounded coverage tradeoff and the output does
  not imply that omitted reason.

**Non-scope and remaining limits**:
- This selection policy preserves an explicit arc under bounded compression; it
  does not guarantee exhaustive event coverage, and semantic review remains
  necessary for representative Story output.
- Long Contract source selection, Story decoder-cap recovery, automatic routing,
  UI, persistence, ingestion, OCR, model training and unrelated verification
  policy remain outside this slice.

## Slice 31 — Bounded Long Contract Synthesis (2026-09-09)

**Status**: implementation, automated gates and live-model acceptance complete

**Verified root cause and implemented behavior**:
- A complete Contract source catalog used coherent synthesis only while the full
  request fit the model context. The oversized-request branch admitted bounded
  selection for General and Story, but excluded Contract, so an otherwise
  complete long Contract returned the verified claim-ledger fallback before a
  Contract synthesis request.
- Contract now reuses the existing bounded, ordered source-selection pipeline
  with a distinct Contract prompt and schema. The selector prioritizes operative
  terms and keeps conditions or exceptions with the terms they limit. General
  and Story retain their existing prompt and schema contracts.
- Complete contracts of six or fewer distinct, simply numbered clauses remain
  outside lossy selection because their existing acceptance contract requires a
  material term from every supplied clause. Structurally mixed or larger
  complete Contract catalogs may use bounded selection; incomplete catalogs
  still use the verified claim ledger.
- Small eligible Contract catalogs retain three quarters of their sources,
  rounded toward preserving context, while catalogs above the existing
  16-source target use that target. Every iteration must still strictly shrink,
  preserve canonical source order, fit the calculated and provider preflight
  bounds, and stay within the existing request and candidate limits.
- Contract selection separates ordinary choices from typed identity/scope and
  risk/exit arrays without increasing the requested total. A single window
  reserves identity/scope first and risk/exit when a second slot is available;
  for multiple windows, the first reserves identity/scope and the final window
  reserves risk/exit. Missing arrays, wrong counts, duplicate, overlapping or
  foreign IDs, and fields belonging to another profile fail closed.
- Selected Contract sources retain their originating windows. The Contract
  synthesis prompt and response parser prevent a paragraph from combining
  separate windows, and bounded repair may retain a valid Contract sibling when
  another unit violates that boundary. The synthesis prompt also keeps dates
  attached to their sourced actor-action-object relationship instead of
  converting an engagement period into an agreement effective term.

**Acceptance evidence so far**:
- A deterministic low-context end-to-end test forces a complete long Contract
  beyond one synthesis request and proves that the persisted result remains in
  coherent Contract mode, uses the Contract selection and synthesis schemas,
  reaches provider preflight, and completes verification and citation rendering
  without the source-context fallback warning.
- Boundary probes cover the protected short-contract limit, mixed and larger
  admission, zero and over-limit counts, one- and two-slot Contract role
  allocation, first/final multi-window roles, missing, duplicate, overlapping
  and foreign IDs, cross-profile fields, canonical ordering, mixed
  selected/unselected sources, cross-window summary units, and profile schema
  isolation.
- The ignored live harness ran the configured Contract selection and synthesis
  path on a synthetic ten-clause agreement through
  `qwen3-30b-a3b:latest` at 100% GPU. Bounded selection kept clauses 1–7 and 9:
  the parties and engagement dates, service and payment duties, the expense and
  confidentiality exceptions, termination and cure rules, the data-security
  deadline, and the liability cap with its exceptions. The resulting three
  cited paragraphs preserved the responsible parties, amounts, timing,
  conditions and exceptions without adding a legal conclusion or unsupported
  relationship, and no unit was withheld.
- `cargo test --all-targets` passed 433 library tests with 12 intentional
  ignores, 3 office tests with 3 opt-in ignores, and all 3 release-contract
  tests. Strict all-target/all-feature Clippy, Rust formatting, the
  TypeScript/Vite production build and `git diff --check` passed.

**Non-scope and remaining limits**:
- The live bounded selection omitted the ownership and governing-law/amendment
  clauses. This is an explicit compression tradeoff; the summary does not imply
  that those omitted terms were covered. Selection improves representative
  coverage but does not guarantee an exhaustive review of a long agreement.
- This slice does not change short-Contract exhaustive coverage, General or
  Story selection behavior, automatic routing, UI, persistence, ingestion, OCR,
  model training, decoder-cap recovery or semantic-verification policy.

## Slice 32: disclose bounded source selection

**Status**: implementation and automated gates complete

**Verified root cause and implemented behavior**:
- Long General, Story and Contract synthesis can replace the complete source
  catalog with a smaller, model-selected catalog. The selected catalog resets
  its internal omission counter because it is complete relative to the selected
  request, so the delivered coherent summary previously gave no explicit notice
  that source segments had been excluded before synthesis.
- When bounded selection strictly reduces the available catalog and that
  coherent result is delivered, synthesis now emits
  `COHERENT_SUMMARY_SOURCE_SELECTION_APPLIED`. The warning records the selected
  and available source-segment counts and states that details outside the
  selected evidence may be omitted.
- The warning uses the existing synthesis-warning persistence and presentation
  path. Full-context coherent summaries and verified claim-ledger fallbacks do
  not receive it.
- Coherent synthesis, verification and final-summary versions advance with this
  output contract. A pre-disclosure coherent `Synthesized` or `Verified`
  checkpoint now fails recoverably before final artifacts are written so retry
  regenerates it; already completed pre-disclosure coherent summaries remain
  readable through their exact historical version pairing. Pre-disclosure
  claim-ledger fallback checkpoints remain continuable because they already
  disclose their source-context limitation and never used bounded selection.

**Acceptance evidence**:
- Deterministic low-context end-to-end tests for General, Story and Contract
  force bounded selection, require the warning, and prove that verification and
  the completed result preserve it.
- The full-context source-aware end-to-end test covers all three profiles and
  proves that none receives the warning without a reduced catalog. The exact
  selection-context rejection test proves that fallback output receives only
  the existing fallback notice.
- Continuation regression coverage installs both affected pre-disclosure
  checkpoint versions and proves that each becomes a recoverable failed run
  without summary or citation artifacts. Paired historical-validation coverage
  proves that completed pre-disclosure coherent output still validates, and the
  legacy mechanical verification continuation remains supported. A two-sided
  fallback regression proves that pre-disclosure Synthesized and Verified
  fallback checkpoints both complete with the fallback warning and without the
  bounded-selection warning. The completed coherent compatibility fixture
  reconstructs real version-6 synthesis-evidence and prose-claim identities,
  proves that pair remains readable, and rejects a version-6 artifact carrying
  version-7 evidence identities.
- `cargo test --all-targets` passed 435 library tests with 12 intentional
  ignores, 3 office tests with 3 opt-in ignores, and all 3 release-contract
  tests. Strict all-target/all-feature Clippy and the TypeScript/Vite production
  build passed.

**Non-scope and remaining limits**:
- This warning makes lossy selection visible; it does not change which source
  segments are selected, make a long summary exhaustive, or establish semantic
  support for the generated wording.
- This slice does not change prompts, profile routing, model calls, summary or
  citation schemas, persistence schema, source-selection policy, or UI layout.

## Slice 33 — Combined Profile Release Acceptance (2026-09-09)

**Status**: combined harness and live-model baseline complete; General semantic
release acceptance exposed issue #49

**Implemented acceptance boundary**:
- `scripts/profile-release-acceptance.sh` runs the existing automatic-routing,
  bounded long-Story, bounded long-Contract and complete persisted long-General
  live gates in one fixed sequence. It changes no product behavior.
- The command accepts only the documented public DOL deck by SHA-256 before it
  enables model-response and summary tracing. It builds the current frontend
  sources and fails before Rust testing when that build fails, the selected
  Ollama model is not resident, or the model row does not report `100% GPU`.
- `docs/OFFICE_ACCEPTANCE.md` distinguishes the mechanical contract checked by
  the tests from the profile-specific semantic review still required of the
  printed representative output.

**Acceptance evidence**:
- Shell syntax passed. A missing fixture argument was rejected with exit 64,
  and a present file with the wrong SHA-256 was rejected with exit 65 before a
  test ran. An unloaded Ollama model, a different base URL, an inherited
  `OLLAMA_HOST` and an alternate model-settings path were each rejected with
  exit 69. After the model was loaded, `ollama ps` reported
  `qwen3-30b-a3b:latest`, `100% GPU` and an 8,192-token context.
- The combined command passed all four fully qualified opt-in tests. Automatic
  routing classified the agreement as Contract, the legal story as Story, and
  the contract article, anecdotal report and mixed collection as General. Its
  test finished in 2.07 seconds. Long Story finished in 3.07 seconds and long
  Contract in 4.10 seconds. The complete persisted General pipeline finished in
  110.55 seconds.
- The Story output preserved Mara's goal, the destroyed bridge and prohibition,
  the risky alternate path, the rope-to-clinic causal link, and the reopening,
  recovery and archive resolution without adding a motivation. The Contract
  output preserved the named parties, engagement dates, service and payment
  duties, expense and confidentiality exceptions, termination/cure conditions,
  security deadline, and liability-cap exceptions with clause and page
  references.
- The 111-page General run ended `CompleteWithWarnings`, made 186 model
  requests, delivered 82 supported claims in a 4,436-character coherent
  summary, persisted citations and integrity metadata, and exposed
  `COHERENT_SUMMARY_SOURCE_SELECTION_APPLIED` alongside its other warnings.
- The combined command's TypeScript/Vite production build passed. The remaining
  required local gates also passed: Rust
  formatting, strict all-target/all-feature Clippy and `git diff --check`. The
  full Rust suite passed 435 library tests with 12 intentional live ignores,
  three office tests with three opt-in ignores, and all three release-contract
  tests.

**Semantic finding and next repair boundary**:
- The General output converted content under the source heading `Common
  Problems` into the unqualified sentence that employees paid a piece rate may
  fall below the minimum wage and that deductions may bring them below it. The
  exact page citation exists, but dropping the heading's problem framing can
  make prohibited or deficient pay sound permissible. Mechanical source-ID,
  quotation and model-verdict checks did not catch that loss of meaning.
- Issue #49 records the separate repair slice. It must first trace whether the
  framing is lost during extraction, evidence construction, synthesis input or
  generation. Slice 33 does not change prompts or infer the fix.

**Non-scope and remaining limits**:
- This slice does not change product prompts, schemas, routing, persistence,
  source selection, warnings, rendering, ingestion, OCR or model settings.
- The automatic live case stops after classification, and the Story and
  Contract live cases exercise bounded selection and synthesis directly. The
  deterministic suite separately covers expected-hash admission, effective
  profile persistence, warnings, citations and reopen behavior. This harness
  does not drive the native file dialog or establish those steps in one live UI
  invocation.
- The combined command proves the configured model passed one fixed public and
  synthetic corpus run. It does not turn model output into deterministic quality
  evidence, and a green mechanical result remains insufficient without semantic
  review.

## Slice 34 — General Source-Context Framing (2026-09-09)

**Status**: implementation and live-model acceptance complete

**Verified root cause and implemented behavior**:
- The public DOL source retained `Common Problems` in the same normalized block
  and exact quotation as the minimum-wage statements. Analysis, synthesis and
  semantic verification each dropped that governing heading, so the final cited
  prose presented the statements neutrally even though its source did not.
- General synthesis now derives a small, application-owned `source_framing`
  label only from a bounded heading ending in problem, risk, warning, exception
  or limitation and carrying an observable heading signal: casing, a valid
  marker or an explicit colon label. Lowercase wrapped prose does not create a
  framing label. Bounded source segments inherit the
  nearest reliable section heading in canonical document order, including later
  segments and fully available continuation pages that no longer contain it.
  Empty or visual-processing pages reset inherited state before and after the
  page because their complete governing text is unavailable. The resolver scans
  each extracted line so a subsequent bounded heading-shaped line, including
  a decimal, uppercase/lowercase Roman or uppercase/lowercase letter-marked
  heading, including a fully parenthesized outline marker, or a sentence-case
  heading with an explicit heading signal, resets the label. A recognized
  marker supplies the casing signal for a lowercase recognized section lead,
  while marked lowercase prose still does not qualify. An unmarked
  title-case, all-caps or sentence-case line ending in a period, exclamation
  point or semicolon remains body text unless it has a valid outline marker;
  numbering alone
  does not turn a punctuated sentence-case body line into a heading. A
  reliable heading followed by a colon and body on the same line changes state
  at the body boundary, including when the heading ends a preceding body line
  and its own body begins on the next line. Generic colon-ended resets require a
  marked heading or an explicit section lead. A conventional standalone
  title-case colon heading resets prior state; the exact introductory labels
  `Example:`, `Examples Include:`, `Note:`, `Important Note:` and `Supporting
  Example:` remain prose in either layout. An explicit inline prefix ending in a framing noun or one of the
  bounded compound terms resets prior state when negation, uncertainty or
  unsupported modifiers prevent assigning a new label. An interrogative
  heading can end prior state but cannot assign an
  affirmative framing label, including when its answer or another label follows
  on the same line. Direct framing/section phrases and the bounded `How to avoid
  ...` form qualify; ordinary substantive wh-questions do not create that reset,
  even when their prose mentions a framing noun. A
  recognized framing heading replaces the
  prior label. Inline transitions
  are applied in byte order and retain byte positions, so the last textual
  delimiter controls following content; a source after one inherits its state,
  while a source crossing one remains unframed. A standalone colon heading
  transitions at the heading start so a quote containing that heading and its next-line body keeps
  the relationship; a genuinely inline heading transitions at its body. The
  final transition controls following text. A candidate containing only a
  recognized framing heading remains unframed, while the heading's state still
  governs later source blocks.
  A bounded denial such as `None reported`, `None have been identified`, `No
  limitations were identified`, `No risks exist`, `No risks remain`, `No
  exceptions apply`, `No risks have yet been identified`, `No risks are
  currently present`, `No risks to report`, `Not applicable`, `N/A`, `N.A.` or
  `There are no known risks at this time` clears an active label. An implicit
  `None` denial or bounded not-applicable form can
  clear any label; an
  explicit denial must name a noun in the active framing category.
  Explicit denial noun phrases may use the same conservative framing modifiers
  as headings and the same three bounded compound families; uncertainty
  modifiers remain outside the grammar.
  Direct `No`/`Neither`, copular existential, and perfect existential denials
  must consist of a framing-noun phrase plus a bounded absence or reporting
  predicate; only the leading complete sentence is inspected so later prose on
  the extracted line does not hide the reset, unless the next sentence begins
  with a bounded contrast marker and either explicitly reintroduces the active
  framing category or states a bounded unresolved Problem/Risk condition, or
  explicitly reintroduces the category through a bounded affirmative noun-first
  predicate, optionally preceded by one article, conservative heading modifiers
  or `new`. A noun-first `remain` predicate followed by a bounded absence
  complement such as `eliminated` or `resolved` preserves the cleared state;
  `unresolved` reintroduces the active category.
  Negation in a subordinate proposition and a negated
  elimination do not erase that reintroduction. A repeated absence or a
  different framing category still clears state. Repeated determiners after noun
  conjunctions remain within that grammar, while
  interrogative clauses cannot reset it. A recognized outline marker may
  precede the bounded denial; invalid markers and marked negative rules cannot
  clear state. Negative rules and residual-risk propositions such as `No worker
  may be paid below minimum wage`, `No control eliminates every fraud risk` and
  `None of ...` retain their governing context.
  A bounded source segment
  that crosses either transition remains unframed because one relationship does
  not govern all of its text. General
  synthesis and verification use the same section-scope resolver. Story,
  Contract and source-selection prompts do not receive that context.
- An extracted drafting claim is omitted when its source carries a framing label,
  avoiding a lossy intermediate paraphrase. When General synthesis selects such
  a source, Rust adds the application-owned relationship to the final claim text
  before the existing source-aware semantic verifier runs. Distinct framing
  labels cannot govern one generated unit. Exact source evidence, source order,
  citation rendering and persisted schemas remain unchanged. A model response
  that mixes framing in one unit receives one bounded repair instruction to
  split only invalid units; the repair must preserve every individually valid
  sibling's rendered text and ordered evidence IDs, and a repeated violation,
  omission or rewrite fails closed. The repair request includes the rejected
  response as explicitly untrusted draft data so the model can preserve those
  siblings instead of regenerating them without seeing their prior wording.
  Those exact repair requirements are consumed
  after that repaired response passes, allowing a later validator repair to
  correct independently invalid wording. The initial
  General prompt and schema use the larger of the base unit budget and total
  compatibility-group count. Only a framing repair expands the prompt and
  schema ceiling to that initial limit times the greatest number of distinct
  framing groups in any one selection window, capped by the existing global
  maximum. The initial response therefore cannot consume the reserved split
  capacity. Story and Contract retain their prior generation ceiling.
  Profile-neutral persisted-content validation enforces the same global
  eight-unit cap as generation. It does not recompute a smaller ceiling from
  the full catalog after transient long-document selection windows have been
  discarded, so a valid selected-window result reaches verification.
- Synthesis, verification and final-summary versions advance for the changed
  output contract. Completed version-7 summaries remain readable; in-progress
  coherent version-7 checkpoints retry, while version-7 claim-ledger fallbacks
  remain continuable.

**Acceptance evidence**:
- Before the deterministic repair, a GPU DOL run passed mechanically but emitted
  a page-21 paragraph beginning `Employees paid a piece rate may fall below the
  minimum wage` while its cited exact quote began `Common Problems`.
- The live run before the final compatibility and denial-boundary fixes used
  `qwen3-30b-a3b:latest` at 100% GPU and passed in 117.81 seconds after 187 model
  requests. Its delivered page-21
  paragraph begins `The
  document presents the following as a problem`, retains the minimum-wage details and
  page citation, and its unchanged exact quote begins `Common Problems`.
- The preceding product-code head `1a3c90425ab0c4eed2dea58eda09b6dfff938f38`
  reran the full 111-page public DOL acceptance through
  `qwen3-30b-a3b:latest`. Ollama reported 100% GPU and NVIDIA reported 18,210
  MiB for the Ollama process. The evidence-traced test passed in 120.48 seconds
  after 186 model requests and produced 81 claims with 97 persisted evidence
  items. The delivered
  page-21 paragraph reads `The document presents the following as a problem:
  Common problems include employees paid a piece rate falling below the minimum
  wage ... [p. 21]`; its persisted exact quote still begins `Common Problems`.
  The run recorded summary integrity hash
  `c8b988ef0694f299139162439718316472520a7e55102d33d9b54b8d927efca3` and
  citation integrity hash
  `e6500ab1141ca63c570f0eaafc206e92b97a08827721121e062fa1dc555c19df`.
  An immediately preceding exact-head run failed closed with
  `SYNTHESIS_REPAIR_INPUT_TOO_LARGE` after the model combined neutral and
  Problem sources in one unusually long unit. The unchanged retry passed; this
  proves one acceptable exact-head execution, not deterministic live-model
  reliability.
- Product-code head `eb6a17b58cd049b67508105d29f64b01ff9682c9`
  also clears an active section after a bounded bare `No` answer to a section
  question and after a bounded denial that follows an earlier declarative
  sentence on the same normalized line. A bare `No` is accepted only in an
  answer context; interrogative, qualified and substantive `No ...` text keeps
  the active framing. The full deterministic gate passed on this head. Two
  unchanged-head live DOL runs used `qwen3-30b-a3b:latest` with an 8192-token
  context at 100% GPU, but both failed closed with
  `MODEL_SUMMARY_RESPONSE_WINDOW_MIXED` after 102.88 and 106.30 seconds when the
  model combined `s19`, `s20` and Problem-framed `s22` in one unit. No summary
  was persisted. The preceding exact product head therefore supplies this
  slice's representative live output; these final-head runs prove rejection,
  not successful live completion. Follow-up is tracked in issue #53.
- Product-code head `4cfaa383db21314f626f32371f60ac5b497af947`
  admits bounded `No risks were discovered` as an empty-section predicate and
  restores Risk for `Risks are unresolved` after a prior denial. Qualified
  discovery statements, `Risks are resolved`, and `Risks are not unresolved`
  remain on the opposite side of those boundaries. The focused classifier test
  and full deterministic gate passed. No further DOL model run was made because
  issue #53 already reproduces the independent cross-window failure twice and
  neither added predicate occurs in the affected DOL source text.
- Product-code head `c815b2bfd94e0b39dc78836f07bc699255547ae8`
  recognizes a leading bounded `N/A` or `N.A` before splitting its same-line
  continuation. `N/A. Overview follows.` therefore clears prior framing before
  the overview, while an interrogative, qualified, or attached path-like form
  retains it and an explicit adverse continuation can restore framing. The
  focused classifier test and full deterministic gate passed.
- Product-code head `d9dc80d48c4cd720b364e5be7c7a7a9728c2e030`
  resets prior framing at bounded mitigation questions such as `How can risks be
  reduced?` and `What are the solutions?` before their answers. Equivalent
  mitigation verbs and solution/control nouns share that bounded grammar, while
  failure-qualified questions and substantive questions about remaining risks
  retain their active framing. The focused classifier test and full
  deterministic gate passed.
- Product-code head `d93c604537cef762b3193050c2fd03288aba0e4e`
  admits bare `No` only after a bounded question about the presence or existence
  of the active framing category. Substantive questions about control failure do
  not clear the category, including through a labeled `Answer: No`; an explicit
  bounded denial such as `No risks were identified` still clears it. The focused
  classifier test and full deterministic gate passed.
- Product-code head `1226db8bb714ab5fb0d828e5f7c9ae1791c8251e`
  restores Problem or Risk framing when a later sentence says the condition has
  not been, was not, or cannot be ruled out. The predicate requires the bounded
  `ruled out` complement; affirmative `ruled out` and `ruled in` controls remain
  unframed. The focused classifier test and full deterministic gate passed.
- Product-code head `4bb76df9709d410837d9ae4dde43a0d9b1bab86f`
  admits modal `no longer be ruled out`, splits UTF-8 en/em-dash continuations
  only when a bounded coordinator follows, and rejects unmarked framing phrases
  that end as declarative sentences. Wrong complements, qualified dash clauses,
  and marked or colon-signaled heading controls preserve their prior behavior.
  The focused classifier test and full deterministic gate passed.
- Product-code head `e9ea756744309abfb2c5b4866a46344d2a7ae00a`
  retains a denied category as non-governing parser state, allowing a bounded
  adverse continuation on a later line or normalized block to restore it. The
  suspended category does not label neutral text and clears at a recognized
  section boundary or unavailable visual gap. Blank-line, cross-block,
  explicit-boundary and synthesis/verification parity controls pass. The
  focused classifier test and full deterministic gate passed.
- Product-code head `ee020922153d4dd9b507e8a2159d565036465367`
  recognizes bounded comma-separated framing denials before clause splitting
  and uses the shared declarative-terminal predicate when deciding whether a
  sentence-shaped line can reset active framing. ASCII, full-width and Arabic
  comma forms pass; missing and misplaced commas, mixed noun categories,
  compound-noun splits and question terminals fail closed. Supported Unicode
  declarative terminals preserve active framing. The focused boundary probe and
  full deterministic gate passed.
- Product-code head `3880c825ed8b51bbe6cb281f70a5a53c63f0e31f`
  resets framing for bounded auxiliary-led solution and control questions, and
  parses bounded expanded or apostrophe-contracted negated existential denials.
  Risk questions, control-failure questions, extra qualifiers, missing
  determiners, mismatched categories and interrogative denials retain framing.
  The focused two-sided classifier probe and full deterministic gate passed.
- Product-code head `a289dc20c02d4752f88441d9d99a24f8ea9cec93`
  admits bounded modal-led mitigation-action questions and supported temporal
  adverb positions around expanded negation. Modal failure and residual-state
  questions, noun uses of `control`, repeated negation, mismatched categories
  and qualified denials retain framing. The focused two-sided propagation probe
  and full deterministic gate passed.
- Product-code head `c4985e22eae058649e879473e522397db371dbee`
  uses the apostrophe-preserving bounded tokenizer for framing reintroductions,
  normalizes contracted auxiliaries into the existing polarity checks, and
  recognizes full-width and Arabic commas as byte-safe coordinated-denial
  delimiters. Wrong complements, repeated negation and qualified continuations
  remain unframed. The focused two-sided propagation probe and full
  deterministic gate passed.
- Product-code head `8c9aeacf33d7e45976d4ae6761c54df000a43606`
  admits `discovered` in the existing affirmative copular and perfect
  reintroduction predicates. Explicitly negated discovery remains neutral. The
  focused two-sided propagation probe and full deterministic gate passed.
- Product-code head `9c38c1c62c0d053cac9883c84c270de7819aecbc`
  requires a declarative first sentence terminal before a suspended framing
  category can be restored by either noun-based or residual predicates. ASCII,
  full-width and Arabic questions remain neutral; declarative controls restore
  framing. The focused two-sided propagation probe and full deterministic gate
  passed.
- Product-code head `87770baa08c0eb5a27f2eb67830bad52f2c91438`
  scans bounded coordinated suffixes for a denial that follows an affirmative
  clause and recognizes full-width inline colons using their UTF-8 byte length.
  ASCII, full-width and Arabic comma coordinators, semicolon clauses and an
  em-dash coordinator clear the active category at the denial; qualification,
  comma-splice and negative-rule controls retain it. Full-width framing, reset
  and labeled-answer colons transition at the body without labeling a source
  that crosses the transition. The focused two-sided propagation probe and full
  deterministic gate passed.
- Product-code head `99585de5bb4a736b70c1d486e608b12b964673d7`
  retains an active category through a title-case question about that category's
  presence, while a bounded negative answer still clears it. Coordinated `it`
  and `they` continuations reuse the existing adverse-predicate grammar to
  restore a denied Problem or Risk category. Affirmative answers and bounded
  occurrence questions retain framing; negative answers clear it; affirmative
  resolution, wrong-complement and double-negation anaphoric controls remain
  neutral. The focused two-sided propagation probe and full deterministic gate
  passed.
- Product-code head `7e38aea2fe222e40901611234d4bd36cff1f978b`
  uses the shared Unicode sentence-terminal, coordination-delimiter and inline-
  colon predicates when scanning a suspended category for an adverse residual
  clause, and records the exact byte offset where framing resumes. A neutral
  clause before that adverse clause remains unframed, while a leading
  coordinator and same-sentence condition stay attached to the adverse clause.
  ASCII, full-width and Arabic questions do not restore framing. The focused
  two-sided boundary probe passed; the full gate passed 442 library tests, 3
  office tests and 3 release-contract tests with the documented opt-in tests
  ignored, plus strict Clippy, Rust formatting, `git diff --check` and the
  TypeScript/Vite production build.
- Product-code head `459028bca503e580008d6a94e0080b052916a263`
  restores a denied Problem or Risk category for bounded adverse copular
  residuals such as `Fraud is possible`, without requiring `still`, and adds
  the Arabic semicolon to the shared coordinated-clause delimiter family.
  Negated, interrogative, non-adverse and protective copular controls remain
  neutral. Arabic-semicolon neutral, adverse, trailing-denial, qualification
  and mixed-source controls match the existing ASCII-semicolon behavior. The
  focused two-sided boundary probe passed; the full gate passed 442 library
  tests, 3 office tests and 3 release-contract tests with the documented opt-in
  tests ignored, plus strict Clippy, Rust formatting, `git diff --check` and the
  TypeScript/Vite production build.
- Product-code head `96a40208ae50e92cd10a79dd71c859e2fb33b45a`
  treats bounded `No ... remain outstanding` and singular `No ... remains
  outstanding` statements as empty-section denials. The complement remains
  scoped to those two predicates and the existing bounded denial tail;
  questions, qualified statements and category mismatches retain framing, while
  a later adverse clause restores it only at that clause. The focused two-sided
  boundary probe passed; the full gate passed 442 library tests, 3 office tests
  and 3 release-contract tests with the documented opt-in tests ignored, plus
  strict Clippy, Rust formatting, `git diff --check` and the TypeScript/Vite
  production build.
- Product-code head `019c7a5597a8109437bff3c8a1deb8ed26fc69a9`
  requires a predicate before an explicit framing noun can restore suspended
  state, so a punctuated noun fragment such as `Risks.` leaves later text
  neutral. It also accepts bounded copular outstanding denials only after an
  observed `are`, `is`, `was`, `were` or `been` auxiliary. Missing-copula,
  interrogative, qualified and category-mismatch controls retain framing. The
  focused two-sided boundary probe passed; the full gate passed 442 library
  tests, 3 office tests and 3 release-contract tests with the documented opt-in
  tests ignored, plus strict Clippy, Rust formatting, `git diff --check` and the
  TypeScript/Vite production build.
- Product-code head `236693234155b40d3f310a42dfd81dd3d154a6cd`
  carries the already-validated existential copula state through noun parsing,
  so bounded direct, contracted and perfect forms such as `There are no risks
  outstanding` clear their matching section framing. Questions, qualified
  tails, category mismatches and a missing copula retain framing. The focused
  two-sided boundary probe passed; the full gate passed 442 library tests, 3
  office tests and 3 release-contract tests with the documented opt-in tests
  ignored, plus strict Clippy, Rust formatting, `git diff --check` and the
  TypeScript/Vite production build.
- Product-code head `435fb1308388dbc49bf02c217eee9dfa8af505c0`
  restores a suspended Problem or Risk category for bounded affirmative modal
  occurrence clauses with an adverse subject, while preserving polarity,
  declarative-clause and same-clause checks. It also inspects each immediate
  declarative sentence suffix for an explicit framing reintroduction, allowing
  neutral prose before a later adverse sentence without labeling a source that
  crosses the transition. Negated, interrogative, non-adverse, protective and
  cross-sentence pseudo-predicates remain neutral. The focused two-sided
  boundary probe passed; the full gate passed 442 library tests, 3 office tests
  and 3 release-contract tests with the documented opt-in tests ignored, plus
  strict Clippy, Rust formatting, `git diff --check` and the TypeScript/Vite
  production build.
- Product-code head `9448f6e8c72a6a2aa043fcb7d567223ff132d57a`
  bounds modal occurrence parsing to the current sentence and checks the
  complement of `remain` and `continue` before restoring a suspended Problem
  or Risk category. Resolving complements such as `impossible`, `resolved` and
  `eliminated` remain neutral, while `possible`, `unresolved`, continuing
  existence and double-negative forms restore framing. The same bounded
  predicate contract now governs explicit framing nouns and adverse residual
  subjects. The focused two-sided boundary probe passed; the full gate passed
  442 library tests, 3 office tests and 3 release-contract tests with the
  documented opt-in tests ignored, plus strict Clippy, Rust formatting,
  `git diff --check` and the TypeScript/Vite production build.
- Product-code head `0a65c09df1b4985a1f2189d7e4fd46b1df06190d`
  routes direct `continue` predicates through the shared occurrence-complement
  check and consumes bounded `to`, `be`, `been` and `being` links before
  classifying the complement. Direct and modal resolving forms such as
  `continue resolved` and `continue to be eliminated` stay neutral, while
  `continue unresolved` and `continue to exist` restore framing. The focused
  two-sided boundary probe passed; the full gate passed 442 library tests, 3
  office tests and 3 release-contract tests with the documented opt-in tests
  ignored, plus strict Clippy, Rust formatting, `git diff --check` and the
  TypeScript/Vite production build.
- Product-code head `cc665da46659d990b043080ea716d30b9604389e`
  recognizes a bounded framing or reset heading before an en or em dash and
  transitions at the following body, while keeping a source that spans the
  heading/body boundary unframed. It also routes direct and modal appearance
  predicates through the shared complement check, so resolved or eliminated
  appearance stays neutral while bare, possible or unresolved appearance
  restores framing. The focused two-sided boundary probe passed; the full gate
  passed 442 library tests, 3 office tests and 3 release-contract tests with
  the documented opt-in tests ignored, plus strict Clippy, Rust formatting,
  `git diff --check` and the TypeScript/Vite production build.
- Product-code head `eadd494b79d77dc24963124bc472f34e64db9f7f`
  treats bounded sentence-case benefit and advantage headings as neutral
  section boundaries while retaining terminal-punctuated benefit prose inside
  its governing section. It also admits bounded `can`, `could`, `may`, `might`,
  `will` and `would` questions about the active category's presence or
  occurrence before a bare denial; reporting, causation, control-failure and
  category-mismatch questions remain ineligible. The focused two-sided boundary
  probe passed; the full gate passed 442 library tests, 3 office tests and 3
  release-contract tests with the documented opt-in tests ignored, plus strict
  Clippy, Rust formatting, `git diff --check` and the TypeScript/Vite production
  build.
- Product-code head `783deaffe10270b9f76bbfc3d1a0690dfff49462`
  admits `may` and `might` through the existing bounded mitigation-question
  action grammar. Recommendation text following questions such as `Might these
  risks be mitigated?` is neutral, while failure-qualified questions remain in
  their governing Problem or Risk section. The focused two-sided boundary probe
  passed; the full gate passed 442 library tests, 3 office tests and 3
  release-contract tests with the documented opt-in tests ignored, plus strict
  Clippy, Rust formatting, `git diff --check` and the TypeScript/Vite production
  build.
- Product-code head `574707b4291584a4f4554a6b93c2f0a325712bd4`
  resets framing for bounded inline `Benefit(s):` and `Advantage(s):` sections
  and recognizes a spaced ASCII hyphen as an inline heading separator. Unspaced
  word hyphens and spaced hyphens without a bounded heading prefix retain the
  active category. The focused two-sided boundary probe passed; the full gate
  passed 442 library tests, 3 office tests and 3 release-contract tests with the
  documented opt-in tests ignored, plus strict Clippy, Rust formatting,
  `git diff --check` and the TypeScript/Vite production build.
- Product-code head `864ca63404b707f729ae7d910dcd9fa15bd5dc90`
  carries an eligible presence-question context into only the immediately
  following normalized line, allowing a split bare `No.` answer to clear the
  active category. A blank line, intervening content, or non-presence question
  consumes or never creates that context. The focused two-sided boundary probe
  passed; the full gate passed 442 library tests, 3 office tests and 3
  release-contract tests with the documented opt-in tests ignored, plus strict
  Clippy, Rust formatting, `git diff --check` and the TypeScript/Vite production
  build.
- Product-code head `786e1688bb0cd76930a6ec144f673487fc357178`
  recognizes singular inline `Remedy:` as the same bounded neutral section
  transition as `Remedies:`. The focused boundary probe passed; the full gate
  passed 442 library tests, 3 office tests and 3 release-contract tests with the
  documented opt-in tests ignored, plus strict Clippy, Rust formatting,
  `git diff --check` and the TypeScript/Vite production build.
- The final product-code head `7bbf862dc4ef5b4508356b52d897146b12b4be9e`
  scopes `non` to the following recognized heading modifier, so a qualified
  heading such as `Non-material risks` establishes Risk framing while direct
  negations such as `Non-risks`, `Non-risk factors`, and `No material risks`
  remain unframed. The focused two-sided boundary probe passed; the full gate
  passed 442 library tests, 3 office tests and 3 release-contract tests with the
  documented opt-in tests ignored, plus strict Clippy, Rust formatting,
  `git diff --check` and the TypeScript/Vite production build.
- Boundary tests admit the five bounded heading classes and reject missing-body,
  multi-line heading candidates, overlong, negated, uncertainty-qualified,
  solution-oriented and unrelated headings. Tests
  also prove application-owned framing of neutral General units, propagation to
  later segments and page blocks of a split section, reset at a later
  sentence-case or punctuated marked title-case solution section, retention
  across short capitalized, numbered-list and numbered sentence-case body
  paragraphs, reset across empty and
  visual-processing pages, retention across an introductory colon, single- and
  excess-newline framing and reset headings, decimal/uppercase/lowercase Roman,
  uppercase/lowercase-letter and fully parenthesized outline framing
  headings, a lowercase recognized section lead after a valid marker with
  marked lowercase and title-case prose retention, inline framing and reset headings,
  bounded mitigation/control neutral headings in standalone, marked and inline
  forms with ordinary mitigation sentences retained as body text,
  bounded benefit/advantage sentence-case resets with substantive prose
  retention,
  bounded mitigation questions with failure-qualified and substantive-question
  controls, including `may` and `might` action questions,
  rejection of standalone and
  bounded compound `Risk Factors`, `Warning Signs` and `Problem Areas`
  headings with the existing conservative modifiers, rejection of unrelated
  or uncertainty-qualified compound headings,
  inline interrogative framing headings, same-line and trailing interrogative
  reset before answer text with substantive-question retention, full-width and
  Arabic question terminals in standalone and inline headings with UTF-8-safe
  answer offsets, byte-ordered
  colon/question precedence in both directions, fail-closed
  transition-spanning sources, whole standalone-colon heading/body candidates,
  inline transition
  isolation, heading-only candidate neutrality with state propagation, marked
  standalone-colon transition at the full marker, reset at uncertain and
  negated inline simple or compound framing headings, retention
  across inline and standalone introductory labels,
  trailing inline headings, rejection of lowercase wrapped framing nouns,
  denial clearing in following-line and inline bodies with trailing prose and
  repeated determiners, conservative modifiers, compound framing nouns,
  recognized outline markers on
  bounded denials, rejection of invalid markers and marked negative rules,
  bounded copular presence and perfect existential predicates,
  bounded `to report` and bounded `Not applicable`, active-category noun
  matching, rejection of uncertainty modifiers, retention through
  interrogative denials, affirmative and unrelated post-denial contrast
  sentences, explicit same-line
  framing reintroduction with predicate-bound negation, bounded leading
  determiners and modal predicates, rejection of adjectival framing compounds,
  repeated-absence and category-mismatch rejection, modified reintroduction
  with uncertainty modifiers still excluded,
  explanatory `Not applicable because ...` and `No risks to report because ...`
  text and negative-rule, residual-risk, qualified-denial and `None of ...`
  retention,
  retention across punctuated title-case, all-caps and sentence-case body text,
  bounded mixed-framing repair grounded in the rejected untrusted draft, with
  exact valid-sibling preservation, exact per-invalid-unit source
  coverage, omission/rewrite/partial-source and repeated-response failure, distinct
  initial and expanded repair contracts, worst-permitted compatibility-group
  split capacity with Story isolation, and
  zero/exact/over-limit persisted-count boundaries and selected-window capacity
  after the transient selection labels are unavailable,
  same-line `N/A.` and marked `N.A.` clearing with interrogative, qualified,
  attached-text and explicit-reintroduction controls,
  neutral reset at uncertainty-qualified sentence-case framing headings,
  bounded `detected`, `discovered`, `remain` and `apply` denials, one bounded `currently` or
  `yet` inside an auxiliary denial predicate or around an existential auxiliary
  and `no`,
  with category-mismatch and substantive-tail rejection,
  resolved/eliminated absence continuations with `remain` and copular unresolved-risk
  reintroduction, punctuation-scoped residual-predicate negation with
  conditional-clause and same-clause controls, adverse-subject gating for
  residual possibilities with positive, recovery and protective-compound
  rejection, comma- and semicolon-coordinated denial continuations with
  additive, contrast, bare-independent-clause, noun-list and qualification
  controls, complement-aware `remain not possible` versus `remain not
  eliminated` polarity including double-negative `not impossible` inversion,
  modal and non-modal `remain` or `continue` occurrence complements bounded to
  the current sentence for both explicit framing nouns and residual subjects,
  including direct and modal `to`/copula-linked complements,
  resolved, eliminated, possible, unresolved and bare appearance complements,
  matching copular polarity for `are impossible`, `are not possible`, and
  `are not`, `were never`, or `are no longer impossible`,
  bounded residual-risk forms using `has`, `have`, `had`, copular, `cannot`, or
  modal `not be ruled out` predicates with affirmative and wrong-complement
  controls,
  modal `no longer be ruled out` predicates with wrong-complement controls,
  coordinated en/em-dash denial continuations with neutral, adverse and
  qualification controls, bounded en/em-dash inline framing transitions with
  transition-spanning rejection, and unmarked punctuated framing-phrase
  rejection with marked and colon-signaled controls,
  non-governing denied-category suspension across blank lines and normalized
  blocks, bounded next-line reintroduction, explicit section-boundary and visual
  reset, and shared synthesis/verification state reconstruction,
  and disjunctive continuations admitted only when their second disjunct is an
  independent declarative bounded denial, with ASCII, full-width and Arabic
  interrogative second disjuncts rejected, and denial/continuation splitting
  across the same supported ASCII, CJK, Arabic, Urdu, Armenian and Devanagari
  sentence-terminal family, bounded `Answer` and `Response` denial bodies, exact
  neutral and affirmative
  transitions within one denial/reintroduction line and its colon-body form,
  post-question denial answers including bounded bare `No` with interrogative,
  qualified-tail, active-category section-question, substantive-question and
  labeled-answer controls, modal-led active-category presence questions with
  reporting, causation, control-failure and category-mismatch controls,
  declarative sentence-suffix
  denials after ASCII and CJK terminals with non-denial and explicit
  reintroduction controls,
  and fail-closed
  rejection of a source candidate spanning those transitions,
  framing-repair integrity release before a subsequent modal repair,
  mixed verification context, Story isolation, and rejection of a short source
  segment spanning both problem and solution sections,
  lossy drafting-claim suppression and exact version-pair retry
  behavior. A focused live GPU probe accepted a final framed claim whose exact
  segment omitted the heading and rejected framing applied to an invented
  resolution. The full Rust library suite passed 442 tests with 13 intentional
  ignores.
- `cargo test --all-targets` passed those 442 library tests, 3 office tests and
  all 3 release-contract tests; 13 opt-in library tests and 3 opt-in office
  tests remained ignored. Strict all-target/all-feature Clippy, Rust formatting,
  the TypeScript/Vite production build and `git diff --check` passed.

**Non-scope and remaining limits**:
- This is a conservative repair for explicit leading headings in source quotes,
  not a general discourse parser. Semantic review remains necessary for framing
  expressed indirectly or across separate blocks.
- A coordinated heading that names distinct framing categories, such as `Risks
  and Limitations`, remains unframed. The source-framing contract stores one
  application-owned relationship, so selecting either category for every later
  segment would overstate the source; combined relationships require a separate
  contract and compatibility design.
- A model response whose invalid mixed-framing unit is too large to include in
  the bounded repair context still fails the run closed with
  `SYNTHESIS_REPAIR_INPUT_TOO_LARGE`; this slice does not add an automatic
  whole-run retry policy. Follow-up is tracked in issue #52.
- A long-document General response that combines sources from distinct selection
  windows still fails closed with `MODEL_SUMMARY_RESPONSE_WINDOW_MIXED`; this
  slice does not add a model retry or redesign selection windows. Follow-up is
  tracked in issue #53.
- This slice does not change Story or Contract behavior, routing, UI, persistence
  schemas, source selection, ingestion, OCR or model configuration.
