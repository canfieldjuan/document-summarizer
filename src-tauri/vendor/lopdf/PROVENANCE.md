# Local numeric precision patch

Archive: lopdf 0.42.0, SHA256
`25aab26d99567469098e64a02f42679f8965c6401263eefa31d8f2dcc37a221c`.
The included MIT LICENSE is retained verbatim. Upstream VCS metadata records
`b68476c2a067f3b5158de60cd8ebce69f72068c8`, dirty=true. The archive checksum
identifies these sources. This fork retains the archive VCS metadata. The
application Cargo.lock pins the dependency graph. The archived
`lopdf-pristine-manifest.json` lists every file from the crate.

Local delta implements accepted r5: request-owned opt-in Real64 decimal parsing,
recursive object/content propagation, explicit numeric access/serialization,
and regression tests. Existing defaults and From<f64> retain Real(f32).
[BACKPORT.md](BACKPORT.md) records the checked encryption changes to this
same archive. Both object-stream loaders pass the document numeric mode to
the shared parser after their respective decompression step.

## Formatting packaging policy

The local `rustfmt.toml` sets `disable_all_formatting = true` for this retained
crate only. This packaging delta preserves upstream source/table formatting
under the application's unchanged `cargo fmt --all -- --check` CI gate. It
also exempts local edits within this fork; those require source-delta review.
Application Rust remains checked. `scripts/test_vendor_formatting.py` proves
both sides using these actual configurations and the same Cargo command.
The remaining rustfmt settings are retained from the upstream archive.
