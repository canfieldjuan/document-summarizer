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
- Startup executes remote OCR submission/status polling inside Tauri's synchronous setup closure, so a persisted processing job can block application initialization for the full request deadline.
- OCR submission and polling do not receive the desktop worker's cancellation control, so cancellation can terminalize the root while leaving a pre-admission handoff recoverable against a root that can no longer admit its child.
- Detached startup recovery does not reserve its pre-admission root IDs in the desktop active registry, so a user continuation can race the OCR owner before child admission.
- The registry reservation ends when a recovery attempt exits, but a pending handoff can outlive that attempt. The generic continuation path does not check durable handoff ownership, so it can advance the root after a provider outage and strand the saved job.
- OCR HTTP submit/status uses the full phase deadline as one blocking call timeout, so cancellation is not observed until a stalled call returns.
- Root-ID status reads project to the admitted child before the child worker enters the active registry; the status command checks only the projected child ID, so a poll in that interval reports an inactive background job and stops monitoring despite the still-active root owner.
- The `output_ready` coordinator commits an ingestible child before materializing its retained OCR PDF. A snapshot write failure leaves the committed child pointing at a missing source, while the handoff has already moved to `child_admitted`.
- A bounded OCR HTTP call maps a timeout to `Uncertain`, but the coordinator returns immediately rather than retrying within its phase deadline. The durable handoff then has no active same-session worker.
- Detached startup recovery rewinds an admitted child before claiming its active-registry ID. A user continuation can own that child concurrently, lose its state-version race to the rewind, and leave no worker after recovery's late claim fails.
- The current installed OCR provider isolates retained source graphics with a paired `q /Artifact BMC` and `EMC Q` wrapper, while the existing tagged parser admits only the earlier wrapper without graphics-state isolation.
- The provider's canonical tagged text preserves ASCII tabs and line feeds, while the generic PDF string decoder drops those control bytes and makes a valid PDF/text pair fail exact validation.
- The gateway client forwards the canonical synthesis schema unchanged, including `uniqueItems`, while the accepted gateway contract rejects that decoder-unsupported keyword with HTTP 422; the existing direct-runtime adapter already projects it away without weakening Rust validation.
- A provider can send HTTP headers and then stall while sending its body. The body-read timeout is classified as malformed output, so the durable coordinator exits instead of reconciling an uncertain transport result.
- The direct normalization command reaches the shared `Parsed` to `Normalizing` store transition without checking whether an OCR handoff owns the root. A pending OCR root can advance and permanently lose child-admission eligibility.
- Live OCR child admission makes the child visible as `Ingested`, but the root worker transitions it to `Parsing` before claiming its active-registry ID. A competing continuation can claim first; both workers then exit on ownership loss and leave the child without an active worker.
- The loopback oversized-response test sends a response before reading the client's request, then closes immediately. Native Windows CI failed its invalid-response assertion, and a held-open variant also returned a send error on Linux. The fixture does not reliably reach the bounded-header decision.

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
12. Keep application setup free of OCR network waits by dispatching persisted handoff recovery to a named background task after local interrupted-run reconciliation.
13. Propagate cancellation through OCR coordination, terminalize the owned pre-admission handoff without admitting a child, and let the existing worker finalizer complete the root cancellation.
14. Reserve every pre-admission recovery root in the desktop active registry before dispatch and release the reservation when recovery exits. Also reject generic root continuation while any durable OCR handoff owns that root, including after recovery exits.
15. Bound individual OCR HTTP calls well below the overall phase deadline so a blocked call returns to the cancellation checkpoint promptly.
16. Keep projected child status active while either the original root owner or child worker is active, using one registry snapshot; offer child cancellation only after the child worker itself is active.
17. Materialize and durably sync the retained OCR PDF before child admission. If materialization fails, leave the handoff `output_ready` with no child; retry against the same retained output and child identity after storage recovers.
18. Retry uncertain submit/status calls with bounded backoff and cancellation checks until the existing phase deadline, preserving the durable phase and reconciling a saved job by status before any replay.
19. Claim an admitted child in the desktop active registry before any restart rewind, and transfer that claim to the resumed worker without an unowned interval. If a user worker already owns the child, recovery must not rewind it.
20. Classify response-body transport timeouts as uncertainty while retaining invalid classification for oversized or malformed complete responses, so the existing bounded retry and status-before-replay logic runs.
21. Reject normalization of a root with any durable OCR handoff inside the shared immediate store transition, while leaving native roots and admitted OCR children eligible.
22. Claim a live-admitted OCR child before its first `Parsing` transition, hold that claim through worker dispatch, and release or fail the child consistently on errors. A competing continuation must not lose a state-version race to an unowned transition.
23. Make the oversized-response loopback fixture read the request and send a complete response that exceeds a small test limit, so the real response classifier is exercised without an incomplete HTTP exchange or changes to production transport behavior.

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
4. Extend startup recovery so local interrupted-run reconciliation completes first, then persisted OCR phases resume on a named background task.
5. Add focused migration, boundary, lost-acknowledgement, exact-instance, reopen, and completed-child tests.
6. Align the tagged-parser content wrapper with the current installed provider and prove the old and new paired forms without widening other structural admission.
7. Align gateway transport with the existing decoder-schema projection so an admitted OCR child can complete its summary without relaxing canonical response validation.
8. Reconcile exact-head review findings by preserving active OCR child ownership through restart, counting only visible history rows toward the limit, and scoping Unix-only code out of the Windows build.
9. Reconcile exact-head review findings by moving remote restart recovery off synchronous setup and making OCR waiting cancellation-aware through terminal handoff ownership.
10. Reconcile exact-head review findings by reserving pre-admission roots across detached recovery and terminalizing cancelled handoffs before provider discovery.
11. Reconcile the durable-root and stalled-HTTP review findings without changing product-facing output.
12. Reconcile the root-to-child monitoring gap while preserving the existing cancellation target and UI copy.
13. Reconcile snapshot-write failure before child admission without changing the OCR wire or derived document contracts.
14. Reconcile same-session transport uncertainty and the admitted-child recovery race without changing public contracts.
15. Reconcile response-body timeout classification and direct normalization of OCR-owned roots at their shared boundaries.
16. Close the live child-admission ownership gap and make the bounded-response test stable on native Windows.

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
11. Tauri setup performs no OCR provider transport or polling; it schedules a named background recovery task and remains available while that task is blocked.
12. A cancellation committed while OCR is pending records a non-retryable terminal handoff, admits no child, and lets the existing root cancellation transition finish without later startup replay.
13. Startup reserves each recoverable pre-admission root before the detached task can run, rejects a second owner while recovery is blocked, and releases the reservation after recovery exits.
14. A pre-admission OCR handoff that remains after startup recovery exits still prevents generic continuation of its root, and history does not advertise that root as continuable.
15. A stalled submit or status transport call returns control to the cancellation checkpoint within a bounded per-call timeout, rather than waiting for the full phase deadline.
16. A status poll by the originally accepted root ID remains background-active between child admission and child worker registration; the projected child becomes cancellable only after its own active registration.
17. A failed derived-PDF materialization leaves the handoff `output_ready`, exposes no child or lineage, and can be retried after the storage obstruction clears to admit exactly one child whose source bytes match the retained PDF.
18. A transient uncertain submit or status call retries in the same coordinator invocation, checks cancellation, and never replays a saved job without a status `NotFound` response; repeated uncertainty stops at the existing phase deadline.
19. A competing child continuation prevents recovery from rewinding its state; when recovery claims first, the claim remains exclusive through rewind and worker dispatch, including error cleanup.
20. A loopback provider that sends headers then stalls its body produces `Uncertain` and stays on the existing bounded same-session reconciliation path; oversized complete responses remain invalid.
21. The direct `normalize_document` path cannot advance a parsed root with a persisted OCR handoff, and its state/version remain unchanged; a native root without a handoff can still normalize.
22. Within the desktop manager's shared mutex-backed active registry, competing root/user worker claims are serialized: if a competing owner claims an admitted `Ingested` child first, the root worker leaves its state/version unchanged; if the root worker claims first, it holds the claim through `Parsing` and worker dispatch. The controlled competing-claim test samples both sides of that invariant.
23. The loopback oversized-response test consumes the request headers and sends a complete body larger than its bounded test limit; native Windows Connect CI passes that assertion.

