# Seven-setup verifier ablation

Approved cases and labels are unchanged. The independent review is PR116 comment 5936227862, file SHA256 b290394ba9a94d3cf707e9053ba64d9105face98ec73cb0da646ddb9eed99a1b. Source-reading quality and equipment-scope narrowing-policy compliance are reported separately.

C0: production batched. C1: thinking on. C2: assessment before verdict. C3: one claim/request. C4: reversed verdict enum. C2+C3: assessment plus single claim. C5: split factual/preservation judgments.

Both models use their packaged templates. Thinking-on for 27B includes its packaged xhigh reasoning prefix; 9B does not add that prefix. The input case/order and application prompt otherwise stay fixed. C0/C1/C2/C4/C5 use groups of 8,8,8,6. C3/C2+C3 use single claims. All use seed7, temperature0, actual32768 context, and a4096 generated-token cap including C1 reasoning.

## Validated raw verdicts

| Model | Setup | Reading caught /12 | Reading false holds /12 | Policy applied /3 | Policy false holds /3 | Overall caught /15 | Overall false holds /15 | Failed claims | Candidate rule |
|---|---|---:|---:|---:|---:|---:|---:|---:|---|
| qwen35-9b | C0 | 9 | 0 | 2 | 0 | 11 | 0 | 0 | FAIL |
| qwen35-9b | C1 | 0 | 12 | 0 | 3 | 0 | 15 | 30 | FAIL |
| qwen35-9b | C2 | 10 | 1 | 2 | 0 | 12 | 1 | 0 | PASS |
| qwen35-9b | C3 | 8 | 0 | 1 | 0 | 9 | 0 | 0 | FAIL |
| qwen35-9b | C4 | 9 | 0 | 2 | 0 | 11 | 0 | 0 | FAIL |
| qwen35-9b | C2+C3 | 12 | 3 | 3 | 0 | 15 | 3 | 0 | FAIL |
| qwen35-9b | C5 | 9 | 0 | 2 | 0 | 11 | 0 | 0 | FAIL |
| qwen35-27b | C0 | 12 | 0 | 1 | 0 | 13 | 0 | 0 | PASS |
| qwen35-27b | C1 | 11 | 5 | 0 | 1 | 11 | 6 | 8 | FAIL |
| qwen35-27b | C2 | 12 | 0 | 1 | 0 | 13 | 0 | 0 | PASS |
| qwen35-27b | C3 | 11 | 0 | 1 | 0 | 12 | 0 | 0 | PASS |
| qwen35-27b | C4 | 12 | 0 | 1 | 0 | 13 | 0 | 0 | PASS |
| qwen35-27b | C2+C3 | 12 | 0 | 3 | 0 | 15 | 0 | 0 | PASS |
| qwen35-27b | C5 | 12 | 0 | 1 | 0 | 13 | 0 | 0 | PASS |

The fixed aggregate candidate rule is >=12/15 catches and <=1/15 false withholds. A malformed, truncated, missing or failed response is not a successful catch. Faithful failures count as false withholds. Policy misses are not source-reading errors. These are synthetic diagnostic results, not production qualification or evidence of an intrinsic capacity ceiling.

## Per error class (validated raw verdicts)

