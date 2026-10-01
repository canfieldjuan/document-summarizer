# F3: verify prose against its governing clause

## Contract

### Root cause

F2 reconstructs whole numbered clauses for General drafting, but `verify` and
`coherent_verification_exceeds_runtime_context` still build requests containing
only `EvidenceItem.exact_quote`. A continuation can lose the governing payment
stage. The saved A/B review also found dropped qualifications even when they
were in the excerpt; context transport alone does not prove semantic accuracy.

### Required change surface

- Reuse F2's clause owner over normalized source and retained furniture ranges.
  Bind each cited block/quote to its original governing context. Do not accept
  context supplied by the model or an unrelated clause.
- One shared General coherent verification prompt builder must serve preflight
  and execution. Include distinct governing contexts once per request, with
  evidence references. Exact quotes and durable citation identities stay intact;
  context IDs cannot enter verdict/citation vocabularies.
- Tell verification to consider the governing stage, conditions, qualifications
  and scope. An omitted material qualification is insufficient support. Existing
  strict verdict parsing, withholding, semantic guards and source-contribution
  checks remain in force. There is no rewriting or repair generation.
- Count the complete serialized context in existing verification admission and
  batching. Overflow follows the existing disclosed fallback; never truncate a
  clause to fit. New verification is version 11; completed version-10 artifacts
  remain readable under their original contract.

### Explicit non-scope

No model/runtime/settings change, new retry, OCR change, UI/Connect wire change,
schema migration, citation widening, new inference infrastructure or claim that
a mocked verdict proves natural-language accuracy. Contract/Story and historical
source-selection policies remain unchanged. #111 remains held; #100 is frozen.

### Assumptions/blockers

Numbered clauses use the F2 reconstruction boundary; unnumbered or ambiguous
source keeps exact-quote verification. Context is source data, never instruction.
The existing model remains the semantic judge. A failing live fidelity probe is
a remaining blocker, not a reason to weaken validation or add keyword patches.

### Verification plan

1. Fail first through production verification admission: a continuation's
   governing payment-stage opening must reach the captured request exactly once.
2. Cover execution/preflight identity, cross-page/parent context, unrelated
   clauses, no-context/historical paths, distinct versus repeated contexts,
   request isolation, strict verdict IDs, mixed verdicts and over-budget input.
3. Commit public synthetic cases for the five findings: payment-stage transfer,
   lost invoice-period qualification, working-day/cure conditions, insurance
   coverage condition, and narrowed equipment-use scope. Include faithful
   paraphrases. Deterministic tests prove transport and enforcement; real model
   verdicts, not fixture labels, determine semantic success.
4. Replay saved private A/B artifacts before inference. Confirm historical
   readability, context ownership and request fit; keep private text local.
5. Run a bounded public-case 9B verification probe with the existing qualified
   runtime/settings and inference lock. Keep failed attempts in the evidence.
   Fresh whole-document A/B accuracy review follows F3 and reports the five
   classes separately from coherent rate and page coverage.
6. Run focused and adjacent summary tests, formatting, strict Clippy and a cold
   diff audit. CI owns the duplicated platform suites.

## Implementation summary

Pending.

## Cold diff audit

Pending.

## Gap audit

NOT DONE: implementation, deterministic proof, saved replay, public fidelity
probe and current-head CI/review remain.
