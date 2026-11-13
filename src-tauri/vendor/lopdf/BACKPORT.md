# lopdf 0.42.0 backport

The source comes from the [published lopdf 0.42.0 crate](https://static.crates.io/crates/lopdf/lopdf-0.42.0.crate).
The archive SHA-256 is `25aab26d99567469098e64a02f42679f8965c6401263eefa31d8f2dcc37a221c`.
The upstream MIT license is in [LICENSE](LICENSE).

`Document::load_mem_checked_encryption` adds an opt-in empty-password load.
It reports typed authentication, object decryption, and required stream
decoding failures before returning the document. The regular load entry
points still use the upstream recovery path.

- For encryption version 4 and later, resolves the declared crypt filters
  for strings, streams, and embedded files. It uses Identity when the string
  or stream filter is omitted and applies an explicit first-stage `/Crypt`
  filter before decoding the remaining stream filters.
- For Standard security revisions 2–4, derives the file key from the user
  password recovered when the empty credential authenticates as the owner.
- Validates all four permission bytes in the AES-256 `/Perms` entry after
  empty-password authentication.
- Checks the declared stream length against the stream boundary and decodes
  object streams and required page content before returning the document.
  The checked decoder supports Flate, LZW, and ASCII85 and reports failures
  to the caller.

The isolated source delta is recorded as `lopdf-reader-backport.patch` in the
issue #133 evidence packet. Its SHA-256 is
`ea395bb751216c873b01fb50562b201d415fb4d262536bc01597da6ea682fc79`.
The packet also contains the reader-only test and its synthetic fixtures.

Retire the Cargo override when a compatible upstream release passes the
encrypted page and object-stream regressions and the Doc Sum admission tests.