| Model | Setup | Class | Caught /3 | False holds /3 | Failed claims |
|---|---|---|---:|---:|---:|
| qwen35-9b | C0 | coverage-condition | 3 | 0 | 0 |
| qwen35-9b | C0 | payment-stage | 2 | 0 | 0 |
| qwen35-9b | C0 | working-day-cure | 2 | 0 | 0 |
| qwen35-9b | C0 | equipment-scope | 2 | 0 | 0 |
| qwen35-9b | C0 | invoice-period | 2 | 0 | 0 |
| qwen35-9b | C1 | coverage-condition | 0 | 3 | 6 |
| qwen35-9b | C1 | payment-stage | 0 | 3 | 6 |
| qwen35-9b | C1 | working-day-cure | 0 | 3 | 6 |
| qwen35-9b | C1 | equipment-scope | 0 | 3 | 6 |
| qwen35-9b | C1 | invoice-period | 0 | 3 | 6 |
| qwen35-9b | C2 | coverage-condition | 3 | 0 | 0 |
| qwen35-9b | C2 | payment-stage | 2 | 1 | 0 |
| qwen35-9b | C2 | working-day-cure | 3 | 0 | 0 |
| qwen35-9b | C2 | equipment-scope | 2 | 0 | 0 |
| qwen35-9b | C2 | invoice-period | 2 | 0 | 0 |
| qwen35-9b | C3 | coverage-condition | 3 | 0 | 0 |
| qwen35-9b | C3 | payment-stage | 2 | 0 | 0 |
| qwen35-9b | C3 | working-day-cure | 1 | 0 | 0 |
| qwen35-9b | C3 | equipment-scope | 1 | 0 | 0 |
| qwen35-9b | C3 | invoice-period | 2 | 0 | 0 |
| qwen35-9b | C4 | coverage-condition | 3 | 0 | 0 |
| qwen35-9b | C4 | payment-stage | 2 | 0 | 0 |
| qwen35-9b | C4 | working-day-cure | 2 | 0 | 0 |
| qwen35-9b | C4 | equipment-scope | 2 | 0 | 0 |
| qwen35-9b | C4 | invoice-period | 2 | 0 | 0 |
| qwen35-9b | C2+C3 | coverage-condition | 3 | 0 | 0 |
| qwen35-9b | C2+C3 | payment-stage | 3 | 1 | 0 |
| qwen35-9b | C2+C3 | working-day-cure | 3 | 1 | 0 |
| qwen35-9b | C2+C3 | equipment-scope | 3 | 0 | 0 |
| qwen35-9b | C2+C3 | invoice-period | 3 | 1 | 0 |
| qwen35-9b | C5 | coverage-condition | 3 | 0 | 0 |
| qwen35-9b | C5 | payment-stage | 2 | 0 | 0 |
| qwen35-9b | C5 | working-day-cure | 2 | 0 | 0 |
| qwen35-9b | C5 | equipment-scope | 2 | 0 | 0 |
| qwen35-9b | C5 | invoice-period | 2 | 0 | 0 |
| qwen35-27b | C0 | coverage-condition | 3 | 0 | 0 |
| qwen35-27b | C0 | payment-stage | 3 | 0 | 0 |
| qwen35-27b | C0 | working-day-cure | 3 | 0 | 0 |
| qwen35-27b | C0 | equipment-scope | 1 | 0 | 0 |
| qwen35-27b | C0 | invoice-period | 3 | 0 | 0 |
| qwen35-27b | C1 | coverage-condition | 3 | 1 | 1 |
| qwen35-27b | C1 | payment-stage | 2 | 1 | 2 |
| qwen35-27b | C1 | working-day-cure | 3 | 1 | 1 |
| qwen35-27b | C1 | equipment-scope | 0 | 1 | 2 |
| qwen35-27b | C1 | invoice-period | 3 | 2 | 2 |
| qwen35-27b | C2 | coverage-condition | 3 | 0 | 0 |
| qwen35-27b | C2 | payment-stage | 3 | 0 | 0 |
| qwen35-27b | C2 | working-day-cure | 3 | 0 | 0 |
| qwen35-27b | C2 | equipment-scope | 1 | 0 | 0 |
| qwen35-27b | C2 | invoice-period | 3 | 0 | 0 |
| qwen35-27b | C3 | coverage-condition | 3 | 0 | 0 |
| qwen35-27b | C3 | payment-stage | 2 | 0 | 0 |
| qwen35-27b | C3 | working-day-cure | 3 | 0 | 0 |
| qwen35-27b | C3 | equipment-scope | 1 | 0 | 0 |
| qwen35-27b | C3 | invoice-period | 3 | 0 | 0 |
| qwen35-27b | C4 | coverage-condition | 3 | 0 | 0 |
| qwen35-27b | C4 | payment-stage | 3 | 0 | 0 |
| qwen35-27b | C4 | working-day-cure | 3 | 0 | 0 |
| qwen35-27b | C4 | equipment-scope | 1 | 0 | 0 |
| qwen35-27b | C4 | invoice-period | 3 | 0 | 0 |
| qwen35-27b | C2+C3 | coverage-condition | 3 | 0 | 0 |
| qwen35-27b | C2+C3 | payment-stage | 3 | 0 | 0 |
| qwen35-27b | C2+C3 | working-day-cure | 3 | 0 | 0 |
| qwen35-27b | C2+C3 | equipment-scope | 3 | 0 | 0 |
| qwen35-27b | C2+C3 | invoice-period | 3 | 0 | 0 |
| qwen35-27b | C5 | coverage-condition | 3 | 0 | 0 |
| qwen35-27b | C5 | payment-stage | 3 | 0 | 0 |
| qwen35-27b | C5 | working-day-cure | 3 | 0 | 0 |
| qwen35-27b | C5 | equipment-scope | 1 | 0 | 0 |
| qwen35-27b | C5 | invoice-period | 3 | 0 | 0 |