Affected surfaces: Local Connect v2 discovery/client transport, SQLite migration and transactions, desktop background scheduling, application startup recovery, native scanned-PDF routing.

Risk areas: lost acknowledgements, provider replacement, duplicate child admission, original-byte retention, malformed provider output, startup ordering, blocking transport deadlines, cancellation races.

Triggered review rules: requirements match, test evidence, security/authentication, data and migration safety, backward compatibility, error handling, concurrency/idempotency, dependencies/configuration, deployment/startup safety, codebase verification.

Reachability proof: start from the real desktop `summarize_document` admission, observe a persisted OCR handoff and exact-provider job, restart through the application recovery owner, then observe one child pipeline run with `OcrText` and one completed persisted summary.

## Mechanism

The OCR handoff row is the single recovery owner. It stores a bounded private source snapshot in SQLite so admission and deletion exclusion are one transaction. Its phase moves monotonically from prepared through uncertain/running/output-ready to child-admitted/completed or failed. Network calls happen only after the transaction commits. Completed output admission stores the bounded OCR PDF and creates the derived document/run plus lineage in one immediate transaction. Startup reads nonterminal rows, rediscovers live providers, matches the persisted instance exactly, and advances the same coordinator state.

Workspace status and summary reads project an admitted root onto its stable child identity. The internal root remains preserved for audit, but it is hidden from recent history after child admission so the unchanged UI follows the child worker and result.

