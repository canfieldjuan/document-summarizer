# Accepted single-field message-boundary trial

Status: ACCEPTED. The operator directly replied "i accept it" on 2026-10-07 before adaptation or inference. PR116's prior accepted trial stopped on the completion projection match; this is a new scope. No production fix is authorized.

## Root cause and proven inventory
Completion transport on the original baseline launch reproduced recorded production case 1 exactly (720 output tokens), while chat produced the historical baseline (773). Offload 999, flash auto and one slot separately retained baseline bytes. Batch/microbatch configurations are identical and skipped. The 9B's demonstrated success remains the reference; no model/capacity conclusion follows.
Pinned original runner SHA256 fb2fff0b00079aa359eda1d634ec9c0ce552c53d912527e29ce4b297de284015, Qwen3.5-9B Q4_K_M model cd76ec205963b3b33350093e6904d9de16c4e666fd104e1f632d25c7f15f2a13, llama-server 0ca399edd758decd825a71823b04ba7ddbc8b2e10d2309d8bf623ee3c2283099, runtime b1-c1d0e7a/source c1d0e7a004015f23bc0233470b747b596f29b264. Baseline launch: offload99, flashon, four auto slots, context32768, warmup enabled, thinking false, original alias/TCP. Case 1: fixed output1500, temperature0, original full8587-token array.
Zero-generation actual parser/schema probe against the pinned installed libraries shows a user boundary at41 for chat and none for native. Native plus message_delimiters from that parser reconstructs exact chat spans and keeps serialized sampler settings identical. A standalone public30-token fixture reproduces/restores the same omission. Source owner: native CompletionRequest at llama_cpp.rs:2090 (original446dd3b4), projected by :586. Pinned server supplies metadata at server-common.cpp:1350, assigns spans in server-context.cpp:4193, and checks user boundary for eligible batch splitting at :3434.
This proves missing metadata and an execution-path difference, NOT that it caused the measured output divergence. Chat parser/preserved tokens/generation prefix/stop differences remain listed. No diagnosis by model insufficiency.

## Every difference in this trial
Reuse the already successful completion-copy runner and capture/one-case stop seams. Start from its exact native wire request, original baseline launch and complete token array. Add ONLY message_delimiters, byte-for-byte values from the pinned actual chat parser output. No generation_prompt, preserved_tokens, chat_parser, reasoning-budget fields or other input field is added. Preserve native explicit seed4294967295, stop[<|im_end|>], cache_prompttrue, n_predict1500 and temperature0. Keep launch/runtime unchanged. Fresh port/PID/output copy, durable reservation and capture instrumentation are operational differences.
Before inference, evaluate both native task schemas through the pinned parser: sampler fields must remain identical and actual message spans must equal chat's. Exact original token array, request delta, settings, model/server/library pins and process ownership must pass.

## Required change surface
After acceptance only: one durable copied runner packet, capture seam and single-attempt driver, adapted from the already validated completion transport trial. No production source changes. Contract-only commit records acceptance before adaptation/call.

## Verification plan and ceiling
ONE generation call, case1 only. Journal before HTTP generation; uncertain calls charged. No repeat, alternate variable, confirmation or second case. Response must be complete EOS/word, nonempty, correct prompt/output accounting, no reasoning markers, and unchanged sampler defaults. Compare raw content byte-for-byte to the original historical baseline SHA256 d0a8ae37f72260767c03d00e4b634d1b4e002b087fe87bc85af143f62340ed96; also record comparison to native720 target. Preserve raw wire/settings/response, tokens, model/runtime identity, library hashes, latency and finish privately. Stop immediately on response/error/mismatch/match. No automatic next trial.
Offline probes reject extra/missing/mutated metadata, changed tokens/fields/settings, malformed/truncated/empty response, near target and a second generation. Public minimal fixture is the regression input for boundary ownership; it is not a substitute for live semantic proof.

## Non-scope and assumptions
No production runtime/code/profile/stage change, model or prompt change, limits, fidelity/capacity label, verifier/gateway work, combined changes, unseen run, v2, merge or hold release. Checkpoint eligibility and numerical effect are not independently measured in the zero-call trace. A match would isolate the added metadata's effect on this frozen input under the baseline server, not qualify production's other launch or transport changes. Production repair requires a separately accepted contract and stage-specific re-qualification.

## Implementation summary
Acceptance recorded before adaptation or inference. No candidate producer or live call at this commit.

## Cold diff audit
All existing production source and frozen evidence remain unchanged. Only durable trace/proposal artifacts.

## Gap audit
NOT DONE: accepted; adaptation and the single live call remain pending. Regardless of outcome, stop after the one call and report. PR116's hold and full-document admission remain open.

## Acceptance receipt and citation erratum
Original proposed contract SHA256 817bb88d79e9f3fcf6c6efb3d87b920551b7e020df73997e52400eae8d514bd4 remains frozen in durable storage. The citation erratum corrects message_spans assignment to server-context.cpp:4193 and native raw-body forwarding to :4785; no scope, field, input, budget, target or stop rule changed. The operator accepted the proposed trial explicitly; this commit precedes implementation and every call.
