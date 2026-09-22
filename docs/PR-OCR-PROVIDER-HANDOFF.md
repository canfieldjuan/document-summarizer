# Direct OCR provider handoff and restart reconciliation

## Why this slice exists

Document Summarizer can validate and summarize an already-derived OCR PDF, but a directly admitted scanned PDF remains on the native parser path. The native parser records `NO_NATIVE_TEXT_IN_DOCUMENT`; no consumer-owned client discovers `document.ocr`, persists a stable provider request, or admits the provider's OCR PDF as a child run.

### Problem-derived contract

Root cause:

- The desktop worker has no transition from the native parser's whole-document no-text signal into the accepted OCR capability.
- Connect code is provider-only. No consumer-owned store pins the selected OCR provider instance, original bytes, remote job identity, returned OCR artifact, child pipeline identity, or recovery phase.
- Startup recovery only reconciles interrupted pipeline runs, so a lost OCR submission response or restart cannot resume the same remote job and child.
- Generic interrupted-run recovery claims active OCR children before the OCR restart owner can resume them, converting the child to `Failed`; the desktop resume path also accepts only the initial `Ingested` checkpoint.
- Recent history applies its 30-row limit before admitted OCR roots are suppressed, so internal roots consume visible document capacity.
- Unix-only discovery and snapshot operations remain in the non-Unix compile surface, making the required Windows warning gate fail even though direct OCR remains intentionally disabled there.
- The current installed OCR provider isolates retained source graphics with a paired `q /Artifact BMC` and `EMC Q` wrapper, while the existing tagged parser admits only the earlier wrapper without graphics-state isolation.
- The provider's canonical tagged text preserves ASCII tabs and line feeds, while the generic PDF string decoder drops those control bytes and makes a valid PDF/text pair fail exact validation.
- The gateway client forwards the canonical synthesis schema unchanged, including `uniqueItems`, while the accepted gateway contract rejects that decoder-unsupported keyword with HTTP 422; the existing direct-runtime adapter already projects it away without weakening Rust validation.

The correct fix must:

1. Detect the persisted native parse result whose complete document has no native text.
2. Discover exactly one live provider whose registration and manifest match `document.ocr` 1.0, then pin its app and durable instance identity.
3. Atomically retain the exact original bytes, stable request, remote job ID, lineage identity, and pre-dispatch phase before the first network request.
4. Reconcile status against that exact provider instance before replay after an uncertain submission; never send the saved job to a replacement instance.
5. Validate the completed paired OCR outputs, retain the exact OCR PDF, and atomically admit one `SourceType::OcrText` child document/run with one immutable lineage edge.
6. Resume the same persisted phase during application startup and let the existing worker/pipeline owners complete the child summary.
7. Admit the current provider's exact paired graphics-state wrapper while retaining backward compatibility for already-stored legacy OCR artifacts and rejecting mixed wrappers.
8. Preserve the provider's ASCII tagged-text bytes, including tab and line-feed controls, while retaining BOM-aware decoding for non-ASCII PDF strings.
9. Reuse the existing non-mutating decoder-schema projection for gateway requests so unsupported decoder keywords are removed only from the transported copy and the canonical Rust validation contract remains unchanged.
10. Preserve admitted OCR child ownership across generic restart recovery, rewind any interrupted active stage to its last durable checkpoint, and continue the same child run through the existing continuation worker.
11. Exclude admitted/completed OCR roots before applying the recent-history SQL limit, and compile Unix-only OCR discovery/snapshot code only on Unix without enabling Windows OCR.

Must not change:

- Connect v2 wire schemas or strict `derived_from` rejection.
- The existing Document Summarizer provider in `connect/provider.rs`.
- Native-text parsing, tagged OCR logical extraction, model selection, summary output shape, or customer-facing UI layout/copy.
- Contract fixtures, deferred malformed-input hardening, Windows behavior, or unrelated provider lifecycle work.

## Scope (this PR)

Ownership lane: scanned-ocr-blocker

Slice phase: direct OCR consumer vertical proof

1. Add one consumer-owned OCR discovery, transport, validation, and phase coordinator module.
2. Add one additive schema migration and transactional store functions for the OCR handoff and child lineage.
3. Route the existing whole-document native no-text result into that coordinator and schedule the admitted child through the existing desktop worker.
4. Extend the existing startup recovery owner to reconcile persisted OCR phases before normal interrupted-run recovery.
5. Add focused migration, boundary, lost-acknowledgement, exact-instance, reopen, and completed-child tests.
6. Align the tagged-parser content wrapper with the current installed provider and prove the old and new paired forms without widening other structural admission.
7. Align gateway transport with the existing decoder-schema projection so an admitted OCR child can complete its summary without relaxing canonical response validation.
8. Reconcile exact-head review findings by preserving active OCR child ownership through restart, counting only visible history rows toward the limit, and scoping Unix-only code out of the Windows build.

