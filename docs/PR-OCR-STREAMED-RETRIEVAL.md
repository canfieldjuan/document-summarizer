# OCR streamed retrieval consumer

## Root cause

At `6941c60`, the OCR consumer discovers only v2/profile 1.0, decodes inline
base64, and limits the derived PDF to 2 MiB. The durable handoff already owns
the exact request, provider instance, source bytes, completed status, paired
output bytes and one child. These are the owners for v3 recovery too.

Authority: merged connect-contracts ADR-0010 and v3 schemas at
`5e74cf650df07df22d1cff60d35f678601c7cfc1`; document-ocr provider PR17 merged as
`48ba7458e64a51037e6ab335f5c312d6ac137f87`.

## Required change surface

1. Prefer a conforming v3/profile 1.1 registration for a new handoff, with v2
   permitted when only v2 is available. Two registrations for the same instance
   are one provider. Pin protocol in the existing saved request. Recovery must
   select that same app, instance and protocol, with no automatic downgrade.
2. Share the existing strict job envelope across output representations while
   keeping the v2 output type/default and wire behavior unchanged. V3 has only
   closed descriptors, never an inline/stream union. Bound JSON before parsing
   and reject duplicate members, non-finite numbers and depth above 16.
3. Validate status identity, terminal shape and both descriptors before any
   output GET. Persist completed metadata in the existing running handoff before
   retrieval. A saved completed descriptor cannot change on reconciliation.
4. Fetch each output sequentially from the validated registration origin and
   canonical IDs. No proxies, redirects, decompression or supplied URL/path.
   Require exact HTTP 200/octet-stream/no-store/one decimal Content-Length and
   forbid transfer/content encoding and filename headers. Count and hash chunks
   of at most 64 KiB into an owner-private temporary file. Reject malformed,
   short, excess or corrupt bodies before parsing or publishing either sibling.
5. Keep ADR-0010's 60-second transfer deadline and stricter parent deadline.
   At most three attempts per artifact/cycle; one-second transient backoff.
   Retries reuse descriptors from byte zero; discard partial files. Restart or
   stale-token recovery validates the same instance's manifest/status and exact
   descriptors before retrieval. Never rerun OCR to recover a completed output.
6. Apply profile 1.1's 36 MiB PDF and unchanged 256 KiB text caps. Feed only
   verified bytes through the existing canonical tagged-PDF/text pair validator,
   atomic paired storage, private materialization and single child admission.
   Source kind stays OcrText. Existing warnings, corrections and summary checks
   remain authoritative. The direct child path has no lower configurable input
   cap; generic Connect input admission is a different path and stays unchanged.
7. Reuse the current cancellation, phase ownership and restart coordinator.
   A malformed retrieval terminalizes only the consumer handoff with a visible
   error; transient exhaustion keeps it recoverable. No partial child or extra
   summary can be admitted. No network I/O holds a database write transaction.

## Explicit non-scope

No provider changes, new service/queue, model selection,
prompt, summary scoring, OCR engine, correction UI, native PDF admission,
Windows discovery enablement, dependency version updates or invoice consumer changes.
The existing Unix-only direct OCR discovery boundary is preserved, including
Windows compilation and current v1/v2 provider behavior.

## Assumptions and blockers

The provider is merged. The protocol already lives in the saved request;
the existing running status column can retain completed descriptors until both
verified outputs are atomically stored, so no new persisted phase is needed. The existing PDF-size CHECK does require a schema migration.
Temporary downloads are anonymous private files and do not survive process
exit. The bounded parser/storage may allocate verified bytes up to the declared
cap; this is not a streaming PDF parser or model qualification claim.

## Verification plan

- Fail-first v3 discovery/profile and a real HTTP output above 2 MiB through the
  coordinator; existing v2 fixture/recovery paths remain unchanged.
- Pin canonical v3 fixtures. Reject malformed descriptors, wrong provenance,
  aliases, malformed metadata and mixed pairs before retrieval/admission.
- Actual HTTP framing/integrity boundaries: absent/duplicate/conflicting length,
  short/excess body, wrong hash, encoding, redirects, exact caps and cap+1.
- Lost transfer, busy, restart, token rotation and changed descriptors retain
  the same remote job, with no partial child or duplicate summary. Verify
  cancellation and no network under a write transaction.