## Application effects

Raw/final changes: 0. Full raw and final per-class counts are in scores.json; per-claim decisions and diagnostic fields are in per-claim.json.

## Billing-period synonym control

| Model | Setup | Raw | Final | Failure |
|---|---|---|---|---|
| qwen35-9b | C0 | supported | supported | None |
| qwen35-9b | C1 | None | None | MODEL_OUTPUT_LIMIT_REACHED |
| qwen35-9b | C2 | supported | supported | None |
| qwen35-9b | C3 | supported | supported | None |
| qwen35-9b | C4 | supported | supported | None |
| qwen35-9b | C2+C3 | supported | supported | None |
| qwen35-9b | C5 | supported | supported | None |
| qwen35-27b | C0 | supported | supported | None |
| qwen35-27b | C1 | None | None | MODEL_OUTPUT_LIMIT_REACHED |
| qwen35-27b | C2 | supported | supported | None |
| qwen35-27b | C3 | supported | supported | None |
| qwen35-27b | C4 | supported | supported | None |
| qwen35-27b | C2+C3 | supported | supported | None |
| qwen35-27b | C5 | supported | supported | None |

A valid rejection of this approved invoice-period/billing-period paraphrase is a false withholding and synonym-sensitivity finding, never a caught error. A runtime/truncation failure is counted as a false withholding for delivery but is not evidence of over-literal reading. Raw/final separation identifies model versus downstream-guard rejection.

## Cost and completion

| Model | Setup | Requests | Output tokens | Reasoning tokens | Generation wall seconds | Milliseconds/verdict (amortized) | Failed requests |
|---|---|---:|---:|---:|---:|---:|---:|
| qwen35-9b | C0 | 4 | 358 | 0 | 4.768 | 158.9 | 0 |
| qwen35-9b | C1 | 4 | 16384 | 16384 | 147.012 | 4900.4 | 4 |
| qwen35-9b | C2 | 4 | 3225 | 0 | 31.043 | 1034.8 | 0 |
| qwen35-9b | C3 | 30 | 540 | 0 | 9.968 | 332.3 | 0 |
| qwen35-9b | C4 | 4 | 358 | 0 | 4.898 | 163.3 | 0 |
| qwen35-9b | C2+C3 | 30 | 4809 | 0 | 49.952 | 1665.1 | 0 |
| qwen35-9b | C5 | 4 | 1350 | 0 | 13.723 | 457.4 | 0 |
| qwen35-27b | C0 | 4 | 478 | 0 | 16.771 | 559.0 | 0 |
| qwen35-27b | C1 | 4 | 10434 | 9948 | 269.874 | 8995.8 | 1 |
| qwen35-27b | C2 | 4 | 3435 | 0 | 92.614 | 3087.1 | 0 |
| qwen35-27b | C3 | 30 | 540 | 0 | 29.465 | 982.2 | 0 |
| qwen35-27b | C4 | 4 | 478 | 0 | 16.098 | 536.6 | 0 |
| qwen35-27b | C2+C3 | 30 | 4161 | 0 | 122.634 | 4087.8 | 0 |
| qwen35-27b | C5 | 4 | 1350 | 0 | 38.879 | 1296.0 | 0 |

Times surround the production generate call and exclude registration/startup and deterministic parser/guards. Batch costs divided by claims are amortized, not independent per-claim timings. per-request.json retains actual costs, stop/completion status and token accounting, including failures. C1 reasoning counts use returned token IDs through the closing think marker, excluding the prefilling opening marker; an unterminated reasoning prefix is explicitly marked.

