# Accepted single replacement Synthesize invocation

## Acceptance and inventory

ACCEPTED by the operator, relayed in PR126 issuecomment-6099751504. This contract-only amendment records exactly one replacement for the lost Synthesize-only invocation. Commit it before any implementation. The original combined run and lost Synthesize-only reservation remain spent and immutable; this is neither a resume nor a budget reset.

The preceding accepted contract is native-stage-qualification-contract-20261009/synthesis-after-budget-stop/CONTRACT-ACCEPTED.md, SHA256 d9341331f7e8b877e9f827148db50b70d1f364376abcb7d35900ea0aee7aa58f. Its inventory83774cb1790d6ebdf41cdd777101c2497c851d9c1288a1a837e1362010cdf401 and tested local57b0e4e remain the specification: selected Qwen3.5-9B Q4_K_M, context32768, thinking off, pinned native server/libraries, repaired message boundaries, /completion transport, saved fresh Analyze artifacts validated/reloaded through current owners, unchanged Synthesize planner/parser/schema/source-review checkpoint. C9's30 existing case checks and fresh Analyze10 completions are retained proof, never regenerated here. The lost run admitted its request after31 delegated preflights and performed no completion. Independent criteria a91f1a26 remains unchanged.

## Root cause

The historic termination source/signal remains UNDETERMINED. A zero-model cross-turn probe survived, so ending a turn is not established as the cause. Separately, my private native-stage run_once.py:114-155 registers no signal handler. Default SIGTERM exits the interpreter before cleanup/receipt writes. Reusing that owner and injecting SIGTERM reproduced missing exit.json and owned-runtime.json and a surviving child; ordinary approval wrote both receipts. The introducing owner was my untracked qualification adapter, not a demonstrated production or model regression. Its preserved lost-run source has SHA256 5be8542b3a491eab0c1a1fcdf58434f4b6420dcb3bec5598b6bf2899f0ef0752. There is no tracked introducing commit to invent.

## Required change surface and every departure

1. Extract the existing native-stage supervisor into scripts/native_qualification_supervisor.py, keeping descendant discovery, pidfd server-pin ownership, shared evidence utilities and normal/deadline cleanup. The replacement adapter calls this one owner; remove its copied supervisor implementation. Immutable spent exports are records and stay untouched. Other already-qualified lanes are outside this repair.
2. Handle SIGTERM, SIGINT and SIGHUP at the supervisor origin. Record the signal name/number, mark the invocation interrupted/failed, terminate/reap its direct child and pinned owned servers, and write both final receipts. Repeated handled signals during cleanup must not interrupt receipt writes. Preserve ordinary completion and timeout outcomes. SIGKILL cannot be caught; never promise an in-process receipt after SIGKILL.
3. Launch the replacement driver as one bounded user-systemd transient service, outside tool-process lifetime. No restart, no extra invocation, unchanged1800-second runtime wall, control-group cleanup and bounded stop timeout. An independent ExecStopPost receipt records service result/exit identity even if the supervisor cannot write. This is an evidence/cleanup last line of defense consuming systemd's termination signal, not a second inference or parsing owner. A stop receipt alone never counts as a completed qualification or substitutes for missing final receipts.
4. Use a new private durable replacement namespace, new exclusive reservation and fresh output directory. Copy the unchanged frozen Rust export/inputs; update only the runner/supervisor freeze manifest. Validate saved identities and original hashes with existing owners. No source, prompt, framing, model, schema, sampler, capacity or admission changes.
5. Approval carries over only for byte-identical packet5b48b3e5c458c768497ee4a7b968b5a2d1c4846dad427770bf951d08d80f9652 and request1bbd41b45b186b66ec305fc8417e20095cf5fc7ab1edc8c1fa3979d6eb6d4d40. Approval SHA256 d943ee98896468d986ee86c56099adeb2fb249c734869bc9a9f6a0dd0fb7ecdf. Stage the approval only after checking its exact bindings. The existing in-wall polling checkpoint independently verifies the actual newly produced bindings; a difference blocks dispatch and requires fresh independent review, with no retry or extra reservation.

## Verification plan

Before a model run: fail-first test against the unchanged old owner must show SIGTERM loses final receipts, then the same regression must pass on the repaired owner. Cover SIGINT/SIGHUP, normal and child-error completion, wall timeout, repeated signals during cleanup, and injected uncatchable loss with independent service evidence. Assert child termination, exact signal identity, both receipts and no falsely successful result. Reuse pidfd ownership; do not signal unrelated processes.

Use the same detached-launch owner for a zero-GPU/no-model driver and delayed-approval child across an actual end of turn. Both survive and consume approval; final and independent stop receipts exist. Post both lifecycle proofs on PR126 before any model run. Compile/lint the changed Python files and run their focused tests; do not duplicate unrelated Rust suites for a Python supervisor repair. Cold-audit all tracked and private adapter differences and hash-bind the proof source.

## Live order and stop rules

Exactly one new invocation; at most1 Synthesize completion,64 delegated preflights,1800 seconds including source review, unchanged native request timeout. The existing enumerated owner bound61 remains; any unexpected path is rejected by the existing counters. No Analyze or C9 regeneration, raised cap, retry, repair or resume. Preserve the shared inference lock and free-GPU prerequisite; never stop another app. Freeze all new runner/source/runtime/input/approval pins before reservation. On error or uncertain termination the replacement is spent. Capture output, exact wire/raw response, finish/tokens/latency, tokenizer sends/decodes, cleanup, lock release and independent service stop receipt. Independent synthesis fidelity review remains mandatory after successful execution.

## Explicit non-scope and blockers

No production code, settings, inference policy, prompt/schema/source changes, product behavior, native profile removal, model-host migration, dependencies/workflows, install, push to PR116/100, merge or hold resolution. Browser messaging is prohibited. Unknown historic termination remains recorded rather than invented. Detached turn proof and signal regression block the live run. A changed source packet/request requires independent re-review. PR126 remains draft; all PR116 native-stage/C9/full-document/unseen/fidelity/operator holds remain.

## Implementation summary and cold diff audit

Contract-only amendment; no implementation or model calls yet. The obsolete copied supervisor and default-signal cleanup assumption are removed from the replacement adapter rather than supplemented by another approval loop. Repaired code/test/evidence hashes and all adapter differences must be recorded after implementation.

## Gap audit

NOT DONE: supervisor regression/repair, independent service receipt, detached cross-turn proof, replacement freeze/reservation/result and independent fidelity signoff. Acceptance authorizes the bounded lifecycle repair and exactly one replacement only after its listed prerequisites pass.
