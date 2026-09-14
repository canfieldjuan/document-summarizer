# Bind cross-window General recovery to the rejected source set

### Contract

Root cause:
- Long General synthesis already makes one bounded retry after `MODEL_SUMMARY_RESPONSE_WINDOW_MIXED`, but the retry prompt carries only generic feedback and the application validates only the newly returned response.
- The application does not bind a successful retry to the rejected response's valid siblings or to every source ID from each mixed unit. The existing correcting test demonstrates the gap by accepting a repair that retains `s1` and omits `s3` from the original `s1/s3` mixed unit.
- The final PR #51 code added this retry after issue #53's two failing live runs. A current-`main` GPU run completes successfully when the model emits a valid initial response, so this slice targets the unproven recovery boundary rather than adding another retry.

Required change surface:
- In `src-tauri/src/pipeline/summary/coherent.rs`, derive immutable window-repair requirements from the rejected response, include that untrusted response in the one repair prompt, and accept the repair only when valid siblings are unchanged and every mixed unit's original evidence set is covered exactly once by single-window replacement units.
- Preserve the existing safe-sibling fallback with an explicit cross-window warning when a retry repeats, omits, rewrites, adds, or otherwise violates the repair contract. Continue to fail closed when no safe result exists.
- Extend the existing window-repair regression with fail-first omission, sibling rewrite, added-unit, repeated-mix, repeated-source, independent framing/clipping/modal defects, invalid-unit ordering, later-repair ceiling/fallback ordering, malformed siblings, all-mixed, and valid-split boundaries. Assert the repair prompt carries the rejected response and remains one attempt.
- Record the verified result and current-head GPU acceptance in `docs/BUILD_LEDGER.md`.

Explicit non-scope:
- Do not change source selection, selection-window construction, summary prompts outside bounded repair instructions, Story or Contract behavior, source-framing classification, semantic verification, citations, routing, UI, persistence schemas, gateway/direct runtime selection, model qualification, dependencies, or issue #52's oversized framing-repair design.
- Do not add a whole-run retry, more than one window-repair request, silent source dropping, or deterministic prose assembled from lossy extraction claims.

Assumptions/blockers:
- Evidence IDs remain the application-owned identity used to compare response source sets after request IDs are parsed.
- Existing completed summaries remain valid; this changes recovery admission for failed generation attempts and does not require a persisted schema or result-identity version change.
- No blocker is currently known. If including the rejected response cannot fit the existing bounded context, the current safe fallback or fail-closed result remains valid; issue #52 retains ownership of broader oversized-repair compaction.

Verification plan:
- Add an expected-failing regression showing the current code accepts a repair that omits one original mixed source.
- Run the focused window-repair test with both sides of every integrity boundary.
- Run the complete Rust all-target/all-feature suite, strict Clippy, Rust formatting, frontend production build, and `git diff --check`.
- Re-run the public 111-page DOL acceptance on `qwen3-30b-a3b:latest` at 100% GPU if product code changes after the current baseline; inspect representative wording and source support, not only schema success.