## C5 split judgments

| Model | Case | Factual expected / observed | Preservation expected / observed | Raw / final verdict |
|---|---|---|---|---|
| qwen35-9b | coverage-condition-2-faithful | True / True | True / True | supported / supported |
| qwen35-9b | payment-stage-3-faithful | True / True | True / True | supported / supported |
| qwen35-9b | working-day-cure-2-faithful | True / True | True / True | supported / supported |
| qwen35-9b | equipment-scope-3-faithful | True / True | True / True | supported / supported |
| qwen35-9b | invoice-period-1-faithful | True / True | True / True | supported / supported |
| qwen35-9b | equipment-scope-3-faulty | True / False | False / False | unsupported / unsupported |
| qwen35-9b | invoice-period-2-faithful | True / True | True / True | supported / supported |
| qwen35-9b | payment-stage-1-faulty | False / True | False / True | supported / supported |
| qwen35-9b | working-day-cure-1-faithful | True / True | True / True | supported / supported |
| qwen35-9b | invoice-period-2-faulty | False / True | False / False | unsupported / unsupported |
| qwen35-9b | equipment-scope-2-faithful | True / True | True / True | supported / supported |
| qwen35-9b | coverage-condition-3-faulty | False / True | False / False | unsupported / unsupported |
| qwen35-9b | equipment-scope-2-faulty | True / True | False / True | supported / supported |
| qwen35-9b | working-day-cure-2-faulty | False / True | False / False | unsupported / unsupported |
| qwen35-9b | coverage-condition-3-faithful | True / True | True / True | supported / supported |
| qwen35-9b | equipment-scope-1-faulty | True / True | False / False | unsupported / unsupported |
| qwen35-9b | coverage-condition-1-faithful | True / True | True / True | supported / supported |
| qwen35-9b | invoice-period-1-faulty | False / True | False / True | supported / supported |
| qwen35-9b | working-day-cure-3-faulty | False / True | False / False | unsupported / unsupported |
| qwen35-9b | equipment-scope-1-faithful | True / True | True / True | supported / supported |
| qwen35-9b | coverage-condition-1-faulty | False / True | False / False | unsupported / unsupported |
| qwen35-9b | invoice-period-3-faithful | True / True | True / True | supported / supported |
| qwen35-9b | payment-stage-2-faithful | True / True | True / True | supported / supported |
| qwen35-9b | working-day-cure-3-faithful | True / True | True / True | supported / supported |
| qwen35-9b | payment-stage-2-faulty | False / False | False / False | unsupported / unsupported |
| qwen35-9b | payment-stage-1-faithful | True / True | True / True | supported / supported |
| qwen35-9b | coverage-condition-2-faulty | False / False | False / True | unsupported / unsupported |
| qwen35-9b | working-day-cure-1-faulty | False / True | False / True | supported / supported |
| qwen35-9b | payment-stage-3-faulty | False / False | False / False | unsupported / unsupported |
| qwen35-9b | invoice-period-3-faulty | False / False | False / False | unsupported / unsupported |
| qwen35-27b | coverage-condition-2-faithful | True / True | True / True | supported / supported |
| qwen35-27b | payment-stage-3-faithful | True / True | True / True | supported / supported |
| qwen35-27b | working-day-cure-2-faithful | True / True | True / True | supported / supported |
| qwen35-27b | equipment-scope-3-faithful | True / True | True / True | supported / supported |
| qwen35-27b | invoice-period-1-faithful | True / True | True / True | supported / supported |
| qwen35-27b | equipment-scope-3-faulty | True / True | False / False | ambiguous / ambiguous |
| qwen35-27b | invoice-period-2-faithful | True / True | True / True | supported / supported |
| qwen35-27b | payment-stage-1-faulty | False / False | False / False | ambiguous / ambiguous |
| qwen35-27b | working-day-cure-1-faithful | True / True | True / True | supported / supported |
| qwen35-27b | invoice-period-2-faulty | False / False | False / False | unsupported / unsupported |
| qwen35-27b | equipment-scope-2-faithful | True / True | True / True | supported / supported |
| qwen35-27b | coverage-condition-3-faulty | False / False | False / False | unsupported / unsupported |
| qwen35-27b | equipment-scope-2-faulty | True / True | False / True | supported / supported |
| qwen35-27b | working-day-cure-2-faulty | False / False | False / False | unsupported / unsupported |
| qwen35-27b | coverage-condition-3-faithful | True / True | True / True | supported / supported |
| qwen35-27b | equipment-scope-1-faulty | True / True | False / True | supported / supported |
| qwen35-27b | coverage-condition-1-faithful | True / True | True / True | supported / supported |
| qwen35-27b | invoice-period-1-faulty | False / True | False / False | ambiguous / ambiguous |
| qwen35-27b | working-day-cure-3-faulty | False / False | False / False | ambiguous / ambiguous |
| qwen35-27b | equipment-scope-1-faithful | True / True | True / True | supported / supported |
| qwen35-27b | coverage-condition-1-faulty | False / False | False / False | unsupported / unsupported |
| qwen35-27b | invoice-period-3-faithful | True / True | True / True | supported / supported |
| qwen35-27b | payment-stage-2-faithful | True / True | True / True | supported / supported |
| qwen35-27b | working-day-cure-3-faithful | True / True | True / True | supported / supported |
| qwen35-27b | payment-stage-2-faulty | False / False | False / False | unsupported / unsupported |
| qwen35-27b | payment-stage-1-faithful | True / True | True / True | supported / supported |
| qwen35-27b | coverage-condition-2-faulty | False / False | False / False | unsupported / unsupported |
| qwen35-27b | working-day-cure-1-faulty | False / False | False / False | unsupported / unsupported |
| qwen35-27b | payment-stage-3-faulty | False / False | False / False | unsupported / unsupported |
| qwen35-27b | invoice-period-3-faulty | False / False | False / False | unsupported / unsupported |