### Files touched

- `src-tauri/src/connect/mod.rs`
- `src-tauri/src/connect/ocr_consumer.rs`
- `src-tauri/src/desktop.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/pipeline/gateway_client.rs`
- `src-tauri/src/pipeline/model.rs`
- `src-tauri/src/pipeline/db.rs`
- `src-tauri/src/pipeline/parser.rs`
- `src-tauri/src/pipeline/recovery.rs`
- `src-tauri/src/pipeline/schema.rs`
- `src-tauri/src/pipeline/service.rs`
- `src-tauri/src/pipeline/state.rs`
- `src-tauri/src/pipeline/workspace.rs`
- `docs/PR-OCR-PROVIDER-HANDOFF.md`

### Review Contract

Acceptance criteria:

1. A whole-document `NO_NATIVE_TEXT_IN_DOCUMENT` parse result is the only native path that admits an OCR handoff; native documents and partial empty-page warnings remain on the existing path.
2. The admission transaction contains one stable provider job identity, canonical request, exact selected provider app/instance, exact original bytes and digest, and one recovery phase before the transport is called.
3. An uncertain submission performs `GET /v2/jobs/{stable-id}` before any same-request replay; a missing selected instance leaves the row recoverable and sends no request to another instance.
4. A valid completed OCR response is selected by exact media type, validates paired output cardinality/integrity and tagged PDF canonical text, then commits one lineage edge and one `OcrText` child identity.
5. Reopening the database and running startup reconciliation preserves an active OCR child from generic interruption failure, rewinds it to the last persisted stable checkpoint, and resumes the same child without a duplicate remote job, lineage edge, document, or run.
6. The admitted child selects `TaggedOcrParser` through its persisted `SourceType::OcrText` and reaches one completed summary under the existing pipeline service in the end-to-end test.
7. Status and summary reads for the originally accepted run resolve to the admitted child, while recent history excludes admitted/completed roots before its 30-row SQL limit, so the existing UI monitors the real completed summaries at full capacity rather than rendering handoff markers as failures.
8. The tagged parser admits both the legacy `Artifact BMC` / `EMC` pair and the current `q` + `Artifact BMC` / `EMC` + `Q` pair, while a mixed pair remains `OCR_STRUCTURE_INVALID`.
9. Tagged `ActualText` made only of ASCII bytes preserves tabs and line feeds byte-for-byte, so canonical PDF text equals the provider's paired UTF-8 text output.
10. Gateway request serialization removes nested decoder-unsupported `uniqueItems` and oversized decoder-only string bounds from its copied response schema while leaving the caller's canonical schema byte-for-byte unchanged.

Affected surfaces: Local Connect v2 discovery/client transport, SQLite migration and transactions, desktop background scheduling, application startup recovery, native scanned-PDF routing.

Risk areas: lost acknowledgements, provider replacement, duplicate child admission, original-byte retention, malformed provider output, startup ordering, blocking transport deadlines.

Triggered review rules: requirements match, test evidence, security/authentication, data and migration safety, backward compatibility, error handling, concurrency/idempotency, dependencies/configuration, deployment/startup safety, codebase verification.

Reachability proof: start from the real desktop `summarize_document` admission, observe a persisted OCR handoff and exact-provider job, restart through the application recovery owner, then observe one child pipeline run with `OcrText` and one completed persisted summary.

## Mechanism

The OCR handoff row is the single recovery owner. It stores a bounded private source snapshot in SQLite so admission and deletion exclusion are one transaction. Its phase moves monotonically from prepared through uncertain/running/output-ready to child-admitted/completed or failed. Network calls happen only after the transaction commits. Completed output admission stores the bounded OCR PDF and creates the derived document/run plus lineage in one immediate transaction. Startup reads nonterminal rows, rediscovers live providers, matches the persisted instance exactly, and advances the same coordinator state.

Workspace status and summary reads project an admitted root onto its stable child identity. The internal root remains preserved for audit, but it is hidden from recent history after child admission so the unchanged UI follows the child worker and result.

