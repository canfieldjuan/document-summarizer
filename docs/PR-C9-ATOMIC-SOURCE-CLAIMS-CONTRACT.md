# C9: atomic source-bound claims and per-claim admission

## Status and authority

**PROPOSED — contract only. No implementation or live dispatch accepted.**

This proposal follows the operator's three decisions relayed in
[PR132 comment 6221656443](https://github.com/canfieldjuan/document-summarizer/pull/132#issuecomment-6221656443):
“Smaller claims (Recommended)”; “Mark it, keep the rest (Recommended)” for a
single governing clause over 4,096; and “Open an issue, build later
(Recommended)” for encrypted PDFs. Acceptance of this document is a separate
step. All implementation choices below, including the overflow signal and
remaining admission-size cases, are proposals for that acceptance.

Code citations below are against `fcf7580bbb0ae90ba56b12cc46731782002f1cb8`.
Paths beginning `summary/` are under `src-tauri/src/pipeline/`; `summary.rs`
and `contracts.rs` are in that same directory. This is a General-profile
candidate only. PR116/126/128/132 holds and draft status remain unchanged.

## 1. Root cause: current behavior and measured reproduction

The writer is told to combine related information into paragraphs containing
one or two sentences (`summary/coherent.rs:80`, `GENERAL_SYSTEM_PROMPT`).
`RawUnit` assigns one `source_ids` list to the whole paragraph
(`summary/coherent.rs`, `RawResponse`/`RawUnit`).
`parse_response_with_limits` turns each such unit into one `ValidatedClaim`
(`summary/coherent.rs:6845`); `materialize_cited_claims` preserves that boundary
and derives a durable identity (`summary.rs:3646`). It does not know which
source belongs to which proposition within the paragraph. Consequently the
comparison must include the union of those sources, with their complete owned
contexts (`summary/coherent/verification.rs:4`, `summary/comparisons.rs:147`).

Measured at the unchanged owners: all nine existing A/B claims fit the 4,096
combined-scalar cap. Seven fail the source enum union cap; B4 and B5 fail the
schema-byte cap. Their individual source catalogs fit, which does **not** prove
that each source plus a smaller claim fits its complete request schema. DV10
has one effective source context over 4,096; DV15 has eleven. These source-size
counts are lower-bound inventory, not measured admission of new DV claims.

The next loss of information occurs in `comparisons::plan`
(`summary/comparisons.rs:376`): collecting a `Result` aborts the whole plan on
one size failure. `coherent_verification_exceeds_runtime_context`
(`summary.rs:2009`) turns that into a whole-summary fallback at synthesis
(`summary/coherent.rs:3195`). After Verify, the separate
`apply_verified_delivery_coverage_fallback` (`summary.rs:3859`) can replace a
coherent result with a ledger and clear its summary claims. Both consumers
must honor the proposed per-claim policy; catching a size error only at final
rendering would leave the origin defect intact.

This is a representation and admission-policy change. The measurements do not
establish a new model limitation or prove that the proposed writer is faithful.

## 2. Proven setup inventory, before any port

Reuse the existing harnesses and evidence in
`PR-C9-PREPRODUCTION-PARITY.md` and `PR-C9-QUALIFICATION-READINESS.md`, not a
replacement scorer. Durable aliases below are in the public
`document-summarizer` evidence namespace; private artifacts stay private.

| Proven work | Recorded result and scope |
| --- | --- |
| Original joint C9 bounded-excerpt runner; `c9-preproduction-parity-20261007/gateway-restored/receipt.json`, SHA256 `54977626b64ee78505f49d26e07b99c2d1e5655bcba8c67fc0eb11bc61e3cd47` | 30/30 verdict matches, 29 adjudicated cases plus one unresolved label; 111/120 relation matches and 227/250 reference containment. This is a case-specific baseline, not perfect passage accuracy or full-document qualification. |
| Corrected native message framing; inventory and E3/E9 receipts in `PR-C9-QUALIFICATION-READINESS.md` | Native diagnostic cases passed their recorded checks; the later stage lane stopped. Historical freeform A/B parity does not qualify a new cited JSON writer. |
| Current parser/guard and source/admission seams; `c9-qualification-readiness-20261016/seams-measurement-report.json`, SHA256 `57ce54ed63953ad08883fb165c1a515b17e35740901bbedf4952f1e310275e5e` | Zero generation/startup/native preflights. Frozen A1 raw response parses Supported, then becomes Unsupported for `evaluation_or_causal_relation`; draft bytes unchanged. A/B and DV results above are admission/source measurements only. |
| SAME60 receipt in `c9-qualification-readiness-20261016/seams-final-proof.json`, SHA256 `5e6884714cedf3781bb985abded7af61afcf9be889079429e` | Full JSON equality across 60 candidates, 825 verdicts, 31 validated summaries. Existing library, ordinary-build gate, format and strict Clippy checks passed on the preceding implementation. No new writer proof. |

Preserve the proven Qwen3.5-9B profile `qwen35-9b-q4km-v1`, model SHA256
`cd76ec205963b3b33350093e6904d9de16c4e666fd104e1f632d25c7f15f2a13`,
server SHA256 `0ca399edd758decd825a71823b04ba7ddbc8b2e10d2309d8bf623ee3c2283099`,
tokenizer `qwen35-pre-cc5fb918-v2`, context 32,768, seed 7, temperature 0,
closed thinking, corrected message boundaries, one slot, offload 999, reasoning
off, no warmup and `cache_prompt=true`. Freeze the complete inherited runtime,
library pins and effective stage allowances from those receipts before any
future run; omitted defaults are not permission to change them. Verify's
output allowance remains 4,096 tokens. The writer retains its existing budget
owner and effective allowance; no output-cap increase is part of this design.

### Every proposed departure

1. General writer instructions require a proposition per output unit instead
   of combined paragraphs; a new writer-only overflow field and schema identity
   make capacity refusal representable. The C9 response schema is unchanged.
2. General writer allocation can use up to the existing eight units, replacing
   its smaller `source_count.div_ceil(3)` ceiling for this revision. Eight is
   still a ceiling, not a target; source, window and framing constraints remain.
3. Comparison admission gains an ordered, typed per-claim size outcome.
   Admitted requests keep the proven prompt, schema, exact excerpt choices and
   parser. One failed admission no longer removes its admitted siblings.
4. New General synthesis/verification identities bind persistence, notices and
   partial delivery to this policy. Historical artifacts and other profiles
   retain their old policy; shared version constants are not blindly bumped.
   Route version-sensitive prompt projection, clause/furniture handling,
   checkpoint validation and schema support through that identity without
   reactivating a historical feature or bypassing the ordinary-build gate.

All four departures require proof below. No other prompt, framing, schema,
sampler, model, clause-boundary, source-selection or limit change is proposed.

## 3. Required change surface and behavior

### Writer and materialization: establish the boundary once

At `GENERAL_SYSTEM_PROMPT`, `prompt_and_schema_for_version`, `RawResponse` and
`parse_response_with_limits` in `summary/coherent.rs`, the new General revision
must emit one independently checkable proposition per unit, with its own
`source_ids`. Actor, action, object, modality and every necessary condition,
exception, exclusion or explicit relationship stay together. A condition is
not a separate optional claim. A fact needing several clauses retains every
necessary clause, even when that causes admission to fail.

The writer performs the semantic separation directly. Rust preserves each
emitted boundary and validates IDs, exact source ownership, count, size, window
and framing constraints before `materialize_cited_claims`. It does not split
sentences afterward, assign citations by word overlap, or trim clause text to
fit. The generic materializer remains one validated unit to one durable claim.
Schema validation cannot prove semantic atomicity, completeness or fidelity;
those remain independent review gates, with missing exclusions treated as
failures rather than justified by smaller output.

**More than eight necessary claims:** propose a required writer response field
`claim_capacity_exceeded: boolean` in the new General schema. `false` requires
one through eight units; `true` requires an empty units array and becomes an
explicit synthesis capacity failure. Unknown fields, a missing flag, a false
flag with no units, or a true flag with units fail parsing. There is no silent
prefix, cap increase, automatic combination back into compound claims, or new
repair loop. The notice says a summary could not be produced within the claim
limit; no completed qualified summary is stored. The model can misreport this
flag or omit facts, so public nine-fact controls and independent whole-draft
review must test it. A grammar limit alone cannot establish completeness.

### Admission: one owner and an outcome for every claim

Refactor the real `Prepared::new`/`plan` owners in
`summary/comparisons.rs` to distinguish invalid input from capacity limits.
Return an ordered plan entry for every claim: an unchanged admitted request,
or `AdmissionWithheld` with claim/evidence identity, a typed reason and the
measured limit. The source owner remains `summary/coherent/verification.rs`.
No second size scanner or text-based fallback rule is introduced.

The accepted direction requires withholding any claim whose single complete
governing clause exceeds 4,096. **Proposed extension within this contract:**
apply the same per-claim capacity treatment when the unchanged combined-scalar,
enum, schema-byte or deterministic request-context check rejects that claim.
This matters for A/B: only handling individually oversized clauses would leave
their measured failures unresolved. These reasons must remain separately
reported, not called model disagreement. Even a one-source atomic claim may
still be too large. Unsupported protocol, missing/invalid source ownership,
parser failures, cancellation and infrastructure failures remain errors; they
are never converted into admission withholds. A runtime preflight rejection
after a supposedly admitted frozen plan also remains an error to investigate.

Consume this plan at synthesis admission and Verify. Do not run comparison or
trim-contribution model calls for an admission-held claim. Preflight every
admitted request before generation, preserve request ordering and seed rules,
and bind actual bodies to the plan. Merge results by exact claim identity into
the full original order; no filtered-vector `zip` or sibling verdict reuse.
Semantic guards may reject admitted claims but cannot promote held entries.

### Persist, reload and deliver the same result

Propose an optional typed `admission_withhold` record on `ClaimVerification`
(`contracts.rs:511`), present only for the new General policy. It records the
reason and measured bound, not a model-produced quote. Its verdict must be
`Ambiguous` for compatibility with the current three-value internal verdict
enum, but reports and warnings count it separately as **not verified**, never
as a model's ambiguous judgment. No record may combine an admission withhold
with Supported or Unsupported. Missing records mean the historical policy,
not permission to reinterpret an old artifact.

`verify`, `verification_warnings`, `validate_coherent_verified_document`,
version compatibility and completion owners in `summary.rs` must enforce that
record against the saved claim, sources, policy and effective limit. Recompute
the deterministic admission decision through the same owner on reload;
no generation or native startup. Tampered/missing/duplicate records, changed
source bindings, unsupported version pairs, or held prose inserted into the
supported list fail validation. Old records remain readable under their
existing rules and are not relabelled or silently upgraded. The storage/DB
round trip must exercise the actual serializers; no database migration is
authorized merely to add optional artifact metadata.

For a mixed result, render only Supported claims with their original citations
and order. Emit a bounded notice identifying each held claim ordinal, such as
“Claim 2: too long to verify; omitted from this summary.” Distinguish the
actual size reason in typed diagnostics. Do not repeat the withheld prose
as if it were verified. Derive notices from the typed outcome, not parsed
warning text. Use existing warning delivery to desktop and Connect; no public
wire-shape change. Prove the warnings fit its existing limits.

For this new policy, admission-caused coverage loss is disclosed partial
coverage, not a reason to replace all supported prose with a ledger. Update
both fallback consumers named in section 1 and their saved-result validator.
Keep the original coverage denominator and omitted-source disclosures. Do not
count a held claim as covered, or waive an unrelated coverage failure. If
there are no Supported summary claims, retain the existing
`NO_SEMANTICALLY_SUPPORTED_CLAIMS` completion failure (`summary.rs:918`), with
admission diagnostics preserved; an empty result or a notice is not a successful
summary. No admission-held-only re-synthesis retry is introduced.

## 4. Unchanged limits, costs and explicit non-scope

| Constraint | Retained boundary |
| --- | --- |
| Summary claims | 8 maximum; 1,200 characters per unit and 8 sources per unit remain |
| C9 input | 4,096 Unicode scalars across claim and unique complete source strings |
| Catalog | 8,192 values per enum; existing exact token excerpts of up to 240 scalars |
| Response choices | Four selections per side per dimension; joint four-dimension response |
| Schema bytes | Native comparison ceiling 1,048,576; gateway ceiling 250,000; actual runtime limit if lower |
| Verification batches | Existing 64 cap; never enlarge or evade it by splitting plans |

For `n` materialized atomic claims, `a` admitted and `h` admission-held, `a+h=n`
and `n<=8`. The C9 comparison component makes `a` completions, one per admitted
claim, and zero for held claims. A/B currently have three and six compound
claims, all rejected before generation. Eight admitted atomic claims would
cost eight comparison calls each, versus zero today. Relative to their old
planned claim counts, that is five and two additional requests respectively,
not measured runtime spend. These are upper bounds, not predictions of the
new writer's output or latency. Ledger verification, source selection,
existing synthesis repairs and trimmed-source contributions are separate
owners (`summary.rs:1279`, `summary/coherent/trim.rs:155`). The 64 batch cap is
not a bound for the entire multi-stage pipeline. No additional retry, repair,
token budget or live call reservation is authorized here. A future execution
contract must enumerate those requests/preflights and actual totals from its
frozen inputs before dispatch.

No encrypted-PDF implementation: [issue133](https://github.com/canfieldjuan/document-summarizer/issues/133)
is deferred. DV09/DV11 remain unmeasured and their four frozen cases cannot
admit; there is no substitution. No new DV claims, label access, relabelling,
O3 rules, #129/#131 repair, model/profile retirement, gateway deployment,
installed-app change, production activation, merge or hold release. Story and
Contract behavior, full clause lists and source selection remain unchanged.

## 5. Verification plan and stop conditions

After explicit acceptance, first commit the acceptance record. Then follow
reproduce → isolate → explain → fix → prove → prevent regression at the owners
above, with public fixtures and declared fail-first probes before editing
behavior. Record the introducing owner/commit for any own-code finding; remove
the old compound-paragraph instruction and all-or-nothing capacity propagation
for this revision instead of stacking downstream phrase rules. Do not remove
unrelated guards or historical readers.

1. **Writer boundary controls:** independent propositions with distinct
   sources; one proposition requiring several clauses; conditions, exceptions,
   modality and parent lead-ins that must stay attached. Faithful positives
   and dropped-limiter/changed-actor twins. Assert exact frozen writer prompt
   and schema at the production request owner. Exercise parser/materializer
   boundaries with empty, one, eight, nine units and every flag/units mismatch.
   Check real native grammar and gateway schema admission for the new writer
   format without generation, including malformed responses; preserve the
   existing C9 grammar and its negative controls. Hand-authored outputs prove
   parser behavior, not writer fidelity.
2. **Actual admission and worker path:** all-fitting, mixed and all-held
   public documents, including one fitting claim and one over-limit single
   clause. Assert requests, generation/preflight counts, result ordering,
   truthful notices, supported prose and actual completion status. Probe both
   sides of every size bound, Unicode scalars versus bytes, duplicate sources,
   mixed invalid ownership, enum/schema limits below 4,096, and non-capacity
   failures that must still abort. No test-only replacement planner.
3. **Persistence and delivery:** save/reload mixed and all-held results
   through the existing store; retry/resume must preserve holds and must not
   dispatch held claims. Tamper verdicts, policy, source/claim identity,
   measured bound and rendered text. Verify fresh and reloaded results agree,
   including coverage shortfall, zero Supported claims, and warning delivery
   through actual desktop/Connect builders with their unchanged limits.
4. **Mutation checks:** bypass atomic-schema/overflow validation, restore
   whole-plan failure, promote or drop a held entry, restore either admission-
   caused whole-summary fallback, suppress its notice, or ignore the saved
   admission record. The corresponding public regression must fail. Keep
   mutation source hashes identical to the published tested bodies.
5. **Reuse offline measurements:** rerun A/B's frozen existing nine claims
   with the same readiness seam: their seven enum and two schema rejections
   remain the baseline; the new policy may represent them individually as
   held but cannot claim they now fit. Rerun only the previously allowed DV
   source inventory, recording identical contexts, quote-only fallbacks and
   omitted-source counts. No saved DV claim sets exist: report their claim
   admission as unmeasured, never generate claims or inspect frozen labels to
   fill that gap. Candidate A/B writer outputs require a separately accepted
   live contract; once frozen, measure their actual claim/source requests
   through this same seam. Do not count synthetic splits as model results.
6. **Regression gates:** full required Rust suite, ordinary-build disabled
   gate, format and strict Clippy. Replay the original 30-case packet through
   the actual parser/guards and SAME60 by complete ordered results, not counts
   alone. Expected verdict changes: **none** on those saved inputs. Retain
   their historical policy identities. New atomic and per-claim admission
   behavior belongs in separate public controls, not edited old fixtures.
   Any unexpected change is a finding: stop and trace the port first.

Independent whole-draft fidelity review must precede any verifier-based claim
of success for a future writer run. Its fixed criteria must reject missing
exclusions and reversed relations; passing the C9 comparator is insufficient.
Freeze the new prompt/schema, requests, sources, complete effective runtime
settings, budget and before-send gates in that later contract. It must state
an exact live budget and stop on failure; this proposal grants **zero** live
calls or GPU work. The unseen 94-call protocol remains unauthorized and blocked
on readiness and #133. Frozen labels remain oversight-owned and unopened.

## 6. Implementation summary

This commit adds only this proposed contract. No source, test, configuration,
dependency, fixture or generated artifact changes; no new measurements, model
calls or GPU use. Existing test evidence belongs to the tested `fcf7580` head
and is not evidence that any proposed behavior works.

## 7. Cold diff audit

`docs/PR-C9-ATOMIC-SOURCE-CLAIMS-CONTRACT.md`: records the selected direction,
current origin, proven inventory, each proposed departure, capacity and saved-
result behavior, costs and gates. Verify the publication diff contains only
this file and no private text. Code tests are not rerun for this docs-only
proposal; an empty relevant-source diff binds inherited evidence to `fcf7580`.

## 8. Gap audit

**NOT DONE.** Contract acceptance, implementation, fail-first/regression and
mutation proof, new writer fidelity/admission measurements, live execution
budget, unseen qualification and operator hold release are pending. Publish
this contract-only commit, notify oversight, and stop for operator acceptance.
