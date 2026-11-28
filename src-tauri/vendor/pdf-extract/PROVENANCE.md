# Pinned pdf-extract fork

Base: pdf-extract 0.12.0, upstream `jrmuizel/pdf-extract` commit
`b95bf9f6268772d5088f09b0034e488e64294835` (package VCS metadata).
Registry archive SHA256:
`417e8fdc940f1d5bc62c5f89864c3a2255f74f69aa353c98509213d67df61e73`.
All 12 archive files matched the locally extracted cache before staging.
The archive's Cargo.lock and VCS/cache metadata are excluded from this fork.
Application Cargo.lock owns dependency versions. No transitive upgrade is intended.
Pristine manifest alias `c9-native-layout-parity-20261027/vendor-pristine-manifest.json`,
SHA256 `70f8e50e31944542f616043633f2d185c173c2cf7ec528b96f70dfe161160c35`.

## Notices

Package Cargo.toml declares `license = "MIT"` and author
`Jeff Muizelaar <jmuizelaar@mozilla.com>`. The pinned upstream tree contains no
licence file; the licence endpoint at that ref returned HTTP 404. Evidence alias
`c9-native-layout-parity-20261027/licence-stop-r4.json`, SHA256
`7b825d22a2daf839658023820c6fc743b699a08374da6365c6777cc95040f9bc`.
Oversight additionally reported no licence file/default-branch licence metadata.

LICENSE-MIT is a **metadata-derived notice**, not a recovered upstream file.
The operator explicitly approved this notice on 2026-10-28. It uses canonical
MIT text and the package author's name plus contributors; no year is asserted.
The reference-derived AFM data retains its separate verbatim Adobe notice.

## Maintenance scope

This fork is retained source maintenance, not a library upgrade. The accepted
decoder geometry amendment requires an opt-in enriched glyph path while keeping
legacy OutputDev and PlainTextOutput behavior unchanged. Application adoption
waits for public geometry/compatibility proof and frozen-source parity.
Future dependency updates must explicitly reconcile this delta and these gates.

Reference-derived geometry formulas and descriptors are ported from pinned PDFMiner.six 20260107. NOTICE-PDFMINER retains its installed MIT notice verbatim, SHA256 f01e5d699a33766b5caf4a5099f2856aba4f3f75ea1df05dc0572b34b4bea8f2. AFM source SHA256 cd9b9342117ab5a9217f19eb993b5d32ac4e0d686ca04bb3fc47d1289cbddd47; verbatim Adobe notice SHA256 beee33e00ca4108f85e00aafccf8f6ed298d813d734791ea6e928c1fe2eb4622. Generated reference_metrics.rs retains canonical metrics, Unicode width keys and the reference aliases; legacy core_fonts.rs is unchanged.


## Opt-in Form invocation repair (r6)

The geometry path ports Form CTM composition, child state and raw Resources
selection from the same pinned PDFMiner `pdfinterp.py` do_Do/init_state owner.
Image bytes and unsupported subtypes are not interpreted as text operators.
An explicit invocation stack replaces geometry recursion; active canonical
stream identities reject cycles, while repeated sequential Forms are preserved.
Unlike the reference's cycle skip/permissive malformed handling, the accepted
contract requires geometry errors without a successful partial page for cyclic
or invalid required invocation data. Legacy callers retain their original path.
Public fixtures and native error-policy controls live in tests/forms; the latter
are explicitly marked as contract errors, not reference-parity results.
The accepted proposal is pinned in PR-NATIVE-PDF-FORM-INVOCATION-AMENDMENT.md.

## Formatting packaging policy

The local `rustfmt.toml` sets `disable_all_formatting = true` for this retained
crate only. This packaging delta preserves upstream source/table formatting
under the application's unchanged `cargo fmt --all -- --check` CI gate. It
also exempts local edits within this fork; those require source-delta review.
Application Rust remains checked. `scripts/test_vendor_formatting.py` proves
both sides using these actual configurations and the same Cargo command.
This configuration file is local packaging, not an upstream archive file.

The r6 dispatch review repair consolidates raw dictionary presence and numeric
array validation at the invocation owner. See the canonical amendment's
“Review repair: raw Form dictionary semantics” section for the reference binding,
null-Matrix classification correction and regression evidence.
