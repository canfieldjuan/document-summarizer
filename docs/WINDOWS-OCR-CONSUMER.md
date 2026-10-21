# Windows scan consumption through the real OCR provider

At `939f89f40c2b74a43cb21dfc08bb7cdabaefc2ea`, direct OCR discovery and
materialization return an explicit platform refusal on Windows. This prevents
the existing desktop `process_scanned_document` path from consuming an OCR
provider that is already running. Fix these two boundaries at their owner.

Use existing Windows storage admission for current-user LocalAppData
`LocalConnect/runtime/v2` and `v3` registrations, bounded no-reparse private
reads, and atomic private derived-PDF writes. Share existing registration and
authenticated-manifest validation and the existing real HTTP transport, durable
handoff, output validation and child admission. Preserve Unix behavior, v3
preference, provider identity, exact request lineage and retry/capacity policy.
Unsafe objects remain refused. Do not introduce a second OCR protocol client.

Native regression: start Document OCR's real foreground CLI from
`38ad9ad6552e74a974f846f3230945fa3bbaf600`, pinned Paddle models and original
CORD fixture. Invoke the production scanned-document handoff on Windows with
no mock transport or OCR processor; require a derived child, exact qualified
OCR PDF/text hashes and restart reuse of the same handoff/child. Reproduce the
current platform refusal first. Commit the test and retain executable/source
hashes and raw output. This is Juan's second step, not a financial-extraction,
packaged-provider, summary-model or background proof. No model/prompt changes.

The provider's current private CLI PR remains unmerged pending review, including
its unexplained initial Linux test stall; this proof does not waive that gate.

## Evidence-led correction

The first native discovery implementation rejected the real publisher at the
file owner: the strict reader requires a protected file DACL, while the actual
publisher inherits a private DACL from its protected parent. ADR-0005 requires
protected new directories and effective private file ACLs, not protected file
DACLs. Add an explicit inherited-private bounded-read entrypoint to the existing
Windows storage owner; reuse its no-reparse opened-handle and ACL validation.
Keep existing strict readers unchanged. The consumer uses this entrypoint and
counts all directory children before probing, admitting at most 256 per protocol
and only JSON candidates. Regression covers private inherited files, unsafe
ACLs/reparse ancestry, exact byte and traversal bounds, and the real publisher.

## Native result and reproduction

Both original platform refusals were reproduced in the actual Windows handoff:
first discovery, then recovery after real Paddle output retrieval. The final
foreground test passed using the unchanged qualified scan and model runtime.
Both protocols answered unauthenticated requests with 401 and authenticated
manifest requests with 200. The production consumer selected v3, retained both
outputs, admitted one child, then restored a removed derived PDF after database
reopen without changing the provider job or child.

The ignored native test requires `DOCSUM_NATIVE_OCR_SCAN` and
`DOCSUM_NATIVE_OCR_BASELINE` pointing at Document OCR's committed
`tests/fixtures/cord-v2/validation-000.pdf` and `serving-baseline.json`, plus the
real foreground provider with pinned Paddle models. Run the built native test:

```powershell
.\consumer.exe --ignored --exact connect::ocr_consumer::tests::windows_real_foreground_provider_is_consumed_and_recovered --nocapture
.\consumer.exe connect::ocr_consumer:: --nocapture
.\consumer.exe connect::windows_storage:: --nocapture
```

Result: real flow 1 passed; native consumer 11 passed, 1 ignored; native storage
5 passed. Retained PDF: 57497 bytes, SHA-256
`d6c9a491657ecd98d84d1832398d16e7215762003747d7ad7475b10c7db6a74f`.
Retained text: 98 bytes, SHA-256
`ef74f8e3d4429d9c1cda7a7a718539ff48776bff41e5488607f422a2d93877ef`.
Linux consumer: 39 passed, 2 existing ignored integration tests.

The GNU cross-built test needs the existing Tauri application manifest linked
into its executable: compile Cargo's generated `resource.rc` with
`x86_64-w64-mingw32-windres --output-format=coff`, then add that object through
`RUSTFLAGS=-C link-arg=<resource.o>` when building the test. Without the manifest,
Windows selected Common Controls 5 and lacked `TaskDialogIndirect`; native
before/after import probes confirmed that loader cause. No product dependency
or runtime behavior was changed to accommodate the test build.

Cold diff: the common HTTP/profile checks were extracted unchanged; Windows
filesystem admission and materialization use the existing storage owner. The
native tests exercise the production handoff, not an injected OCR processor.
No model, transport schema, capacity, prompt, invoice, packaging or background
changes. Functional foreground proof is DONE; packaging and independent review,
CI and merge remain NOT DONE. Durable evidence alias:
`windows-ocr-consumer-evidence-v1.zip` (digest posted with the PR).