- Focused Rust tests, formatting and clippy; CI owns broad duplicated Linux and
  native Windows suites. Build frontend assets required by the Rust target.
- Follow the provider's frozen fresh-corpus outcomes through the production
  consumer, keeping failed attempts in the denominator. Report input admission,
  retrieval, child processing and summary quality separately. Reuse unchanged
  source-pinned OCR evidence; do not silently replace its two failed attempts.

## Implementation summary

Implemented at `79201fccab4e8d81377d1c08f9b4e8a5d309a477`, following
contract-only commits `ff78091`, `702d099`, and `99f26a5`.

- Discovery prefers v3/profile 1.1 for new work. Saved requests retain their
  protocol and provider instance. Existing v2 inline handling remains available.
- Completed descriptors are retained before retrieval. Sequential private
  downloads validate framing, length, digest, canonical pairing and provenance
  before the existing atomic output store and child admission.
- Cancellation interrupts stalled reads. Three-attempt transient retries restart
  at byte zero. Bad framing, malformed status and integrity failures become a
  visible failed consumer handoff; provider completion is not rewritten.
- Schema 22 changes only the saved-profile PDF cap, preserving v2's 2 MiB cap,
  paired data, foreign keys and immutable lineage. No installed user database
  was opened or migrated by this work.
- The HTTP parser is the existing hyper dependency, now named directly for its
  typed parse-error classifier. Cargo.lock adds only that dependency edge.

## Verification and effect trace

At the implementation commit:

- `cargo test --locked --lib connect:: -- --test-threads=2`: 83 passed,
  4 ignored, 14.50s. Includes both-direction framing/cap probes, malformed
  metadata, cancellation, total deadline under continuing progress, lost
  transfer, busy exhaustion, token rotation, restart and concurrent retrieval.
- Schema migration checks: 14 passed, including populated v21 preservation,
  rollback/FK restoration and existing older migrations. Profile-specific
  storage boundaries are also exercised in the consumer tests.
- Pinned v3 fixtures: 38 status/error cases passed at the canonical revision.
  Entitlement fixtures also passed. The separate v2 conformance check FAILED
  at v2.rs:716 because its older pinned manifest omits OCR media. It fails in
  the same place on unchanged main `6941c60ac94c000942bd6956604a82a98b51caf7`.
  This PR does not change that manifest or pin; the operator placed those
  cross-app corrections after the streamed-retrieval and model-preset work.
- A test-only follow-up transfers the exact 36 MiB PDF and 256 KiB text caps
  through actual HTTP (passed). Production code remains identical to the
  corpus-tested implementation.
- Formatting and all-targets/all-features clippy passed. Frontend assets built
  successfully before Rust validation. Broad Linux and native Windows suites
  remain CI gates, not locally claimed results.
- Fail-first evidence: v3 profile initially rejected; large paired storage
  initially failed the old SQLite cap; stalled cancellation took 3.023752393s;
  malformed status left the consumer running; conflicting Content-Length was
  transient. The corresponding final regressions pass.

Boundary-probe: valid and invalid HTTP framing, identity, descriptors and paired
bytes; exact profile caps and cap+1; mixed pair corruption; unknown/duplicate
metadata; restart and cancellation. A corrupt second output stores neither
sibling and admits no child. Concurrent downloads produce one child/lineage;
a second database connection can acquire a write transaction during network I/O.

Effect-trace: a previously oversized OCR PDF reaches an OcrText child | v3
selection, verified download, SQLite profile cap and existing admission own the
result | actual HTTP coordinator test above 2 MiB plus the frozen corpus proof.

## Frozen real-corpus proof

The final replay used consumer `79201fccab4e8d81377d1c08f9b4e8a5d309a477`
and production provider `3f101d74b7d9dce2aaf9defb4f9326009da30941`, with source
hashes unchanged for the entire run. It replayed the earlier source-pinned OCR
outcomes through the real provider store, registration, authenticated HTTP
routes and production consumer. It used fixture entitlement and an isolated
state directory. OCR recognition was replaced only by the exact hash-verified
recorded pair or the original recorded failure.

All 15 source documents took the native scan-to-OCR route. The 13 successful
pairs were retrieved and persisted byte-identically, then parsed, normalized,
structured and chunked as OcrText children. Their PDFs ranged from 2,165,528 to
7,360,291 bytes. Reopening after moving the original input reused each child
and retained its single lineage. The two original OCR_ENGINE_FAILED outcomes
remain in the denominator and admit no child.

