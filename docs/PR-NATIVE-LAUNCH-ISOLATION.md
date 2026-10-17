# Accepted native launch-isolation follow-up

## Root cause
Production's raw first summary differs from the proven original-runner summary despite identical rendered prompt/token arrays and sampler defaults. Cause is NOT ISOLATED. The previous four-call trial preserved both historical outputs with flash auto alone and one slot alone. These are reproducibility targets, not independent fidelity labels or model-capacity proof.

## Pre-production inventory and every difference
The durable inventory alias is summary-launch-isolation-followup-20261007/inventory.json. Original runner SHA256 fb2fff0b00079aa359eda1d634ec9c0ce552c53d912527e29ce4b297de284015, Qwen3.5-9B Q4_K_M, server b1-c1d0e7a, same model/runtime pins, whole-contract freeform messages, disabled thinking, 32768 context, temperature zero, output 1500. Case 1 reproduced 773 output tokens on 8587 prompt tokens; production recorded 720 output tokens with complete EOS. All private source-bearing captures remain in durable heldout storage.

Inventory first: inspect llama_cpp.rs:292-350 (launch), :578-601 (completion), original runner spawn and request, pinned binary help, model metadata, captured properties, full token array and request. Both batch/microbatch flags are omitted and the pinned binary supplies 2048/512, so skip that call. Historical effective batch/thread values are not exposed by production properties; configured defaults must not be mislabeled observed runtime values. Offload requested counts differ (99/999) although both exceed the GGUF's 32 blocks. Inventory records every launch/request difference, including warmup, flash, slots/KV, context, threads, Jinja/thinking, listener/auth, alias/UI/offline, sealed loading/model access, process IO/lifetime, token representation, output/seed/stop/cache fields and identical sampler defaults. Newly found differences are inventory-only.

## Recorded acceptance
The operator acceptance and exact stop rules are recorded in PR116 comment 6044546819. The operator then directly instructed this session to address comments and continue. This separate contract-only commit precedes adaptation and every generation call. The original contract/production source, holds and frozen C9 packet remain intact.

## Required change surface
Durable trial copies and capture/driver seams only. No production code. First variant runs the unchanged original runner, replacing only -ngl 99 with 999. Second variant starts from the original baseline and uses a copied runner that submits the exact originally rendered token array to /completion through baseline loopback TCP. Preserve model, libraries, context, flash on, original slots, warmup, sampler defaults, input bytes and budget. Completion request is exactly the recorded production request: prompt array, n_predict 1500, temperature 0, seed 4294967295, stop [<|im_end|>], cache_prompt true, no schema. These necessary transport-field differences are one declared projection, not a combined launch variation. Raw completion response is captured before any original chat parser/scorer; the wrapper stops after the first response.

Operational differences: fresh private copies, outputs, ports/PIDs, capture instrumentation, exact target check, global reservation and per-call journal. ComfyUI idle before launch and before/after generation, exclusive original inference lock, source/head and pins before/after. Byte-equal original assets; copied transport runner's single request block delta explicitly audited and pinned.

## Explicit non-scope
No production launch/source/configuration changes, prompt changes, model/capacity conclusion, combined variations, warmup or new-variable trial, case 2 generation, verifier/gateway changes, fidelity labels, page limits, merge/deploy, v2 work or hold release. Existing output word-limit and semantic limitations remain.

## Assumptions/blockers
Acceptance authorizes at most three calls, not an obligation to spend all three. Batch values are identical, so plan at most two calls. GPU/lock contention, source drift, invalid/incomplete response, hash/request/render/token/default mismatch, missing ownership/provenance or uncertain call stops the attempt. A match isolates the nominated projection for this recorded input; it does not qualify every stage or prove fidelity.

## Verification plan
Before generation, boundary-probe exact target versus near/empty/wrong-case values, reservation type/budget and fourth-call denial, one-case stop, launch delta, transport payload/token equality and copied-source diff. Offline pins for each packet. Journal before every HTTP generation; uncertain calls stay charged. Freeze script hashes/source head. Stop immediately at first byte-for-byte match to production content; no confirmation/retry or second case. Normal mismatches permit only the next admitted variable. Capture raw request/response, settings, template/token array, loaded libraries, process identity, complete finish and reasoning state. Afterward compare original, historical and production bytes, check cleanup and unchanged production diff. No broad suite for this document-only commit.

## Implementation summary
Contract and inventory only at commit time; trial adaptation pending.

## Cold diff audit
Contract-only new document. Durable inventory is separate; no runtime code or existing fixture changed.

## Gap audit
NOT DONE: no live calls yet. If neither admitted variable matches, stop and report; only the operator can choose further exact parity or quality comparison. PR116 operator hold and full-document admission remain open.
