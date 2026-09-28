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

## Implementation summary / cold diff / gap audit

NOT DONE: contract-first stage. Implementation, regressions, production consumer
proof and exact-head PR review remain. The invoice consumer follows separately.

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