## Interpretation and next gate

The 27B assessment-plus-single-claim setup (C2+C3) is the strongest result on this frozen diagnostic: all twelve source-reading errors rejected, all three narrowing-policy cases handled, and all fifteen faithful claims accepted. Its generation calls took 122.634 seconds across thirty requests. The 27B baseline already catches all twelve reading errors; the combined setup improves policy compliance from one of three to three of three. This is an observed diagnostic result, not a production qualification.

The 9B assessment-before-verdict setup (C2) meets the predeclared aggregate nomination rule, but it still approves payment-stage-1-faulty and rejects payment-stage-1-faithful. Its other misses are invoice-period-1-faulty and equipment-scope-2-faulty. The 9B combined setup catches every faulty claim but rejects three faithful claims: payment-stage-1-faithful, invoice-period-3-faithful and working-day-cure-3-faithful. That is why it fails the candidate rule. The aggregate C2 pass does not resolve the original payment-stage failure.

No completed setup rejected the approved billing-period/invoice-period paraphrase. Its two unavailable C1 judgments belong to output-limited batches and cannot establish over-literal reading. C1 exhausted the fixed answer budget in every 9B batch and the first 27B batch; the other three 27B batches completed. Completion failures remain in every denominator.

The results show verification-design sensitivity and model differences under these fixed configurations. They do not establish an intrinsic capacity ceiling. C3 alone does not improve either model, reversing enum order leaves these results unchanged, and C5 does not improve final admission counts over C0. With C5, the 9B factual-support flag disagrees with the approved reading labels on eight of twenty-four claims, while its final verdict still withholds some of them through the preservation flag. The 27B disagrees on one reading factual-support flag and zero reading preservation flags, but misses two policy-preservation judgments. A correct final verdict therefore does not prove that both diagnostic judgments were correct.

The production parser and semantic guards changed no valid model verdict in this run. Full source/claim byte checks and the raw-response audit passed. On these fixtures, remaining semantic misses are present in the model outputs; this does not certify real PDF parsing, clause assembly, summary generation or private A/B accuracy.

Recommendation: retain the production hold and have the independent reviewer check these raw artifacts. If continuing, use a newly approved, independently labeled set to test the 27B combined candidate and its cheaper baseline, with the 9B C2 result as a comparison. Include faithful paraphrases and the original payment-stage failure as explicit gates. No additional inference or production change is authorized by this report. The current approved cases and answers remain frozen.

