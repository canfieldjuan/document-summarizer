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

**Status**: implementation and local verification complete; hosted checks and
review pending

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
