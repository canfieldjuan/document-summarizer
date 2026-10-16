# PR128 source clause ownership consolidation proof

## Contract

Operator acceptance and oversight issuecomment-6137493893 accept the exact third-round root-cause note. Contract acceptance commit a9dfcae precedes the policy edit. Restore only per-clause source aggregation inside each OwnedPermissions; no new parser, permissive parse, evidence merging, downstream re-parse, activation or qualifier change. Claim-side strictness and both reviewed P3 observations remain outside scope. All PR116/production/qualification/merge holds remain. Zero generation, preflight and tokenizer calls.

## Reproduce → isolate → explain → fix → prove → prevent regression

1. Reproduce: the seven-case public probe at published285ad6e plus the additive test retained four wrong Supported verdicts, with three controls Unsupported; fail-first exit101.
2. Isolate: permission_equivalence's OwnedPermissions construction, now semantic_support.rs:3956. The shared conservative reader returns None for the text it is asked to own; the caller erroneously expanded that failure scope to an entire evidence quote. Current modal consumers already invoke the same reader per clause.
3. Explain: my afe5fdd consolidation changed f7ca3f5's per-clause source collection to whole-quote permission_propositions(...).unwrap_or_default(). A malformed or negated sibling erased the recognized qualified permission. The no-permissions branch then admitted the injected Supported verdict. This is an aggregation regression introduced by me, not model insufficiency or a precision cost.
4. Fix: use existing semantic_clauses and the shared raw reader per source clause before collecting each evidence owner's permissions. None removes only its clause. Successful siblings retain their existing evidence_id. Remove the whole-quote source defaulting. Raw parser, claim strictness, modal consumers, literal form activation, source ownership and qualifier license remain unchanged.
5. Prove: final permission6 tests pass, including all seven new withholds and every registered permission control. Archived C9 keeps all30 recorded verdicts. Original A1 is Unsupported. Saved-completion/reload probes pass with0 generation calls. SAME60 full ordered lists against f7ca3f5 and original baseline show only the approved ART0051 ledger claim-621537aaf3aeab33e3e46183fca58f13ec3ce915d5a8f7ddac013ff845f4c436 unsupported→supported (baseline supported). No other ledger/summary changes, zero releases past baseline, saved validation31 unchanged. Format and strict all-target/all-feature Clippy pass. Clippy initially rejected my redundant IntoIterator conversion; it was removed and affected tests/lint rerun. The exact no-behavior-diff receipt binds earlier replay/C9/A1/saved evidence to final bytes.
6. Prevent regression: commit permission_source_clause_failures_preserve_owned_siblings at :322. Both directions, mixed valid/unparseable text, malformed list, trailing incomplete may-not, single valid-source controls and a separate invalid evidence item are covered. Reverting only source aggregation to whole-quote defaulting reproduces exactly the four wrong approvals. The mutated final input is byte-identical to the recorded tested mutation input; scratch source was restored. Existing faithful/twin tests remain passing.

## Cold diff audit

- semantic_support.rs:322 adds the public seven-case regression. At :3956 the only policy change restores the original clause-level collection in the existing owned source constructor. No new scanner, helper or fallback; no other code changed.
- PR-C9-PERMISSION-CLAUSE-OWNERSHIP-ROOT-CAUSE.md records the reviewed diagnosis, reader inventory and acceptance before policy implementation. Immutable proposal remains in durable evidence.
- This proof records observed results and limits. The local broad suite was not repeated to duplicate CI; targeted permission tests, archived C9/A1, saved completion, frozen exhaustive replay and strict lint are the accepted local gates.

## Gap audit

Offline repair and accepted local gates complete. NOT DONE: fresh exact-head CI, bot reconciliation and independent oversight PR review. P2 is reproduced and repaired locally but not independently cleared until the pushed head is reviewed. No inline thread exists for it; the review comment remains first-class. Both PR116 hold threads and every production/qualification/merge hold remain open. No model run, install or merge follows this proof.

## Durable evidence

Public aliases under c9-guard-origin-20261011; private comparison under c9-guard-precision-inventory-20261012. SHA256:

- permission-clause-ownership-fail-before.log: 24afef7aa93c7295125ca0baabc4ac98003742889d30968da13831746d5dfee3
- permission-clause-ownership-targeted-final.log: efc7fd956137577c3dc0f13ae1bd9b90d58ada76617795326d3b4f7664243be2
- permission-clause-ownership-aggregation-mutation.log: caa485e3caf42f7059be932cbc853e225b1e8b027caeeaed63151c2e37de0d9a
- permission-clause-ownership-c9.log: 2d7e7fe641277662e25cfc9aef35c131c6d50410878fa4b560097a921ecb9bdd
- permission-clause-ownership-a1.log: 07f0bb1879d961210021aeb7f97ac8be894750e333a9f13744505ea049401e09
- permission-clause-ownership-saved.log: a047ac1c11eb0b27ff40e0aba75159d45e79ecfdd60cde9f986d2445992c1fb6
- permission-clause-ownership-fmt-final.log: e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
- permission-clause-ownership-clippy-final.log: 7f7212b6ea6ed73bae7b5bc9ea028b480571415916fe656e526365a04d181de7
- permission-clause-ownership-final-source-binding.json: e74d2ecefa7a82e7a091390f52d1bbae7bfb4bf1d8cc8caa6b5d2e36b03f3f31
- private:permission-clause-ownership-full-comparison.json: 315a81f857f376ac38ab3ec23cb37ead598c3d03cf52878e9dacbcb51ca8d6bb
