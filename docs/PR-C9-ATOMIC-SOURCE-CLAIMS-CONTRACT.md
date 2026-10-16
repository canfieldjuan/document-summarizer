# C9: whole-document coverage and atomic source-bound claims

## Status, inventory first, and authority

**PROPOSED revision — contract only; stop for operator acceptance.**
No implementation, test execution or live dispatch is authorized by this revision.

The operator's [match-preproduction decision](https://github.com/canfieldjuan/document-summarizer/pull/132#issuecomment-6222114647)
supersedes “Pick top 8, disclose”. The eight-claim ceiling, fixed 16,000-character
writer cap and `claim_capacity_exceeded` proposal from the previous revision
are withdrawn. The [earlier extension decision](https://github.com/canfieldjuan/document-summarizer/pull/132#issuecomment-6222032831)
retains per-claim withholding for every size reason. Encrypted PDFs stay in #133.

The later oversight mailbox correction `0bfa6682`, 2026-10-16 18:53:10 UTC,
invokes the operator's standing rule against weakened alternatives. It directs
one full-section-coverage proposal for over-context input, replacing the public
comment's earlier request for options. It is a later design direction, not
operator acceptance of a detailed implementation. This document proposes that
single path and returns to the operator. The earlier “build once it matches”
sequence does not bypass this acceptance boundary.

Code references are against `67cf1611a4619ca35625bd1299c9ae93a946cfb0`.
Paths below are relative to `src-tauri/src/pipeline/` unless prefixed `docs/`.
All PR116/126/128/132 holds, draft status and the ordinary-build gate remain.

## 1. Canonical preproduction inventory

SAME60 is one regression packet; it is not the whole preproduction baseline.
The following experiments have different inputs, protocols and runtime settings.
Do not combine their claims of proof. Durable aliases are in the existing
`document-summarizer` evidence namespace unless marked private.

### Whole-document synthesis: capacity and reproducibility reference

- Public `synthesis-port-inventory-20261007/inventory.json`, SHA256
  `cf911522bbed1e680562b2b48e8a8280c1ef7002e4014c2a7fe448ab85364879`;
  production inventory revision `43d3fca56a6950a2e4ea9df27965c33b53aac225`.
- Private recovery packet `preproduction-proof-recovery-20261007/summary-rerun/`:
  `summary_run.py` SHA256
  `fb2fff0b00079aa359eda1d634ec9c0ce552c53d912527e29ce4b297de284015`;
  actual `summary_models.json` SHA256
  `e2f78fcf5d42b0153f1cfb534e424ec428e9a661fcfd822251b82ad99a60593b`;
  `summary_facts.py` SHA256
  `81348c4600e62d93a7015f67e9421dbf1c7a489e11a352466828f29b9cc90c17`.
- Saved `execution.json` SHA256
  `7e3bc6b28a5ed6e9511e94ed0e21980c02f760649f9a467b670c576c9ae290fb`.
  Capture manifest SHA256
  `a4cec3166ebe9d883a3b9321e7ae0ab357a9ec114efe41e1cf07111fa9522d24`
  binds both requests, wire payloads, raw responses, templates, tokens and props.
- Original invocation reference: `python summary_run.py qwen35-9b`. The preserved
  capture wrapper sets that argv; SHA256
  `01b089ac6327400cccb62a74659009cda5d41ca464edd8571b467b693f7da65c`.
  These are command references, not dispatch authorization.
- Whole contracts in one request: 8,587 and 18,420 prompt tokens; 773 and 939
  output tokens. Freeform brief, fixed 1,500 output allowance, temperature zero,
  omitted seed with captured default 4294967295, context 32,768, thinking off,
  Jinja chat, TCP, four observed slots, offload 99, flash attention on.
  Captured build: `b1-c1d0e7a`.
- Corrected native adapter reproduced both outputs byte-for-byte at source
  `456fa8aa9ee5e677997a3c46132c86e583777326`. Receipt
  `native-message-boundary-repair-20261007/result-summary.json`, SHA256
  `8cc43e004e2ec3a0f644a7a2d694b1be55ba6922757c38585bea4a11b497662e`.
  Two calls, normal finishes, no retries. Accepted adapter contract and registry:
  `docs/PR-NATIVE-SUMMARY-PARITY.md` and
  `summary/comparisons/fixtures/native-summary-parity-manifest.json`.

Model SHA256 is
`cd76ec205963b3b33350093e6904d9de16c4e666fd104e1f632d25c7f15f2a13`;
server SHA256 is
`0ca399edd758decd825a71823b04ba7ddbc8b2e10d2309d8bf623ee3c2283099`.
These two inputs establish same-input reproduction and demonstrated capacity.
Their summaries exceed the requested word limit; saved ratings retain omissions
and a modality issue. The lexical scorer is not independent fidelity gold.
Historical harness Git provenance and complete original raw captures remain
incomplete; the preserved source/config bytes and fresh capture receipt are the
binding references. Do not substitute the production inventory revision for a
missing historical harness revision. No safe page ceiling follows from them.

### C9 comparator and current guard/admission regressions

| Packet | Binding and proof scope |
| --- | --- |
| Original C9 | `c9-preproduction-parity-20261007/original-c9/manifest.json`, SHA256 `731b18c54a45f34ada58a8964fd0e599eecfcdd4d3a2ae1e6f15a245b01cca4c`; tracked manifest records original source `a25b22f6c5242c311474681d289d724db0de33a9`. Original 30 cases, ordering, raw requests and results; later controls/adjudication stay distinct. |
| Restored production comparator | `c9-preproduction-parity-20261007/gateway-restored/receipt.json`, SHA256 `54977626b64ee78505f49d26e07b99c2d1e5655bcba8c67fc0eb11bc61e3cd47`; source `eec58548a632ade9666fe4347eaccf639817753c`. 30/30 verdict parity, 29 adjudicated cases plus one unresolved; 111/120 relations and 227/250 passage containment. Not perfect passage accuracy or current full-document qualification. |
| Current saved-response/source measurement | `c9-qualification-readiness-20261016/seams-measurement-report.json`, SHA256 `57ce54ed63953ad08883fb165c1a515b17e35740901bbedf4952f1e310275e5e`. A1 parses Supported then becomes Unsupported at the relation guard. All nine existing A/B claims fail enum/schema admission, despite fitting the scalar cap. DV claim admission is unmeasured. |
| Current SAME60 and test receipt | `c9-qualification-readiness-20261016/seams-final-proof.json`, SHA256 `5e6884714cedf3781bb985abded7af61afcf9be889f1a5b7c2b726889079429e`. Tested implementation `fcf7580bbb0ae90ba56b12cc46731782002f1cb8`; full equality of 60 candidates' 825 ordered verdicts and 31 validations. Private complete `seams-same60.json` SHA256 `ceee845cb539968ed40b87185484496e08f455057040665352559cf96b2e5b53`; preserve all lists/reasons, not counts alone. |

C9 retains joint four-dimension exact-excerpt selection, seed 7, temperature zero,
closed thinking, 32,768 context, 4,096 output tokens. The native owner uses one
slot, offload 999, reasoning off, no warmup and corrected framing; the gateway
has its separately bound deployment/profile/template/settings receipts. These
are not the whole-document runner's seed/slot/transport settings. Preserve the
current pinned profile, tokenizer `qwen35-pre-cc5fb918-v2`, library identities,
framing, samplers and request-seed policy. No runtime-setting experiment here.

## 2. Root cause and every proposed departure

Current app-side differences from the whole-document capacity reference:

- `summary.rs:101`, `generation_input_character_limit_for_context` at `:1257`:
  caps synthesis input at 16,000 characters even when runtime context admits more.
- `summary/coherent.rs:19`, `maximum_summary_units` at `:75` and
  `maximum_initial_summary_units_for_catalog`: hard eight-unit ceiling, with a
  smaller source-count compression heuristic. These are not measured 9B limits.
- `source_catalog_for_profile` at `coherent.rs:7564` and
  `page_balanced_catalog` at `:6674` supply selected quote fragments instead of
  the complete normalized document. The current quote segmentation can also
  omit overlong sentence units (`summary.rs`, `analysis_quote_segments_v13`).
- The writer combines propositions into paragraphs sharing one source list;
  `parse_response_with_limits` at `coherent.rs:6845` makes one claim per unit.
  `materialize_cited_claims` at `summary.rs:3646` preserves those boundaries.
- `comparisons::Prepared::new`/`plan` at `summary/comparisons.rs:147,376` admit
  a union of complete governing sources; one size failure aborts the full plan.
  Synthesis preflight (`summary.rs:2009`, `coherent.rs:3195`) and later delivery
  coverage (`summary.rs:3859`) can replace the whole result with a ledger.
  Connect completion also calls `delivery_claim_prefix_coverage_satisfied`
  (`summary.rs:2864`, `connect/provider.rs:2048`), which checks cited-page
  coverage again. All consumers must use the same versioned admission policy.

Proposed changes are: whole normalized writer input; context-derived admission;
atomic units up to 64; complete citation/source association; section drafting and
one bounded consolidation for over-context input; typed per-claim size holds;
versioned persistence and partial verification notices. All are new behavior
requiring acceptance and proof. The new cited atomic writer is not the old
freeform task; section drafting/consolidation has no preproduction qualification.
No other prompt, schema, sampler, model, runtime, clause-recognition or profile
change is included. Existing shared owners are reused, not copied.

The shortened receipt hash in the previous proposal was this session's error,
introduced at `67cf161`, this document's former line 65. It is corrected above
from the saved file's SHA256; frozen evidence is unchanged. No test is appropriate
for that documentation transcription correction.

## 3. Required change surface: complete input and exact citations

### Whole-document mode

For the new General revision, construct the writer input from every normalized
page and block in canonical order, including all text. Retain page/block/span
identity and structural ancestry. Furniture can be labelled by its existing
owner; it does not authorize deleting source text from this full-input payload.
Show source content once, with a compact exact-ID registry pointing into it;
include complete governing context and parent lead-ins through the existing
`whole_clauses::Clauses` owner. Source text remains untrusted data.

Use `source_catalog_with_furniture_policy`, canonical quote/span identity and
materialization owners for citations. The new revision bypasses page balancing
and relevance selection as input coverage decisions. Citation IDs identify exact
owned quotations, not guessed passages or model-supplied offsets. Do not assign
IDs by text similarity. Keep existing quotation/ownership bounds; expose the
existing omitted/uncitable status instead of silently pretending the registry
covers every fact. Any benchmark fact lacking a usable owned citation blocks
that benchmark and requires an origin amendment; full-text input alone cannot
repair missing citable evidence. Do not rewrite historical source catalogs.

The full input and citation registry must coexist in the actual admitted request.
A fact visible in full text but absent from the registry cannot be delivered with
an unrelated citation. Retain every necessary governing clause for each claim;
source fragments are citation anchors, never permission to shorten Verify context.

At `summary/coherent.rs::prompt_and_schema_for_version` and `budget::admit`,
measure the actual framed request plus schema, IDs, output allowance and existing
reserves against the pinned 32,768 runtime context. Retain the existing budget
owner's preflight semantics; remove the fixed 16,000-character ceiling only for
this General policy, not globally for Story/Contract/legacy paths. A raw document
fitting the context does not imply that its larger cited request fits. No caller
may independently estimate characters, sample source or reduce coverage to pass.

### Atomic claims and scaling

The writer directly emits one proposition per existing `units` entry, with its
own `source_ids`. Actor, action, object, modality, condition, exception, exclusion
and stated relationship stay together. Separate independently checkable facts
semantically, never by a character cut or a later punctuation parser. Several
necessary source clauses remain attached even if their union cannot be verified.
Rust validates boundaries/IDs/counts/ownership and preserves each unit through the
existing materializer; it cannot prove semantic atomicity or completeness.

The new General ceiling is 64 atomic claims, using the existing verification
batch bound as the shared ceiling. It is not a target and not a fixed top-N
selection policy. Actual count follows the necessary propositions in the
independently reviewed coverage specification and grows with document content.
There is no smaller page/source-count ceiling on this new path. Extend the real
schema, parser, budget structure estimator, request planner and persisted count
validation coherently; keep the old eight-claim policy for historical versions
and other profiles. No overflow field or top-eight path is added. Rust rejects
more than 64; it never takes the first 64 or recombines unrelated facts to fit.

If a measured necessary fact inventory exceeds 64, report the exact count, source
binding and cost at the shared claim/batch owner before implementation proceeds
on that case. The proposed remedy is a separately accepted increase at that one
owner, with corresponding schema, budget, persistence and regression proof. Do
not quietly introduce multiple 64-claim plans to evade the cap or offer partial
coverage as the substitute. Whether this limit is reached is currently unmeasured.

## 4. Single over-context design: all sections, then one consolidation

This is a proposed extension beyond the two one-pass baseline inputs.

1. **Source ledger first.** Build an ordered inventory of every normalized span,
   exact citation identity, existing structure node and owning clause/parents.
   `structure.rs::DeterministicStructureInterpreter`, `chunk.rs` and
   `coherent/whole_clauses.rs` own those boundaries. A model does not choose
   which sections enter the plan. Every span is assigned to a draft request;
   duplicated parent/dependency context is marked as context, not new coverage.
2. **Sections admitted by the same budget owner.** Use structure/whole-clause
   boundaries and canonical chunk order as the partition candidates. Pack
   adjacent complete governing units while the actual framed request admits.
   Existing 12,000-character chunks are a traversal aid, not token admission
   proof. Do not invent another heading or number parser. Unrecognized layouts
   retain ordered blocks; they are not omitted. Splitting a governing unit is
   not an implicit fallback: if its complete required context cannot fit even
   alone, record that genuine capacity limit and return for an owner amendment.
3. **Boundary and cross-section facts.** Each section receives its complete
   parent lead-ins and app-owned dependency context. Preserve original global
   source IDs across requests. Relationships requiring clauses in different
   sections must retain both identities and complete texts in a request that
   admits; neither a section title nor another generated summary substitutes
   for them. Existing clause ancestry does not resolve arbitrary non-parent
   cross-references. Unresolved dependencies are explicit planning blockers,
   not permission to assume independence or omit an exception. Any needed
   dependency-reader change belongs at that source owner and requires an
   explicit amendment before implementation, not a downstream phrase rule.
4. **Draft every section.** Section outputs use the same atomic unit/source-ID
   semantics. Persist their request/source bindings and completion status. A
   missing, failed, incomplete or stale section prevents the whole-document
   completion claim. No seed-only retry, skipped section or first-window
   success path is added. The source-coverage ledger retains its denominator.
5. **One bounded consolidation request.** Supply all section candidates in
   canonical order with original citation bindings and complete source context
   needed to reconcile cross-section facts. The consolidator emits the same
   atomic `units` shape; generated section text is drafting input, not evidence.
   Materialize from the original source registry. Deduplicate only exact text
   plus identical complete governing-source sets deterministically; keep
   different conditions, actors, dates, exceptions and source identities.
   Retain lineage from every section candidate to its final claim or exact
   duplicate. No silently dropped candidate or semantic dedup heuristic.
6. **Check final claims.** Check every admitted final claim against its original
   complete governing sources through unchanged C9. Not the section summaries.
   Preserve source order; render supported claims with their original citations.
   Independent whole-draft review still rejects lost conditions or coverage.

The actual consolidation request must fit input, schema and output budgets.
This is a real additional limit: section coverage alone does not prove that all
candidates and required source context fit one merge. Preflight its actual bytes
before its sole invocation; a failure stops, retaining all drafts. No dropping
candidates, repeated compression or unreviewed recursive merge is permitted.
If the bound is hit, record the measured excess at the request/budget owner and
return for an amendment of consolidation there. No claim of unlimited-document
support is made. This path is not ready for implementation until its unresolved
source/dependency and planning proof requirements are reviewed and accepted.

## 5. Per-claim admission, persistence and delivery

Extend the actual `Prepared::new`/`plan` owners with typed capacity outcomes:
ordered entries are admitted requests or `AdmissionWithheld`, retaining exact
claim/evidence identity, reason and measured bound. Apply it to all size reasons:
single governing clause, combined scalars, enum, schema and deterministic request
context. Do not add a second scanner. Invalid source ownership, unsupported
protocol, parser/infrastructure errors and cancellation remain errors. A runtime
preflight failure after a supposedly admitted plan remains a finding, not a
quiet size withhold. Preflight all admitted requests before generation.

Consume the plan at synthesis preflight, Verify, both coverage-fallback
consumers and Connect completion. Propagate its typed coverage outcome through
saved artifact construction; do not re-parse warning prose at the provider.
Held claims consume no comparison or trim-contribution generation.
Merge results by exact identity in original order; semantic guards cannot promote
held entries. Add optional typed `admission_withhold` metadata to
`contracts.rs::ClaimVerification`. Use Ambiguous only as the existing enum's
storage carrier; report these separately as not verified, not model ambiguity.
A record combined with Supported or Unsupported fails validation.

Version the new General writer/verification policy together without blindly
bumping other profiles. Bind saved outcomes to sources, limits and policy; reload
through `validate_coherent_verified_document` and the same deterministic admission
owner, without generation/startup. Check missing/duplicate/tampered metadata and
source bindings; old artifacts keep their old behavior. Exercise the actual DB
serializers. No database migration or public wire-shape change is authorized.

For a mixed result, keep supported siblings and a bounded notice such as
“Claim 2: too long to verify; omitted from this summary.” Preserve all original
coverage and omission disclosures. Derive notices from typed outcomes. Their
verification loss must not trigger a whole-summary ledger replacement or a new
re-synthesis, but it also cannot excuse an unrelated missing section or lost fact.
No Supported claims means the existing `NO_SEMANTICALLY_SUPPORTED_CLAIMS` completion
failure, with diagnostics retained. A notice is not verified substantive prose.
Prove desktop and actual Connect completion agree on that distinction. Preserve
the delivery byte/warning bounds; a delivery prefix must not drop additional
Supported claims and still claim this policy's complete supported result. Record
any actual output-bound failure at its existing delivery owner for an amendment.

## 6. Limits, costs, assumptions and explicit non-scope

| Boundary | Treatment |
| --- | --- |
| General writer input | Remove fixed 16,000-character cap; actual framed context/output admission at 32,768 through existing budget owner |
| General atomic claims | Scale by necessary content, maximum 64; no top-eight selection or silent prefix |
| Other unit bounds | Existing 1,200-character maximum and eight sources per unit; actual admitted schema may be smaller under the existing budget owner, never permission to clip meaning |
| C9 | Unchanged 4,096 combined scalars, 8,192 values per enum, exact excerpts up to 240 scalars, four selections per side/dimension, joint response |
| Schema bytes | Existing native comparison ceiling 1,048,576 and gateway 250,000; writer's existing non-comparison schema cap also stays |
| Runtime | Existing pinned weights/server/libraries/tokenizer/framing/sampling and output-limit rejection; no new profile or setting |
| Batch bound | 64 final comparison claims; preserve actual callers' existing batch gates; no claim that 64 bounds the entire pipeline |

Let S be the actually admitted section count, N final atomic claims and H
admission-held claims. C9 completions = N-H, with N<=64. Synthesis completions
are one when the whole cited input admits, otherwise S+1 including the sole
consolidation. Thus this component's completions are 1+(N-H) or S+1+(N-H).
For illustration only: 4 sections and 24 admitted final claims cost 29 calls;
10 sections and 64 admitted final claims cost 75 calls. These are arithmetic
examples, not observed counts or authorized budgets. Analyze, ledger checks,
existing repair paths and contribution checks are separate; enumerate them too.

Latency is the sum of all serialized draft, consolidation and Verify durations
plus planning/preflight/tokenization; it grows with both document tokens and
claim count. Do not extrapolate a fixed seconds-per-call from another prompt.
Future receipts must report each request's input/output tokens, allowance,
preflights, completions and observed latency, including failed/rejected work.
Before live work, a separate contract freezes the exact finite budget, requests,
actual before-send gates, runtime identity and no-retry stop rules. Budget-owner
preflight searches count as preflights, separately from model completions.
This revision grants zero live calls and no GPU use.

Assumptions/blockers: exact source IDs must cover the benchmark facts; arbitrary
cross-references, indivisible over-context governing units, necessary counts above
64 and merge capacity have not been proven. Do not hide these behind page counts
or a fallback. Freeze and review the required dependency/coverage policy before
code, and amend at its origin if the current owner cannot represent it.

No new model/runtime/settings experiment, O3 work, #129/#131 fix, scan/OCR or
#133 implementation, profile retirement, installed app, production activation,
public DTO/dependency change, merge or hold release. DV labels remain frozen and
unopened; no generated DV claims or substitutions. DV09/DV11 still block their
four unseen cases. The unseen protocol's live run remains unauthorized.

## 7. Verification plan: reuse proof and stop on gaps

Before behavior code: operator acceptance of the revised complete design,
including the unresolved dependencies/capacity boundaries; acceptance-record
commit; independent source-derived benchmark fact inventory. Then implement
reproduce → isolate → explain → fix → prove → prevent regression at the actual
owners. Group own-code findings by concept and introducing commit before edits.

1. Public fail-first controls at the writer/catalog/budget owners: material
   facts beyond the former eighth position; middle-page citation excluded by
   page balancing; full input beyond 16,000 characters that fits real context;
   facts needing several sources, parents, conditions, modality and exceptions.
   Faithful controls and unfaithful twins use real parsers/materializers and
   validators. An input fitting text alone but failing full framed admission
   must not be falsely marked admitted. No second budgeting implementation.
2. Full-section offline planning proof through the actual proposed shared
   planner: pin the public long fixture and source artifacts; union of primary
   assigned spans equals the full normalized inventory, with canonical order,
   no skipped section, exact labelled context duplication, correct dependencies
   and token/schema/output admission for every planned draft. If the DOL fixture
   fits as a whole, also freeze an equivalent genuinely over-context public
   input; page count is not the test. Bound consolidation capacity with frozen
   candidate responses before any separately accepted live invocation. Missing
   owner seams require acceptance before adding code, not a Python substitute.
3. Save/reload section progress and all-fit/mixed/all-held Verify outcomes.
   Exercise actual completion, coverage and desktop/Connect notice owners.
   Probe 0/1/8/9/63/64/65 claims, every size boundary, Unicode scalar versus
   byte counts, duplicate sources, mixed invalid entries, stale/partial records,
   ordering, cancellation and infrastructure errors. No downgrade to a warning
   for a protocol or ownership defect. All-held is not successful completion.
4. Mutations must kill restoration of the eight-unit or 16,000-character
   restrictions, source/page sampling, skipped section/dependency, false coverage
   denominator, dropped candidate, changed citation binding, suppressed notice,
   whole-plan size failure, either admission-caused whole-summary fallback,
   a mismatched Connect coverage policy, held-claim promotion or ignored saved
   metadata. Mutated bodies must match
   the published tested source, with before/after proof at the owner.
5. Reuse current offline seams for existing A/B claims and permitted DV source
   inventory. Old A/B failures remain seven enum and two schema rejections;
   changing their policy representation is not proof that they fit. No saved
   new DV claims exist; their claim admission remains unmeasured. After a
   separately accepted writer run, freeze its actual A/B outputs and remeasure
   actual claims/source sets through the same owner. No synthetic splits counted
   as model outputs. Whole-document/citation coverage is an additional gate.
6. Preserve original C9 30-case protocol, actual parser/guard replays and SAME60
   complete 825 ordered verdict lists/reasons and 31 saved validations unchanged.
   Expected changes on those frozen inputs: none. Historical policies remain
   versioned; do not edit old fixtures to accommodate the new writer. Run required
   Rust suite, ordinary-build gate, format and strict Clippy after accepted code.

The two whole-contract preproduction inputs and their saved summaries are the
coverage benchmark. An independent reviewer freezes necessary facts from both
summaries AND original sources, correcting their recorded omissions/modality
rather than enshrining old errors as gold. New output must cover that inventory,
retain all material qualifications, and deliver no unsupported claim. Independent
whole-draft review precedes reliance on any Verify result. No prompt tuning,
label change or replacement run to obtain acceptance. New task/request differences
are listed and judged; a changed JSON writer cannot claim byte-equal freeform
parity. Existing same-input native adapter proof remains intact.

## 8. Current evidence, implementation summary and cold diff audit

This revision changes this contract only. No behavior/tests/configuration,
fixtures, dependencies, inference or GPU use. Code evidence remains bound to
`fcf7580`; do not rerun code suites for this documentation revision.

A new read-only inspection of the retained public-long fixture's saved artifacts
found 111 normalized pages/blocks, 28,033 text scalars and three saved chunks.
The saved chunk block sequence covers the normalized inventory exactly. This is
existing chunk provenance, NOT the proposed writer's section/token/merge proof.
Alias `c9-qualification-readiness-20261016/whole-document-contract-source-inventory.json`,
SHA256 `38b981ac577a86d47404bf5a818d978fb21052db4df28d71bfa2e4bd65bc5810`.
The new planner does not exist; its actual request admission is **NOT MEASURED**.
Neither page count nor these saved chunks prove the fixture exceeds context.

Cold diff audit: this file replaces the superseded eight-claim proposal with the
verified whole-document inventory, selected direction, single section/consolidation
design, source/dependency requirements, capacity limits, cost and proof plan. The
receipt transcription is corrected from the actual artifact. Prove no relevant
source diff and no private text before one publication push; retain evidence in
the durable namespace. All independent source/fidelity/planning gaps are explicit.

## 9. Gap audit and stop

**NOT DONE.** Revised design acceptance, independent benchmark fact inventory,
complete source-ID/dependency coverage, actual full-section/token/consolidation
planning proof, implementation/regressions/mutations, new writer fidelity,
separate live budget, unseen qualification and operator hold release remain.
No claim that the existing saved-chunk inspection satisfies new-planner proof.
Publish the contract-only revision, notify oversight and stop for the operator.
No automatic build, live run, merge, ready transition or hold-thread edit.


## Recorded operator acceptance — offline Phase B

ACCEPTED: operator answer “Accept (Recommended)”, relayed verbatim by oversight in
[PR132 comment 6225524659](https://github.com/canfieldjuan/document-summarizer/pull/132#issuecomment-6225524659),
and confirmed directly in this session: “i accept lets check oversight”.
Accepted source revision: `ef2eb6c70155431e8a41ff9c93d191ba7034f6f5`; exact contract
SHA256 `c368fd75c07ec680a869774286ea1b41e3e48bff98d8401eee0554da07282803`.
The proposal status and stop above record the earlier publication state. This
acceptance supersedes that pre-implementation stop only, retaining every scope,
proof, origin-amendment requirement and later live/production/merge hold.

Authorized sequence: this acceptance-record commit first, then test-first offline
implementation under section 7; one push per round, exact-head review, then STOP.
The source/dependency/capacity blockers still require measurement and an amendment
at their owner when the accepted design cannot represent them. No weaker fallback.
No live writer/verifier calls or GPU workload; a separate exact budget is required.

Oversight owns the independent benchmark facts. Their frozen approval is relayed
in [PR132 comment 6226397989](https://github.com/canfieldjuan/document-summarizer/pull/132#issuecomment-6226397989).
Private alias `docsum-preproduction-benchmark-20261016/frozen-20261016/benchmark-facts-frozen.json`;
SHA256 `8b9817a9a425647dc63f2968a56c5962ba19e95109c1f3e5ab7c66f8a2f75cbf`.
This is a received reference, not a local file inspection. Do not open, derive,
modify or tune against it during the offline build. Bind the received hash in the
later live-benchmark contract and consume it only in that accepted run. The DV
labels and unseen cases retain their separate freeze and holds.
