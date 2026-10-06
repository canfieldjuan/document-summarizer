# PR116 blocker fix contract

## Operator acceptance and sequence

The operator explicitly accepted contract revision `3b17a09` as written in
[discussion_r4196477618](https://github.com/canfieldjuan/document-summarizer/pull/116#discussion_r4196477618),
posted on 2026-10-06 at 14:24:15 UTC: "Keep 116 until C9 qualifies. Accept the gate contract".

The earlier request, "Address those open blockers", was direction to address the
findings, not contract acceptance. Codex incorrectly treated it as authorization
to implement the proposed contract. Implementation `42fc08c` was written and
published before explicit acceptance: the recorded push advanced PR116 from
`49e22a7` to `42fc08c` before the publication receipt at 14:02:13 UTC. The P1/P2
code-defect resolutions were also recorded before acceptance. This separate
contract commit records the actual sequence; it does not imply prior approval or
rewrite published history. The comment's description of the branch as unpublished
was stale when posted.

The accepted verification plan still applies. Its recorded implementation proof
is attached to `42fc08c`; this acceptance-only commit changes no code, tests,
fixtures, configuration or dependencies and does not rerun those tests.

## Operator-controlled merge hold

PR116 stays held until C9 passes full-document proof and unseen qualification,
and the operator explicitly clears
[the hold thread](https://github.com/canfieldjuan/document-summarizer/pull/116#discussion_r4196409571).
That thread must remain unresolved until then. The operator chose not to ship C9
gated off. Green CI, review, contract acceptance and resolution of the P1/P2 code
defects do not authorize merging this PR. The separate test-only prompt trial was accepted earlier in
[issuecomment-6010039229](https://github.com/canfieldjuan/document-summarizer/pull/116#issuecomment-6010039229).
The earlier unaccepted status was an agent tracking error. The single development
trial stopped at its first unit after four calls: A1 was labelled withhold but
returned Supported. No rerun or further prompt revision is allowed. This result
cannot clear the hold; per-sentence verification requires a new accepted contract.

## Root cause

- P1: General coherent admission selects an unqualified comparison protocol without an activation gate. The local worker proof has semantic failures. A PR hold does not prevent ordinary binaries from using that code.
- P2: published head 49e22a7e3422c42ef4202d5048b15e1693ff119f still enumerates token-aligned substrings. The already accepted segment implementation at 4c8b9dc75f1b38f39eb8a9b681b8c04ff8413da6 is six commits ahead and has not been published there. The existing frozen implementation and its prior evidence are preserved.

## Required change surface

- Start an isolated branch from frozen 4c8b9dc and fast-forward the PR with its accepted segment implementation plus the gate.
- Keep the unqualified C9 path available only to Rust unit-test/qualification binaries (cfg(test)); ordinary library/application builds, including all-feature builds, cannot enable it via settings, environment or features. Existing ignored qualification tests remain explicitly opt-in.
- Gate at General synthesis admission before drafting, using the existing verified-source-claim fallback with a distinct truthful warning. Preserve subsequent source-claim verification.
- Gate pending General coherent verification before any inference and already-verified pending checkpoints before new artifact completion. Block directly planned C9 requests too. Do not downgrade to an older prose verifier or rewrite a semantic verdict.
- Preserve reads of completed historical artifacts. Keep source extraction, Story, Connect's existing direct-General delivery path, models, prompts, schemas, caps and labels unchanged.
- Add integration regressions that link the ordinary library (without cfg(test)) even under cargo test --all-features. This is essential: the existing unit tests exercise the candidate and cannot by themselves prove the production gate.

## Explicit non-scope
No fidelity claim, live inference, prompt experiment, new model, cap increase, downstream text rescan, fuzzy matching, release, merge or installed-app update. PR116 stays fidelity-held. No resolution based solely on unpublished code or on completion of a later instruction in a thread.

## Assumptions/blockers
The gate is a safety stop pending qualification, not a fix to the model's semantic decisions. There is no production opt-in. A later qualified activation requires a separate accepted change. The failed candidate and all its raw outputs stay immutable.

## Verification plan

1. Fail-first ordinary-library integration test: General synthesis must select the disclosed verified-source fallback with zero synthesis/comparison requests. On the frozen candidate it instead drafts coherent prose; retain that expected failure.
2. Same input passes after gating, proceeds through source-claim verification, saves and reopens identically without emitting the comparison version. Pending coherent checkpoint fails before any model request. Completed historical coherent artifacts remain readable.
3. Positive boundary: unit-test qualification remains available; replay/segment/admission regressions pass, including exact source preservation and overflow boundaries. Negative boundary: ordinary library remains gated with all Cargo features and maliciously permissive model responses.
4. Focused tests, formatting and strict lint. CI owns duplicate broad suites. No new model run and no relabelling.
5. Cold audit, publish by normal fast-forward, update PR documentation to distinguish candidate from qualified production. Resolve only these code defects after the published head contains their fixes; preserve the separate release/fidelity hold. Record the exact head and defer the next PR check for fifteen minutes.

## Implementation summary
The shared comparison owner now permits the candidate only under cfg(test). General synthesis uses a distinct disclosed source-claim fallback before catalog construction or drafting. Pending coherent verification and final completion use the same activation decision. The protocol predicate stays separate so disabling activation cannot silently select an older prose verifier. Direct comparison planning also rejects ordinary builds.

The accepted segment implementation is included through ancestry from 4c8b9dc. It replaces the substring enumeration at its owner. The old nested token-window enumeration and enum-cardinality escape are removed; exact membership, input/schema limits and semantic aggregation remain. This gate change introduces no new text re-scan or heuristic.

Our earlier segment revision also left shared scripted fixtures expecting the old prompt projection and request count. The test-only adapter now handles the dimension metadata; four callers expect one ledger request plus four dimension requests. Production response validation is unchanged.

Local proof:
- Three activation regressions failed before their origin fixes: synthesis reached the runtime, pending verification reached inference, and a Verified checkpoint created a new summary.
- Five ordinary-library integration regressions pass with all features: those three boundaries, unsupported source claims withheld, and completed historical workspace views reopened identically.
- Summary module: 371 passed, 18 opt-in tests ignored, including Story, Contract, source verification, segment/parser boundaries and historical artifacts.
- Segment planner admits 4096 combined characters and rejects 4097 under both runtime schema limits. Qualification unit tests still run; no live model calls were made.
- Frontend build, cargo fmt check and strict all-target/all-feature clippy pass.

No new semantic fix or qualification is claimed.

## Cold diff audit
- `summary/comparisons.rs:34,38,481`: shared activation decision and planner admission. No setting, feature or environment opt-in exists in ordinary builds.
- `summary/coherent.rs:2939,7837`: gate before source catalog/drafting; validate the new fallback as owned source claims. The new warning has a distinct qualification explanation.
- `summary.rs:875,1298`: saved protocol metadata gates verification and completion, before model inference or new output artifacts. Completed workspace reads are not changed.
- `summary.rs:5559` and its four scripted call-count assertions: test-only projection/caller repairs required by the inherited dimension protocol. `comparisons/tests.rs` exercises the shared adapter for each recorded dimension.
- `tests/c9_production_gate.rs:139,180,261,276,290` and `tests/fixtures/c9-public-checkpoint.json`: public fixture, ordinary-library proof and historical workspace reload. The fixture comes from scripted pre-fix public pipeline calls, not semantic model evidence.
- `docs/CONTRACTS.md` and this contract: truthful current activation/status documentation. Existing accepted segment/dimension contract history remains intact.

boundary-probe: ordinary builds reject C9 despite a runtime advertising every schema; supported source claims still save/reopen and unsupported source claims save no result. Both pending checkpoint stages stop; completed historical workspace reads succeed. Unit qualification remains available. Existing invalid/partial/wrong-side/duplicate segment tests and 4096/4097 input boundaries pass. The activation decision has no falsy/default input.

effect-trace: prevent unqualified General prose delivery | shared activation decision at synthesis, verification, planning and final completion | three fail-before regressions now pass in the ordinary library with all features; no synthesis/C9 request or new unqualified result escapes. Source-claim verification and historical reads remain exercised.

## Gap audit
DONE for implementation and local proof. The publication receipt must identify the pushed head; this document is not a claim that unpublished code resolved a thread. Exact-head CI/review and semantic qualification remain separate open gates. PR116 remains on fidelity/release hold; no merge or installed-app promotion.

## Contract revision from code inspection
`complete_verified_document_with_delivery` can complete a saved Verified checkpoint without calling verification again. Apply the same activation decision there before creating artifacts. This consumes the saved protocol/presentation metadata, preserves completed reads, and closes the same unqualified-output blocker. Add a fail-first regression restored to the recorded Verified transition.

The adjacent scripted-verdict caller also reproduced an inherited segment-candidate fixture defect: the shared test-only VerificationPrompt adapter rejected claim_segments/dimension metadata. Update that adapter at its owner, keep production strict parsing unchanged, and cover every dimension through the existing recorded-control regression. Adjacent callers must pass before publication.

## Continuation test correction

CI at `83d3f83` exposed two remaining test callers of the inherited dimension
protocol. Both failures reproduced locally: observed generation counts were six
versus three and five versus two. These tests still assumed one comparison call;
the planner correctly makes four dimension calls. This omission came from our
earlier protocol change.

The test-only counting runtime now records requests, and the checkpoint tests
require one ledger request plus stage, conditions, qualifiers and scope once
each, with verification ordinals zero through four. Expected synthesis and
verification counts are corrected; health-call and completed-artifact invariance
assertions remain. No production path or expected semantic verdict changes.
The adjacent service suite passes 16 tests, with one live test ignored; strict
all-feature lint and formatting pass. CI owns the broader repeat.
