# PR116 blocker fix contract

Authority: operator request, "Address those open blockers." This is the requested safety/publication fix, not acceptance of the deferred prompt trial.

## Root cause
- P1: General coherent admission selects an unqualified comparison protocol without an activation gate. The local worker proof has semantic failures. A PR hold does not prevent ordinary binaries from using that code.
- P2: published head 49e22a7e3422c42ef4202d5048b15e1693ff119f still enumerates token-aligned substrings. The already accepted segment implementation at 4c8b9dc75f1b38f39eb8a9b681b8c04ff8413da6 is six commits ahead and has not been published there. The existing frozen implementation and its prior evidence are preserved.

## Required change surface
- Start an isolated branch from frozen 4c8b9dc and fast-forward the PR with its accepted segment implementation plus the gate.
- Keep the unqualified C9 path available only to Rust unit-test/qualification binaries (cfg(test)); ordinary library/application builds, including all-feature builds, cannot enable it via settings, environment or features. Existing ignored qualification tests remain explicitly opt-in.
- Gate at General synthesis admission before drafting, using the existing verified-source-claim fallback with a distinct truthful warning. Preserve subsequent source-claim verification.
- Gate pending General coherent verification before any inference. Block directly planned C9 requests too. Do not downgrade to an older prose verifier or rewrite a semantic verdict.
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
Pending. No new semantic fix is claimed.

## Cold diff audit
Pending implementation and focused proof.

## Gap audit
NOT DONE. Runtime gate, ordinary-library regressions and publication remain.
