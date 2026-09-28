# Coherent synthesis uses its effective context

## Root cause

The coherent synthesis producer and runtime validator use the analysis-stage
character helper, whose fixed 16,000-character cap survives a larger runtime
context. Saved production requests on real contracts A and B reproduce the
result: their complete retained catalogs fit 32,768 tokens with output and
framing reserves, but the application selects a smaller source catalog before
the exact runtime admission check. This is an admission defect, not evidence
that larger input alone improves summary quality. See issue #102.

## Required change surface

- `summary/coherent.rs`: one coherent input-budget owner, used by synthesis and
  runtime validation. At or below 8,192 context tokens preserve the existing
  conservative cap. Above that context use checked arithmetic for
  `(context - 2,048 output - 512 framing reserve) * 3` characters, including
  system prompt, user prompt and serialized response schema.
- Pass this same input limit through source reduction's stop condition and the
  existing bounded repair loop. Every admitted generation still passes exact
  runtime token preflight. Invalid budgets and non-context admission errors
  continue to fail; overflow still selects/reduces or falls back as before.
- `summary.rs` and coherent tests: reproduce a retained catalog unnecessarily
  reduced at 32k, prove full-source generation and runtime validation, and retain
  8k reduction, exact-token rejection, reserve and downstream verification gates.
- Amend the canonical synthesis budget paragraph in `docs/CONTRACTS.md`.

## Explicit non-scope

No preset/context default promotion, automatic profile routing, output-token
increase, source segmentation/omission change, prompt/schema wording change,
selection-window size change, analysis or verifier budget change, dependency,
public API, storage, artifact-format/version, UI or Connect change. Contract
incomplete-catalog admission remains separate draft #92. Completed artifacts
keep the same content validation; no stored catalog identity or text changes.

## Assumptions and blockers

The effective synthesis context comes from the existing runtime profile, not
ambient settings. The larger character allowance is a conservative proxy, not
token proof. Exact preflight is authoritative. Selection requests retain their
existing small window and budget. The private 32k test profile is experimental;
live evidence may still show fallback, poor coverage or model failure.

## Verification plan

1. Fail first through production synthesis with a synthetic retained catalog
   exceeding 16,000 characters that fits 32k: expect the old code to select
   sources, violating the no-selection assertion. Then prove full generation,
   validation and unchanged lower-context selection.
2. Probe output/reserve exhaustion, the exact character boundary, schema
   accounting, actual runtime context rejection, and unchanged analysis and
   verification limits. Reuse existing repair, grounding and coverage regressions.
3. Run affected summary tests, formatting and strict all-target/all-feature
   clippy. CI owns the duplicated full platform suites.
4. Reuse the saved real-contract diagnostic and pinned tokenizer to check both
   retained catalogs with the new budget. Run General A/B through the production
   pipeline at experimental 9B/32k with the existing inference lock, owned-server
   checks, private receipts and unchanged acceptance gates. Retain failures and
   report source admission separately from supported prose and final delivery.
5. Compare the final diff to this contract and request exact-head review.

## Implementation summary

Pending implementation.

## Cold diff audit

Pending implementation and verification.

## Gap audit

NOT DONE: implementation, fail-first proof, verification and review remain.
