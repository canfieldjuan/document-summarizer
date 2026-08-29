# Build Ledger

This document tracks the vertical slices and architectural decisions for the Document Summarizer.

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