Generic interrupted-run recovery leaves active admitted OCR children under their handoff owner. The OCR restart path atomically rewinds an interrupted active state to the immediately preceding persisted checkpoint, records that transition, and dispatches the existing continuation worker against the same run ID and incremented state version. Both successful terminal states close the handoff. Recent-history SQL excludes admitted/completed roots before ordering and limiting, avoiding both capacity loss and a per-row projection query.

Direct OCR remains unavailable on non-Unix systems. Platform-specific discovery and private-file materialization are separate compile-time functions, while shared retained-output validation still runs before the unsupported-platform response.

Application setup performs only local database reconciliation, constructs the desktop manager, and schedules a named OCR recovery task. That task owns provider discovery, remote status polling, warning emission, and child resumption without delaying Tauri initialization.

Before dispatch, startup reads the durable recoverable handoffs and reserves every pre-admission root in the desktop active registry. A scoped reservation guard releases those roots when the recovery task returns or unwinds, while duplicate continuation admission fails against the same registry entry during polling.

The registry reservation protects the active attempt, while a SQLite existence check protects durable root ownership after that attempt exits. Both generic continuation admission and recent-history action eligibility consult the handoff root key; admitted children remain eligible for their own pipeline continuation. The check reads only existence, not retained source bytes.

The root worker's cancellation token is passed through the OCR coordinator. Each loop boundary and each completed transport attempt rechecks cancellation plus the persisted root state. Before child admission, cancellation compare-and-sets the current handoff phase to terminal non-retryable failure and returns the existing cancellation signal, so finalization completes the root as `Cancelled` and restart cannot replay the abandoned handoff.

Each blocking HTTP attempt uses the lesser of two seconds and the remaining phase deadline, returning control to the cancellation checkpoint even when a provider accepts a connection but never answers. The overall phase deadline remains five minutes.

The desktop registry reads original-root and projected-child activity under one lock. Root activity keeps a projected status poll alive during worker handoff; only child activity enables cancellation of the projected child. The existing monitor and command payload shape stay unchanged.

The output-ready coordinator first materializes and syncs the retained PDF to its preassigned private path, then commits the child document/run and lineage. A failed filesystem write therefore leaves durable output-ready state without a runnable child. If the database commit fails after the file is present, the next attempt verifies and re-syncs the same file before retrying the same child identity.

An uncertain transport response leaves the saved phase unchanged and returns through a short bounded backoff to the next cancellation and deadline checkpoint. Submission uncertainty reconciles by status before any replay; a repeated timeout cannot busy-spin or extend the five-minute deadline.

The bounded response reader distinguishes transport timeouts, including reqwest errors wrapped in an I/O error, from complete invalid responses. The shared normalization transaction checks durable handoff ownership before the state transition, so direct and coordinated callers cannot bypass the same root boundary.

