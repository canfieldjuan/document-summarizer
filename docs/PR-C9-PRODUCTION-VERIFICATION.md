# C9 production integration

## Contract

### Root cause

At base `e9b923df9ae216ac4c1eb29dcdbe7b229a5ca17d`, General coherent
verification transports governing clauses but `summary.rs::verification_request`
still requests a bare model verdict. `parse_verification_response` checks shape
and identities, not whether governing relationships were compared. The retained
F3 live results accept the wrong payment stage, dropped invoice condition, and
weakened working-day/cure condition. Blame attributes the bare verdict schema to
the original verifier, and verification 11 to this arc's F3 commit `af238682`.
C9's experimental request constrains exact source/claim passages and requires
stage, conditions, qualifiers and scope comparisons. Its approved controls have
passed, but the app does not consume this protocol yet.

### Required change surface

- One Rust comparison module owns bounded passage catalogs, the response schema,
  strict decoding and derived verdicts. Port the frozen C9 semantics. Reject
  changed/omitted relationships; withhold uncertain relationships. The model
  still judges relations; exact passages alone do not prove entailment.
- Current General coherent prose uses one claim per request, with complete
  source-owned contexts and request-local identities. Reuse a single plan in
  runtime admission and execution. Preserve cancellation and bounded request
  counts. Reject oversized inputs without shortening clauses; admission uses
  the existing disclosed claim-ledger fallback.
- Version new comparison verification separately. Keep completed historical
  artifacts readable and retain existing storage and citation shapes.
- Share existing entailment/context instructions; remove the bare-verdict
  output instruction from the comparison path instead of stacking instructions.
- Add production regressions, frozen public request/response parity fixtures,
  malformed/foreign-span/identity/budget boundary probes, multi-claim isolation,
  cancellation, saved-result compatibility, and admission/execution parity.
- Update the canonical verification contract in `docs/CONTRACTS.md`; other
  documentation links there for the current rule.

### Explicit non-scope

No model/preset/default changes, new thinking experiments, drafting changes,
Contract extraction changes, Story or Connect direct-ledger behavior changes,
OCR changes, UI redesign, public API changes, database migration, dependency
updates, new retries, keyword exceptions or tuning on unseen documents. Frozen
experiment code, original labels and scores remain unchanged. Existing semantic
guards remain a separate last defense; none is evidence of general fidelity.

### Assumptions/blockers

This work continues the held PR116 branch. Its fidelity hold and PR100's hold
remain. Four operator-approved controls justify integration, not release.
Source without a reconstructed larger clause uses its original exact quotation
as context, matching the existing source ownership boundary. Inputs outside C9's
fixed bounds are unsupported by this protocol and use the existing admission
fallback. No invented independent labels or unseen-batch tuning is allowed.

### Verification plan

1. Fail first: the production captured request must require comparisons and
   forbid a model-supplied verdict. Existing code should fail that assertion.
2. Replay approved C9 controls and retained public failed-case responses through
   the Rust planner/parser. Match schema, instructions, exact catalogs and
   derived verdicts; compare genuine raw model output, not invented judgments.
3. Exercise invalid and valid sides: unknown/duplicate/missing keys and IDs,
   partial/mixed comparisons, wrong-side/non-token spans, empty input, Unicode,
   relation shape, catalog/input/schema limits and complete-source fallback.
4. Exercise production callers for multi-claim isolation, cancellation,
   admission/execution identity, historical saved results and current reopen.
5. Run focused regressions and adjacent summary tests, formatting, strict
   Clippy and a cold diff audit. CI owns duplicated broad/platform suites.
6. Before release: actual production-runtime public controls, full-document app
   proof and locked unseen qualification. Retained Python results alone cannot
   clear that gate. Keep every attempt outside worktrees with owner-only access.

## Implementation summary

Pending implementation.

## Cold diff audit

Pending implementation and verification.

## Gap audit

NOT DONE. Integration, production verification and qualification remain.