This is delivery/child-processing proof using frozen OCR outcomes, not new
OCR recognition, new model inference, summary quality, installed GUI operation
or model qualification. The proof stops before model analysis. Private source
paths, content, requests, output hashes and receipts are retained locally and
are not published in this PR.

## Cold diff audit

| File and line | Actual change and contract trace | Verification |
|---|---|---|
| connect/ocr_consumer.rs:349, 428, 844, 1027, 1111 | Protocol-aware request/recovery, terminal failure, shared pair validation, retained output validation, discovery | Legacy tests, new HTTP/restart/concurrency tests, corpus |
| connect/ocr_consumer/streamed.rs:34, 185, 315, 443, 520, 557 | Strict bounded metadata, paired descriptors, saved completion, retrieval, typed framing failure, interruptible I/O | 38 canonical fixtures and boundary/integrity/recovery probes |
| connect/ocr_consumer/streamed/tests.rs:159, 403, 791, 1032 | Large pair, old database, opt-in real corpus, simultaneous recovery | Focused tests and final corpus run |
| connect/v2.rs:98, 116 | Shared envelope generic over output type with v2 default; no wire/schema/manifest change | Existing Connect tests; old conformance failure reproduced on main |
| pipeline/schema.rs:1152, 2876 | Transactional v22 table rebuild and rollback proof | Schema suite and populated v21 consumer test |
| pipeline/db.rs:1 | Test-only export of old-schema fixture owner | Migration tests |
| Cargo.toml:47, Cargo.lock:1083 | Direct edge to existing hyper for typed errors, no version churn | Framing regression and clippy |
| docs/PR-OCR-STREAMED-RETRIEVAL.md | Contract, code-driven revisions, evidence, boundaries | Diff checked against the contract |

All Rust paths above are relative to src-tauri/src, except Cargo files which
are relative to src-tauri. No model settings, summary prompts, scoring, OCR
engine, native-input limit or Invoice Processor implementation changed.

## Gap audit

DONE: implementation and declared local retrieval/child-processing proof.

NOT DONE for merge: current-head CI and independent review. The existing v2
conformance mismatch remains disclosed, not fixed or waived silently. Summary
quality and installed operation are outside this proof. Invoice Processor is
next in the same streamed-retrieval lane; model setup and other work stay queued.

## Contract revision: persisted output cap

The real large-pair regression reached paired storage and failed SQLite's
`ocr_pdf_byte_size <= 2097152` CHECK in schema.rs. The first contract incorrectly
excluded a database migration. Version 22 must rebuild only ocr_handoffs in one
transaction, preserving every row, index, foreign-key relationship and lineage
trigger. Its PDF CHECK selects 36 MiB only for a saved protocol 3/profile 1.1
request; all other rows retain the 2 MiB bound. The application still validates
the saved request, pair and actual bytes. No new columns or phases are needed.

Use the existing migration owner, restore foreign-key enforcement on every
outcome, and check foreign-key integrity before commit. Test migration from a
populated version-21 database, unchanged v2 records/lineage, idempotent reopen,
rollback on invalid input, and both profile boundaries. Only temporary test
databases are exercised here; do not open or migrate installed user data.

## Contract revision: typed HTTP framing refusal

The conflicting-Content-Length regression failed: reqwest rejects the headers
before exposing a Response, and the generic send-error mapping called that a
transient transport loss. ADR-0010 requires bad framing to fail retrieval.
Declare the already-locked hyper 1 dependency directly (no version update or
new package) to inspect its typed `is_parse()` error through reqwest's cause
chain. Parse failures become Invalid; timeouts and connection loss retain
Uncertain. Do not match error prose or replace the HTTP stack. The existing
framing matrix and interrupted-transfer recovery test cover both directions.

## Review follow-up at 9bac68c
The reviewer identified two missing regression cases, with no production-code
change required. Added an input-aliased output status through the actual HTTP
coordinator (terminal failure before any output GET or child), and a 200
application/json body in the framing matrix. Removing the two guards makes
exactly these tests fail; restoring them yields 18 streamed tests passed,
2 intentionally ignored, in 4.98s. Only tests and this evidence note changed.
