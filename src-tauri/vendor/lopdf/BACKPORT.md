# lopdf 0.42.0 backport

The source comes from the [published lopdf 0.42.0 crate](https://static.crates.io/crates/lopdf/lopdf-0.42.0.crate).
The archive SHA-256 is `25aab26d99567469098e64a02f42679f8965c6401263eefa31d8f2dcc37a221c`.
The upstream MIT license is in [LICENSE](LICENSE).

`Document::load_mem_checked_encryption` adds an opt-in empty-password load.
It reports typed authentication, object decryption, and required stream
decoding failures before returning the document. The regular load entry
points still use the upstream recovery path.

The isolated source delta is recorded as `lopdf-reader-backport.patch` in the
issue #133 evidence packet. Its SHA-256 is
`ea395bb751216c873b01fb50562b201d415fb4d262536bc01597da6ea682fc79`.
The packet also contains the reader-only test and its synthetic fixtures.

Retire the Cargo override when a compatible upstream release passes the
encrypted page and object-stream regressions and the Doc Sum admission tests.
