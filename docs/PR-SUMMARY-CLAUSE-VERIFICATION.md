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
   Report the existing planner limits at both 8,192 and 32,768 tokens: each
   currently offered source and the saved generated claim groups. Report
   overflow/fallback counts, without treating a character estimate as a runtime
   tokenization measurement or those saved claims as fresh F3 output.
5. Run a bounded public-case 9B verification probe with the previously used
   isolated candidate runtime/settings and inference lock. Main has no 9B preset;
   export the F3 commit and record the two runtime-overlay files separately. Keep failed attempts in the evidence.
   Fresh whole-document A/B accuracy review follows F3 and reports the five
   classes separately from coherent rate and page coverage.
6. Run focused and adjacent summary tests, formatting, strict Clippy and a cold
   diff audit. CI owns the duplicated platform suites.

## Implementation summary

Contract commit `19bfd34` precedes implementation `af23868`. One General coherent
verification builder derives governing context from F2's retained source ranges
and clause owner. Both preflight and execution call it. The existing request-ID
serializer uses F2's context interning helper after batching. Context identical
to the exact quote needs no table entry; that rule now belongs to the shared
clause owner. Non-context requests retain their previous serialized bytes.
Verification 11 records the new policy; historical verification 10 remains valid.
No verdict schema, storage, model/runtime setting or rewrite path changed.

The admission regression failed before the fix because its captured request
lacked the governing clause, and passed afterward. Four focused tests pass:
execution/admission equality and withholding for all five fixture verdict pairs;
full cross-page context with the adjacent clause excluded; request-local context
deduplication/isolation; and exact budget and verdict-ID boundaries. These
fixture verdicts prove application enforcement, not model accuracy.

Adjacent summary suite: 278 passed, 15 opt-in ignored. Saved A/B historical
citation artifacts validate. Their 26/42 currently offered sources map exactly
to drafting's context ownership; 20/28 have additional context. Replayed
per-source verification requests peak at 8,437/6,284 characters with no oversized
source. These are per-source admission measurements, not a claim that arbitrary
multi-source paragraphs or fresh summaries fit. Formatting, strict all-target/
all-feature Clippy and diff checks pass. The public live fidelity probe failed; details below.

## Offline fit at both supported context sizes

The extended replay uses the production character-budget planner, without a
model call. It projects the saved prose claims through the new source-context
builder; these are not freshly generated F3 summaries.

| Saved case | Context | Planner character cap | Saved-claim batches | Largest request | Oversized offered sources | Planner fallback |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| A | 8,192 | 10,752 | 2 | 9,715 | 0/26 | No |
| A | 32,768 | 16,000 | 2 | 14,150 | 0/26 | No |
| B | 8,192 | 10,752 | 5 | 8,305 | 0/42 | No |
| B | 32,768 | 16,000 | 3 | 13,152 | 0/42 | No |

Both historical artifacts still validate. The 8k and 32k planner checks pass;
actual runtime tokenization can still reject a request, and these results do
not imply every possible future paragraph fits. The new measurement changes
only the opt-in replay test. Strict Clippy and formatting pass after that change.

## Live setup provenance

The initial probe stopped before inference: main selected an 8,192-token profile,
not the required 32,768-token 9B profile. Registering a GGUF alone does not select
that model. No server started, no model verdict was produced, and this failed
setup remains in the local evidence.

The corrected probe exports implementation `af23868` with the harness's existing
`snapshot_source` helper. Only `model_settings.rs` and `llama_cpp.rs` are overlaid
from the checksum-verified prior `reliability-20260930/matrix` snapshot. These
supply its existing 9B Q4_K_M, 32k, closed-think-block candidate configuration;
they are not changes in this PR or evidence of an installed production preset.
The production verifier and its requests come from F3. The isolated build and
four focused regressions pass; the candidate framing regression also passes.
Model/server hashes must match prior qualification. The driver holds the shared
inference lock, checks an idle GPU, records owned server arguments, preserves all
attempts, and compares source hashes before and after execution.

The F3 live probe completed all ten public cases: 7/10 expectations met. Both raw
verdicts and post-guard results accepted the faulty payment-stage, invoice-period,
and working-day/cure claims. It rejected the faulty coverage-condition and
equipment-scope claims and accepted all five faithful controls. Every captured
request contains the governing clause and its lost condition; there was no
truncation. Actual server context was 32,768, GPU ownership stayed exclusive,
and frozen source hashes remained unchanged. The failed assertion is retained.
A second bounded, evidence-only run placed the full clause directly in each
`exact_quote` and removed context-table lookup. The system prompt, seed, output
schema/limit, model and runtime settings stayed the same. All ten raw and final
verdicts were identical: 2/5 faulty claims rejected and 0/5 faithful paraphrases
wrongly withheld. Removing lookup therefore did not correct these failures in
this probe. The diagnostic modification is not in the production diff. These
small synthetic probes establish a remaining verifier-accuracy failure; they
do not establish general model accuracy or a fresh whole-document result.

## Cold diff audit

- `summary.rs`: shared prompt builder at preflight and execution, context-aware
  verifier instruction, version selection/replay compatibility, and optional
  internal context field. Existing strict response parsing and guards remain.
- `summary/coherent/verification.rs`: context from the cited block/quote through
  the existing source owner; only current General coherent input is extended.
- `summary/identifiers.rs`: existing local claim/evidence vocabularies preserved;
  distinct context table emitted per batch. Context IDs cannot restore as verdicts.
- `summary/coherent.rs` and `whole_clauses.rs`: expose the shared context interner
  and own the existing no-redundant-context rule in one place. Drafting semantics
  are preserved; a production-admission regression and test fixture access added.
- `summary/coherent/verification/tests.rs`: synthetic execution and boundary
  tests, owner-only offline replay, and opt-in live verifier probe using the
  isolated 9B candidate runtime.
  Every probe attempt is retained; semantic expectations are separate from output.
- `summary/legacy_generation.rs`: test-only constructor initializes no context.
- `docs/CONTRACTS.md` and this file: versioned input contract and honest evidence.

boundary-probe: original verdict IDs accepted and context IDs rejected; correct
and rejected fixture verdicts preserved; repeated context shared and different
conditions separate; no-context and historical requests isolated; complete input
passes at its character limit and fails one character below it.

effect-trace: governing clause reaches verification | shared source owner and
request builder | fail-first captured-request regression and identical admission/
execution requests pass; the live probe still passes three incorrect claims.

## Gap audit

NOT DONE for merge: the public fidelity gate fails for three faulty claims.
Current-head CI/review remain. Context transport and deterministic enforcement
are proven, but do not resolve this model-judgment failure. No merge is justified
by green deterministic tests alone.
Fresh full-document A/B accuracy review follows F3; #111 remains held.
