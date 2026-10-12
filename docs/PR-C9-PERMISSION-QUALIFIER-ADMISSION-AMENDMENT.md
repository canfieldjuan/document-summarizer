# Proposed admission-seam amendment — permission qualifier trial

Status: STOP after the fail-first probe; verification amendment proposed, not accepted. No policy implementation has begun.

## Contract

### Reproduce

Accepted implementation scope ab72322 added the reviewed37-case public fixture harness. At unchanged f7ca3f5 production owners, `cargo test --lib permission_qualifier_fixtures_preserve_final_verdicts -- --nocapture` exited101: seven faithful nonliteral permissions were Unsupported as expected, but empty_evidence_twin returned Supported. Log alias qualifier-fail-before.log records all37 final verdicts. The unexpected result triggered the accepted stop rule. Frozen original residual owner trace was also replayed privately; no generation was used.

### Isolate and explain

The failing boundary input is an EvidenceItem with exact_quote="", cited by an explicit permission claim, supplied directly to apply_semantic_fidelity_guards. permission_equivalence's no-source fallthrough returns an empty successful modal projection; the bare semantic guard therefore retains the model's Supported verdict. Blame identifies our afabcb9 as the origin of that fallthrough. This does NOT demonstrate that production delivers a claim with empty owned evidence.

Production Verify first calls validate_synthesized_document_without_runtime (summary.rs:1296), which calls validate_analyzed_content (:4312). That owner checks canonical_bounded_text for exact_quote (:4157); canonical_bounded_text (:5453) rejects empty input. An admission probe exercised that actual Verify path with the exact_quote cleared and its deterministic evidence identity recomputed: INVALID_ANALYZED_DOCUMENT, zero Verify dispatches. The otherwise valid public fixture passed preflight before the one-field alteration. There is no empty-evidence product repair justified by this reproduction.

I introduced the wrong verification seam in this trial's new fixture harness: an invalid EvidenceItem was treated as a prevalidated input to a semantic relation comparator. The first repair belongs to the fixture's admission route, not an extra empty-text filter in the comparator. Other nonliteral fixture failures remain at the registered permission identity owner and still need the cleared implementation.

### Proposed narrow required change surface

Preserve the approved fixture texts and their requirement that no unsupported claim reaches delivery. Register empty_evidence_twin as an admission-invalid control, asserting INVALID_ANALYZED_DOCUMENT and zero Verify requests at the production preflight owner. Keep the other36 controls as final semantic-verdict tests over admitted evidence. Explicitly report these two test seams; do not skip the empty control, silently count a missing permission as supported, or change the original labels into faithful ones.

After oversight clears this verification amendment and resumes the stopped trial, implement only the previously cleared permission qualifier and three formal projections. No additional semantic rule or general empty-source policy is proposed. The qualifier's affirmative same-item license, literal-agent comparison, bounded agent extent, subject-relative tense/polarity constraints, distinct-tier alignment and number extent boundaries remain unchanged.

### Explicit non-scope

No production parser/comparator, writer, prompt, schema, model/server/settings, runtime, budgets, source association or persistence change in this STOP packet. No GPU/generation/tokenizer/delegated preflight calls. No baseline relabeling, original fixture edits, new corpus, B1/B2 or replacement run. All PR116/production/qualification/merge holds remain.

### Assumptions and blockers

The reviewed Unsupported outcome for empty evidence describes withholding, but the recorded guard-only result exposes ambiguity about which owning seam must prove it. Oversight must clear the exact admission outcome/test route before changing the registered harness. The production admission proof must not be represented as completed permission equivalence repair.

### Verification plan

Keep the original fail log immutable. Preserve the passing real-Verify admission regression. After clearance, keep every37 control exercised at its documented owner, show public fail-before/pass-after, retain all negatives, then owner mutations, archived C9/A1/saved gates and the same60 full-verdict replay. No additional semantic scope is inferred.

## Implementation summary

Acceptance record ab72322, additive37-case semantic probe and one production admission regression only. No policy fix. Seven expected faithful rejections remain. Production preflight rejects the invalid empty quote before dispatch.

## Cold diff audit

semantic_support.rs adds the reviewed fixture runner only; its production tail is byte-identical to f7ca3f5. relation_guard_permission_fixtures.json adopts the approved33 texts/labels and four required negative twins. summary.rs adds only the real-Verify admission test. This proposed amendment records the unexpected probe and distinguishes a harness defect from a product finding.

## Gap audit

NOT DONE: the trial stopped before policy implementation. Narrow admission-seam clearance, corrected harness, permission repair, mutations, frozen replay, publication and exact-head reviews remain. The expected-failing probe is preserved locally; this packet is not a green candidate and will not be pushed as a fix.