## Scope

No production code, model defaults, presets, installed settings or private-document summaries changed. PR116 remains held. The precommitted setups, source/binary hashes, runtime manifests, all wire requests, raw responses, token IDs and attempt diagnostics are retained. The case file was hash-checked before every request. No conditions were tuned after observing results.

## Evidence receipts and reproducibility

- Case freeze: `b400fbd913b87e6d4ff2a5ce2578b0fb1cc2802e`; SHA256 `b290394ba9a94d3cf707e9053ba64d9105face98ec73cb0da646ddb9eed99a1b`. No case, source clause or answer label changed.
- Pre-inference setup freeze: `36c4acd0e3d5962c62ea3ba41327ce12c961158d`, [all frozen setups](VERIFIER-DESIGN-ABLATION-SETUPS.json). Request manifest SHA256 `d25d37f33cc4f08b01bbc6d1879e2769862248314f1ae425a42f4f8f2efb43b7`.
- Production F3 snapshot source: `af23868259d3cf4e84caddd799ca18c3929b869a`, with the recorded test-only runtime overlay. Actual snapshot files and hashes are in `source-manifest.json`; its short diagnostic-overlay file list is not the complete changed-file inventory. Snapshot `llama_cpp.rs`, `model_settings.rs`, verification `tests.rs`, and new `verification/ablation.rs` implement the isolated experiment.
- Test binary SHA256: `3d0898984590cd3538b1489ab3592e900a3917d1ace34151e1df35449f455238`.
- llama-server SHA256: `0ca399edd758decd825a71823b04ba7ddbc8b2e10d2309d8bf623ee3c2283099`. Owned-runtime model/server/library checksum verification was retained.
- 9B Q4_K_M SHA256: `cd76ec205963b3b33350093e6904d9de16c4e666fd104e1f632d25c7f15f2a13`.
- 27B Q4_K_M SHA256: `e00082f779fa385cee8c68a3ec8833a75778cc87272240b942f74e0b8243e520`.
- Both runs exited zero with eighty recorded requests each, passed all eighty preflights, matched every frozen request, verified context and source identity, and recorded no GPU contamination. The observed compute-process list was empty after cleanup. Each model remained resident through its complete setup matrix.
- Boundary controls and focused framing, truncation, stop and token-accounting checks passed before inference. The report's denominator controls pass. Independent `audit_results.py` checks all 420 claim mappings directly against native response JSON, unchanged approved labels/full clauses, wire schemas and frozen requests. Its initial internal-ID assertion assumed a per-batch index; source inspection showed the frozen global-order index, and the corrected audit passes. This changed no cases, responses or verdicts.
- No broad production suite was rerun for this evidence-only reporting change. The measured snapshot build and focused diagnostic checks are retained in the local receipts.

Local raw evidence root:

```text
/home/juan-canfield/.codex/worktrees/verifier-design-ablation/doc_sum/.codex/verifier-ablation
```

`artifact-index.json` hashes 347 evidence files. Index SHA256: `2f11080b44259726a9fc7ca55f58b14888f0a8684ec4f30107a643075f1fab54`.

| Raw result | SHA256 |
|---|---|
| `qwen35-9b/results.json` | `b6df2a16148163ec28a3089cba387f47bb21330154a352eebc179dc32d287b01` |
| `qwen35-27b/results.json` | `fe49f0c7e293e0fb4f09b28535c44b3087d73401c2d3a3741c925c8b4d635ba1` |

Each model directory contains `wire/request-N.json` and `wire/response-N.json`, the model/runtime manifest, preflight result, acceptance log and runtime ownership samples. `per-claim.json`, `per-request.json`, `scores.json` and `artifact-audit.json` provide the derived decisions, costs and independent audit. Raw reasoning is retained locally; this public report does not quote it. The approved expected labels are never present in model prompts.

## Gap audit

DONE for the approved seven-setup experiment and artifact audit. Independent review of the results is next. Production qualification and PR116 merge readiness remain NOT DONE; no diagnostic pass lifts that hold.
