# Restore shared Rust CI

Proposed contract for [issue #159](https://github.com/canfieldjuan/document-summarizer/issues/159).
The base is `e5000cf20fdd65d53804d17f93f6f7683e6e2aad`.

## Root cause

The [failing run](https://github.com/canfieldjuan/document-summarizer/actions/runs/43380508326)
used the floating `stable` toolchain in [.github/workflows/rust.yml](../.github/workflows/rust.yml).
Rust 1.100.0 Clippy rejects the final branch of `broader_enumeration_supported`
in [semantic_support.rs](../src-tauri/src/pipeline/summary/coherent/semantic_support.rs#L1135).
The branch introduced in `16edd57bdcab1832139ed7e4e361aa9f1f2b6b4c`
returns false for a kinship claim without a matching source and true
otherwise. Both Linux and Windows reject this branch under warnings-as-errors.
The same diagnostic reproduced on the exact base with the locked dependencies.

GitHub cancelled the macOS recovery job before execution because its
`macos-14` arm64 image was retired. GitHub lists `macos-15` as a supported
arm64 replacement. [Runner retirement notice](https://github.blog/changelog/2026-10-01-github-actions-macos-14-runner-image-retirement/)

## Required changes

- Add one root `rust-toolchain.toml` for Rust 1.100.0, the minimal profile,
  Clippy, and rustfmt. That is the release that produced the failing CI
  diagnostic. The workflow setup on each platform installs the active
  repository toolchain with `rustup toolchain install --no-self-update` and
  reports the active compiler. A checkout using rustup selects the same pin
  from the root or `src-tauri`. This makes lint changes deliberate and adds
  an initial toolchain download for contributors missing the selected release.
  Keeping the version only in CI would leave local checks free to drift.
  [Rustup toolchain selection](https://rust-lang.github.io/rustup/overrides.html#the-toolchain-file)
- Move the macOS recovery job to `macos-15`. Keep its existing recovery
  and runtime checks, source revision verification, and strict Clippy step.
- Express the final guard in its owning function as a short-circuiting
  Boolean result. A non-kinship claim returns true at that final predicate.
  A kinship claim returns the result of searching the selected evidence
  for kinship language. The earlier family-scope checks still apply.
  Warnings-as-errors will continue to enforce the lint that failed.

## Scope and dependencies

This is a shared CI repair on a branch from main. The guard rewrite preserves
its input and output contract. Dependency versions, PDF handling, parser
admission, product behavior, and deployment are outside this scope.
Hosted runners provide the native macOS and Windows evidence.

## Verification

1. Reproduce the lint before the edit with
   `cargo +1.100.0 clippy --locked --offline --no-default-features --lib -- -D warnings`.
   The baseline exited 101 on the expected `needless_bool` diagnostic.
   The [hosted run](https://github.com/canfieldjuan/document-summarizer/actions/runs/43380508326)
   supplies the original macOS runner refusal and the Linux and Windows
   lint failures.
2. Keep the existing semantic-fidelity regression that exercises broader
   family scope, mixed evidence, and comparative language. Cover both
   outcomes of the final kinship guard directly, including empty evidence
   and a matching quote after an unrelated quote. Verify the controls with
   the pinned compiler. The existing regression passed on the baseline.
3. Confirm that a fresh rustup environment reads the pin from the checkout
   root and resolves it from `src-tauri`. The baseline scratch probe
   confirmed that the proposed TOML and install command select Rust,
   Clippy, and rustfmt from the nested directory. Confirm the actual
   workflow preserves the strict commands and the platform-specific tests.
4. Run formatting, the focused semantic regressions, and the baseline
   strict Clippy command on the candidate. Require a successful workflow
   on all three runners at the exact repair PR head, then verify the
   main push workflow on the merged repair before bringing it into #158.

## Baseline evidence

- `clippy-baseline.log` SHA-256 `3ded78cb7994b2f57224c5e406b5ec421e27fc8d06f1a8e78e62dc40e3abfd7d`
- `semantic-baseline.log` SHA-256 `39d0fa05b3acae4b6767fa93b4210a9124d0b63b57114efb2c2edcdbc8d09b84`
- `toolchain-setup-probe.log` SHA-256 `a6f0914a9d3abe917fafa23edb5e13e86a012327fcf5e3dafc90898f2e62377f`

## Review state

This commit records the contract and baseline only. The implementation
follows independent review and operator acceptance of this revision.