Child restart recovery claims the child ID in the same active registry used by user continuation before it rewinds SQLite state. The claim stays present while runtime construction and worker dispatch occur, then transfers to the worker; error paths release only the claim they own.

The live root worker likewise claims the admitted child before loading and transitioning it to `Parsing`. A scoped claim stays armed until the child worker starts; a worker-start failure releases the claim and records the existing recoverable background-start failure. The loopback oversized-response fixture first consumes the complete request headers, then sends a complete small response through the same HTTP client and classifier with a small test limit; the production limit and transport branches remain unchanged.

Restart recovery checks the persisted root cancellation state before provider selection, so a cancelled pre-admission handoff becomes terminal even when its pinned provider is offline.

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

- Fail-first `cargo test --locked --lib desktop::tests::live_ocr_child_is_claimed_before_parsing_and_worker_dispatch -- --exact` failed because the claim-before-transition helper was absent; after the repair, 1 passed. The test covers a competing first claim without state/version mutation, then successful claimed dispatch through completion.
- Native Windows Connect security CI on the previous head failed `stalled_response_body_is_uncertain_but_oversized_body_is_invalid` at the oversized-response assertion. A held-open but premature-response variant also returned `Uncertain` on Linux; after the complete-request/complete-response fixture repair, the targeted test and all 11 OCR consumer tests pass locally. Native Windows CI on the new head remains the required cross-platform proof.
- `cargo test --locked --lib desktop::tests` - 18 passed; `cargo fmt --all -- --check`, strict Linux Clippy, and strict Windows-target Clippy passed after these repairs.
- Fail-first `cargo test --locked --lib connect::ocr_consumer::tests::stalled_response_body_is_uncertain_but_oversized_body_is_invalid -- --exact` failed because the stalled body was not `Uncertain`; after the repair, 1 passed, including the oversized negative case.
- Fail-first `cargo test --locked --lib connect::ocr_consumer::tests::direct_normalization_cannot_advance_an_ocr_owned_root -- --exact` failed because the direct path advanced the root; after the transactional guard, 1 passed with unchanged run and handoff.
- `cargo test --locked --lib connect::ocr_consumer::tests` - 11 passed; `cargo test --locked --lib pipeline::normalize::tests` - 10 passed.
- `cargo fmt --all -- --check`, `cargo clippy --locked --lib --tests -- -D warnings`, and `cargo clippy --locked --target x86_64-pc-windows-gnu --all-targets -- -D warnings` - passed after the review repairs.
- Fail-first `cargo test --locked --lib connect::ocr_consumer::tests::lost_acknowledgement_reconciles_before_one_child_admission_after_reopen -- --exact` failed at the expected `InvalidOutput` assertion because the first uncertain submit returned before status reconciliation; after the repair, 1 passed.
- Fail-first `cargo test --locked --lib desktop::tests::resumed_ocr_child_rewinds_active_stage_and_completes_same_run -- --exact` failed with `Parsed` instead of the live `Normalizing` state after an `AlreadyRunning` result; after the claim-before-rewind repair, 1 passed.
- `cargo test --locked --lib connect::ocr_consumer::tests` - 9 passed, including same-invocation completion after transient submit/status uncertainty, short-deadline repeated uncertainty, cancellation, and reopen reconciliation.
- `cargo test --locked --lib desktop::tests` - 17 passed, including rejection before a competing child's rewind and cleanup of a failed resume claim.
- `cargo fmt --all -- --check`, `cargo clippy --locked --lib --tests -- -D warnings`, and `cargo clippy --locked --target x86_64-pc-windows-gnu --all-targets -- -D warnings` - passed after the review repairs.
- Fail-first `cargo test --locked --lib connect::ocr_consumer::tests::snapshot_failure_does_not_admit_child_and_recovery_retries_same_output -- --exact` - failed with persisted `child_admitted` instead of `output_ready`; after the ordering repair, 1 passed. The test covers failed snapshot creation, failed admission after successful sync, and one-child recovery from retained output.
- `cargo test --locked --lib connect::ocr_consumer::tests` - 8 passed; `cargo test --locked --lib desktop::tests` - 16 passed. `cargo fmt --all -- --check`, Linux strict Clippy, and Windows-target strict Clippy passed after the repair.
- `cargo test --lib desktop::tests::startup_ocr_recovery_reserves_roots_until_task_finishes` - 1 passed after a fail-first missing-method compile error; covers root-only handoff, both owners active, child-only activity, and neither active.
- `cargo fmt --all -- --check`, `cargo clippy --locked --lib --tests -- -D warnings`, and `cargo clippy --locked --target x86_64-pc-windows-gnu --all-targets -- -D warnings` - passed after the status repair.
- `cargo test --locked --lib connect::ocr_consumer::tests -- --nocapture` - 7 passed, including durable-root exclusion after unavailable-provider recovery and a real stalled loopback status request bounded below the phase deadline.
- `cargo test --locked --lib pipeline::service::tests::continuation_rejects_stale_missing_runtime_active_failed_and_terminal_runs -- --exact` - 1 passed.
- `cargo test --locked --lib desktop::tests::pending_ocr_recovery_keeps_the_root_checkpoint_resumable -- --exact` - 1 passed.
- `cargo clippy --locked --lib --tests -- -D warnings` - passed after the review repairs.
- `cargo clippy --locked --target x86_64-pc-windows-gnu --all-targets -- -D warnings` - passed after the review repairs.
- `cargo test pipeline::schema::tests --lib` - 13 passed.
- `cargo test --lib pipeline::recovery::tests` - 8 passed after active OCR child ownership and stable-checkpoint rewind repair.
- `cargo test --lib pipeline::workspace::tests` - 11 passed after visible-history limiting repair.
- `cargo test --lib desktop::tests` - 16 passed, including same-run active OCR child restart, nonblocking startup dispatch, and synchronous root reservation that rejects overlapping worker admission.
- `cargo test pipeline::parser::tests --lib` - 26 passed after installed-provider wrapper and ASCII-control repairs.
- `cargo test pipeline::gateway_client::tests --lib` - 27 passed after fail-first decoder-schema projection repair.
- `cargo test pipeline::model::tests::decoder_projection --lib` - 1 passed.
- `cargo test pipeline::model::tests::structured_response_format_projects_only_unsupported_decoder_keywords --lib -- --exact` - 1 passed.
- `cargo clippy --lib --tests -- -D warnings` - passed.
- `cargo clippy --all-targets -- -D warnings` - passed on Linux.
- `cargo clippy --target x86_64-pc-windows-gnu --all-targets -- -D warnings` - passed.
- `cargo test --locked --lib connect::provider::tests::entitlement_gates_manifest_jobs_and_status_while_registration_stays_owned -- --exact` - 1 passed while reproducing the unrelated Windows CI failure locally.
- `cargo fmt --all --check` - passed.
- `git diff --check` - passed.
- `npm run desktop:build` - passed and emitted `Document Summarizer_0.1.0_amd64.deb`; package SHA-256 `3420cad5d20ea5acb182945161cb4f1eb944b17b8ac3c0a4cfe6221c829c37a6`.
- Installed Debian-package scanned-PDF proof - input SHA-256 `1d65427c5812a77373d3c137b8153a6001a9bbf7213134064b98effdbd6ab2b6`; installed/package-member binary SHA-256 `008c0f19226575f72e483f6a6ba4626208df884a51ff041ffb3427b7dc52449f`; handoff `276ea497-e489-44df-9fc3-693cba77103e` completed provider job `69cfe654-3008-4aef-966d-b7dc074a4c30` and child run `bee32a6b-5652-4c94-8bcd-6c01b3970634` with one lineage edge and one persisted summary.
- Installed restart proof - reopening the same isolated profile retained the same IDs with 2 total runs, 1 handoff, 1 lineage edge, and 1 summary; no duplicate provider job or child was created.

## Estimated diff size

Actual with exact-head review repairs: 14 files, +4,325 / -105. The slice exceeds the usual soft cap because strict discovery, bounded HTTP transport, durable crash ownership, status-before-replay reconciliation, paired-output validation, migration, transactional child admission, startup recovery, workspace projection, gateway-compatible synthesis, and their boundary tests form one indivisible vertical safety boundary; omitting any one recreates the original blocker, breaks the existing desktop monitor, or violates accepted ADR-0008.
