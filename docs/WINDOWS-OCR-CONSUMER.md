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