Generic interrupted-run recovery leaves active admitted OCR children under their handoff owner. The OCR restart path atomically rewinds an interrupted active state to the immediately preceding persisted checkpoint, records that transition, and dispatches the existing continuation worker against the same run ID and incremented state version. Both successful terminal states close the handoff. Recent-history SQL excludes admitted/completed roots before ordering and limiting, avoiding both capacity loss and a per-row projection query.

Direct OCR remains unavailable on non-Unix systems. Platform-specific discovery and private-file materialization are separate compile-time functions, while shared retained-output validation still runs before the unsupported-platform response.

Document Summarizer's tagged-PDF parser exposes one crate-private canonical-text function so the consumer can byte-compare the paired provider text before child admission without importing provider code or introducing a second traversal implementation inside this application. Its tagged-profile decoder preserves ASCII bytes, including tabs and line feeds, and falls back to the existing BOM-aware PDF string decoder for non-ASCII strings.

Gateway request construction reuses the direct runtime's recursive decoder-compatible schema projection. Request hashing and transport use the projected copy, while the caller-owned canonical schema remains available for authoritative Rust validation after generation.

## Intentional

- Provider selection is deterministic only when exactly one conforming live OCR instance exists; zero or multiple candidates fail closed without choosing by directory order.
- The selected provider token and port are not persisted. Recovery rediscovers the same durable instance and uses its current owner-private registration.
- The original native run remains the workflow root and records that processing moved to a derived child; the child owns the summary artifacts.
- No new UI, provider selector, shared workflow grammar, or provider-side lineage field is introduced.

## Deferred

Parking predicate: malformed-input hardening, optional multi-provider UI, Windows discovery, and presentation polish that do not block the supported installed Linux path remain parked.

Parked hardening: the items already listed in `HANDOFF-2026-09-21-SCANNED-OCR.md`; none are reopened here.

## Verification

- `cargo test connect::ocr_consumer::tests --lib` - 4 passed.
- `cargo test pipeline::schema::tests --lib` - 13 passed.
- `cargo test --lib pipeline::recovery::tests` - 8 passed after active OCR child ownership and stable-checkpoint rewind repair.
- `cargo test --lib pipeline::workspace::tests` - 11 passed after visible-history limiting repair.
- `cargo test --lib desktop::tests` - 14 passed, including same-run active OCR child restart through successful handoff completion.
- `cargo test pipeline::parser::tests --lib` - 26 passed after installed-provider wrapper and ASCII-control repairs.
- `cargo test pipeline::gateway_client::tests --lib` - 27 passed after fail-first decoder-schema projection repair.
- `cargo test pipeline::model::tests::decoder_projection --lib` - 1 passed.
- `cargo test pipeline::model::tests::structured_response_format_projects_only_unsupported_decoder_keywords --lib -- --exact` - 1 passed.
- `cargo clippy --lib --tests -- -D warnings` - passed.
- `cargo clippy --all-targets -- -D warnings` - passed on Linux.
- `cargo clippy --target x86_64-pc-windows-gnu --all-targets -- -D warnings` - passed.
- `cargo fmt --all --check` - passed.
- `git diff --check` - passed.
- `npm run desktop:build` - passed and emitted `Document Summarizer_0.1.0_amd64.deb`; package SHA-256 `3420cad5d20ea5acb182945161cb4f1eb944b17b8ac3c0a4cfe6221c829c37a6`.
- Installed Debian-package scanned-PDF proof - input SHA-256 `1d65427c5812a77373d3c137b8153a6001a9bbf7213134064b98effdbd6ab2b6`; installed/package-member binary SHA-256 `008c0f19226575f72e483f6a6ba4626208df884a51ff041ffb3427b7dc52449f`; handoff `276ea497-e489-44df-9fc3-693cba77103e` completed provider job `69cfe654-3008-4aef-966d-b7dc074a4c30` and child run `bee32a6b-5652-4c94-8bcd-6c01b3970634` with one lineage edge and one persisted summary.
- Installed restart proof - reopening the same isolated profile retained the same IDs with 2 total runs, 1 handoff, 1 lineage edge, and 1 summary; no duplicate provider job or child was created.

## Estimated diff size

Actual with exact-head review repairs: 14 files, +3,070 / -89. The slice exceeds the usual soft cap because strict discovery, bounded HTTP transport, durable crash ownership, status-before-replay reconciliation, paired-output validation, migration, transactional child admission, startup recovery, workspace projection, gateway-compatible synthesis, and their boundary tests form one indivisible vertical safety boundary; omitting any one recreates the original blocker, breaks the existing desktop monitor, or violates accepted ADR-0008.
