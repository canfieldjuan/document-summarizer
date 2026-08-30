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

## Cross-App Product Direction: Connect and Shared Inference Are Separate Boundaries

**Status**: Architectural distinction accepted; shared on-prem inference remains
future work

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
- An Ollama runtime serving Qwen3 30B-A3B is the selected operational direction.
  LM Studio and llama.cpp are not current deployment targets; compatibility with
  an OpenAI-compatible protocol does not imply product support for every server
  implementing that protocol. The operator's separate Email Watcher evaluation
  informed this selection, but Document Summarizer acceptance remains a distinct
  deployment proof rather than behavior proved by this entry.

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
  proof. The inference gateway, administrator UI, appliance packaging,
  multi-model routing, capacity policy, and cross-machine discovery remain
  separately scoped follow-up work.

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
