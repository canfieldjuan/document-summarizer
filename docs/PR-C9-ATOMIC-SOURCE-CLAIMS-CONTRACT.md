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


## Accepted source-dependency amendment — oversight text-match pending

The operator directly accepted the source-owner amendment in this session:
“Accept the source-owner amendment”. Oversight also relayed the operator's
“Accept (Recommended)” with the additional forms and external-reference policy in
[PR132 comment 6226652468](https://github.com/canfieldjuan/document-summarizer/pull/132#issuecomment-6226652468).
This revision records that combined scope. It supersedes the narrower proposed
amendment at `79d9a21`, which would have blocked valid inputs by classifying
Article/list/external references as unsupported. That was a contract-design gap
in this PR, not a newly introduced reader regression.

The accepted section 4.3 requires this amendment before a dependency-reader
change. Oversight must confirm that this revised text matches the accepted
scope before reader implementation. All later live and production holds remain.

### Reproduce, isolate and explain

A public three-clause fixture has clause 1: “The operator may use the equipment
subject to Section 3.” Clause 3 says: “This permission excludes exterior work.”
Through the existing General `Clauses::new` / `segment` / `context` owners, clause
1's returned context contains only its own permission and the reference, not the
referenced exception. The declared probe
`whole_document_cross_section_dependency_preserves_governing_exception` fails
at that missing exception; it makes no model call. This is a source-context probe,
not a claim that a completed new section planner or a live model failed.

First missing ownership: `coherent/whole_clauses.rs`, context construction at
lines 96–104, introduced by `c913fd2a`. It builds from numbered parents plus
the current clause. No non-parent dependency graph exists in that reader or the
structure owner. This predates this PR. Neither a larger writer cap nor a later
word-matching exception guard repairs that missing relationship.

### Required change surface and single owner

Add a **versioned source-dependency projection at this existing clause owner**
for the new General revision. Keep historical General contexts, Contract's full
clause list, heading grammar and extraction categories unchanged.

- One resolver handles explicit `Section N`, `Sections N`, `§ N`, `Article N`,
  enumerated lists (`N and N`; `N, N and N`) and ranges (`N through N`,
  hyphen or en dash). Each
  list/range member resolves to its own owned target. Reuse the existing decimal
  and Article identity parsers and source-heading ownership; do not add another
  number parser or infer a dependency by shared words, actors or model output.
  Headings, including a clause-opening `§ N`, are not references. The existing
  heading owner distinguishes heading spans from references in clause content.
- Return app-owned clause IDs, exact source fragments, parent IDs, dependency IDs
  and typed `resolved`, `external` or `unresolved` status with reasons. Build
  stable identities from original document order and source spans. Preserve
  exact source bytes and the exact reference text, including its document
  qualifier. Resolve within the stated document/Article scope, never by picking
  a convenient duplicate target.
- Compute the transitive referenced-clause/parent closure once at that owner.
  Deduplicate by owned identity with a visited set, including finite cycles;
  preserve canonical source order. Retain complete governing exceptions and
  parent context. A missing or duplicate in-document target, unclear boundary,
  or other genuinely unresolvable in-document reference is `unresolved` and
  blocks the affected plan. An explicit in-document reference form the resolver
  does not support (for example paragraph or subsection (b) pointing inside the
  supplied document) is `unresolved` with reason
  `unsupported_form` and blocks the affected plan. It is never silently treated
  as independent. This does not claim to solve arbitrary semantic references
  in prose.
- A reference qualified to a document not supplied, or an absent annex under the
  accepted annex rule below, is `external`, not an internal missing-target error. Preserve its exact text and document identity; do not
  attach a same-number clause from this document, invent external source IDs,
  or fetch the absent document. Keep the reference with its governing unit and
  disclose it in source coverage as “text not in this document”. This status is
  non-blocking. A claim may report that an obligation follows the external
  document; it may not assert that absent document's content. Naming a reference
  does not supply evidence for its content. This distinction is a mandatory
  faithful/unsupported controlled twin in the proof plan, not a new downstream
  phrase filter or a claim of general deterministic semantic completeness.
- The section planner and new-revision Verify source projection consume this
  same metadata. No downstream text re-scan. A resolved dependency in another
  section travels with its governing unit. Complete text and parent context
  stay present even when size admission then fails. Existing per-claim comparison
  bounds and typed size withholding remain unchanged.
- An `unresolved` dependency blocks before generation. Measure/report it for an
  owner repair; do not skip the clause, cut context or claim complete coverage.
  Whole-document input does not make a guessed dependency acceptable. External
  status cannot conceal an ambiguous internal reference.

### Accepted annex-reference amendment — local text-match review required

Oversight relayed the operator decision verbatim in mailbox `52f39bc6`:
“Accept (Recommended)”. The chosen option was: “Doc Sum adds
attachment/appendix/exhibit/schedule references under that rule, with tests,
offline. Everything else stays held.” This is the acceptance record before any
annex implementation; mail this contract-only local commit for worktree match
review. It travels with the round's single later push, not a separate push.

At the same source/heading/resolver owner, recognize attachment, appendix,
exhibit and schedule references with their exact labels. A reference resolves
only to a single in-document annex heading carrying that exact label. The
resolved annex's full governing text, parents and dependencies travel in the
existing source-owned closure. Duplicate labels or an unclear annex boundary
remain `unresolved` and block. Do not select one duplicate or suppress uncertainty.

When no matching annex is supplied, return typed `external` status for an absent
annex. Preserve the exact reference and label with its governing unit, disclose
“attachment not included in this document”, and do not block on absence alone.
The summary may name that obligation/reference; it must never assert the absent
annex's content. No fetch, invented source, guessed annex or same-label attachment
from outside the supplied document. This explicit accepted exception supersedes
T1's earlier unsupported-form treatment for these four annex kinds only.

Public synthetic twins must cover a unique included annex, an absent annex
(non-blocking, faithful reference versus unsupported absent-content assertion),
duplicate labels, and an unclear annex boundary. Preserve exact spans, source
order, closures, the unsupported status of other unsupported internal forms,
and unchanged historical General/Contract outputs. The heading interpretation
change is versioned at its owner; it is not a downstream annex re-scan.

All already accepted scanner/qualifier/range and heading-origin repairs remain
required, with fail-first public twins. Re-measure both original inputs through
the actual owner, zero generation. Any unresolved reference or boundary still
blocks dependent work; report reasons without tuning the count toward zero.
All freeze, private-evidence, live-budget, production, merge and ready holds stay.

### Accepted foreign-numbering amendment — contract before code

Oversight relayed the operator's decision verbatim on R4: “Accept (Recommended)”.
The selected option was: “Doc Sum adds the foreign-numbering rule at the same
resolver, with tests, offline; then the reference gate clears and it moves on to
the summary-planning work. All other holds stay.” This replaces the proposed
exact-token inheritance rule; no inheritance rule is accepted or implemented.
Record this in a contract-only local commit and mail its hash before code.

The source heading owner supplies the numbering shapes of the document's own
resolved heading identities. At the same existing resolver, a section number
whose numbering grammar is absent from that owned set is typed `external` with
reason `foreign_numbering`: keep the exact reference with its governing unit,
disclose “text not in this document”, and do not assert the absent text's content.
Do not guess a law's name or use a statute/abbreviation lexicon. A reference that
is compatible with the owned heading grammar but lacks its exact target stays
`unresolved` with reason `missing_target` and blocks.

Extend the shared number/heading identity owner for the new version to expose
dashed as well as dotted heading identities; the resolver and heading reader
consume the same parse and owned shape metadata. This is not another independent
number parser or a later source re-scan. Preserve the historical General and
Contract grammars and outputs. Treat deeper dotted descendants conservatively
as compatible, rather than declaring a missing internal depth foreign. Derive
dash placement from actual owned identities; formatting glyphs do not identify
a statute. Do not make every failed lookup external.

Explicit preposed/suffix document qualifiers take precedence and preserve their
namespace. A complete owned dashed identity resolves internally. A supported
hyphen/en-dash range of owned heading identities remains an internal range;
explicit plural/range syntax with a missing endpoint stays blocking. Where a
singular dash form could still refer to an owned range endpoint, retain an
explicit unresolved ambiguity instead of guessing foreign scope. Only the
foreign-numbering case authorized above becomes non-blocking. Duplicate targets
and genuinely uncertain boundaries remain blocking.

Public synthetic twins: dashed foreign citation in a dotted-numbered document;
missing dotted `Section 9.4` in a dotted-numbered document; present and missing
dashed numbers in a dashed-numbered document; valid internal ranges and missing
range endpoints; and exact reference/content-absence ownership controls. The
new foreign-numbering exception supersedes the earlier unqualified dashed
citation's unconditional missing-target expectation only when its numbering
grammar is foreign. Other unsupported internal forms stay unsupported.

Prove a mutant that classifies every missing target as external fails the dotted
missing-target twin. Re-measure both original source inputs with zero generation,
without benchmark/DV labels. Continue dependent planner/writer/Verify work only
if both actual source plans have zero blocking unresolved references; otherwise
STOP and report. This acceptance does not promise a measured zero in advance or
release any live, production, merge, ready or freeze hold. One round push and
exact-head review remain required after the complete accepted offline build.

### Proof, compatibility and measurement gates

Keep the current public failing fixture and exercise the new versioned owner
without changing the historical context policy. Add multi-hop/parent/cycle,
missing/duplicate-target, ambiguous boundary, Article, lists and ranges, heading
versus reference, and external-reference controls. Cover mixed resolved/external/
unresolved dependencies in one governing unit. A different referenced clause
must not acquire clause 3's exception. The faithful permission-plus-exclusion
case retains both sources; its omitted-exclusion twin must remain unsupported.
For the external twin, the claim naming the external obligation/reference passes
its evidence controls; the claim asserting absent external content fails. Do not
invent a benchmark label or infer that citation ownership alone proves fidelity.

Mutation of a dependency edge, unresolved status, heading classification or
external/internal distinction must fail the corresponding public regression.
The planner consumes the projection and retains complete source coverage without
copying the resolver. Re-run unchanged historical C9 and SAME60 full verdict
lists after implementation; expected changes on those frozen inputs: none.
No new model prompt, comparator, cap, model/runtime/settings, labels or live budget.

The accepted review requires an offline, zero-generation measurement through
the actual source owner that both original frozen pre-production source inputs
plan with no blocking unresolved references. Oversight clarified in mailbox
`1d09f424` that this follows the test-first resolver implementation and precedes
dependent planner, writer and Verify-consumer work. A manual inventory or a
second parser is not executable planning proof.

The private detailed report retains exact reference text, source bindings and
fragments, status counts, excluded heading occurrences and every blocking reason.
Store it under the private heldout evidence directory with the required private
permissions. On public GitHub, publish only input aliases, counts and reason
codes, plus the SHA256 of that private report. Use source inputs only, never the
independently frozen benchmark fact labels or DV labels. Counts are observations,
not targets to tune for: any genuine ambiguity is reported and blocks; never
force it to zero. If both inputs have zero blocking `unresolved`, mail the counts
and continue without waiting; otherwise STOP and report each reason for an owner
repair before dependent work.

### Current implementation and clarified sequence

The new-protocol schema and budget capacity controls have fail-before/pass-after
logs. They are preliminary local work, not a completed writer or pipeline fix.
Parser, persistence, planning and delivery work remain incomplete. The dependency
probe is intentionally still failing and unpublished; do not weaken or skip it.
No reader fix has been made. No behavior code from this work is published yet.

**NOT DONE:** the resolver and its actual two-input measurement remain unbuilt
and unmeasured. Oversight's text-match review of `5c70289` in mailbox `1d09f424`
requires the unsupported-form and privacy tightenings above in one local
contract-only commit, reviewed in this worktree without a separate push. Send
that hash for review before reader code. Then: source-owner fail-first public
regressions; resolver and transitive closure; actual source-only two-input
measurement; only after its no-blocking gate, dependent consumers and unchanged
historical C9/SAME60 replays; one push, exact-head review, STOP. Do not silently
substitute a parser, weaken a probe or describe a planned test as passed.

The previously accepted benchmark and live-run freezes, all production/merge
holds, and the source-owner amendment requirement remain binding. This contract
match is not an exact-head PASS or permission to start any live run.


## Accepted offline native-preflight test seam (2026-10-17)

Oversight relayed the operator decision verbatim: **"Accept (Recommended)"**.
The accepted option was:

> Doc Sum's contract says a missing test seam needs your acceptance before code. It wants to prove, offline, that each planned section of a whole document fits the 9B's 32,768-token window, using the app's real admission code (PromptFraming::load and LlamaCppRuntime::preflight_request). That code asks the model server to count tokens, and the server can't run under the current holds (no GPU, no startup). Proposal: a test-only stand-in for the server's token counter inside the existing framing tests. It uses the pinned tokenizer for document text, and recorded real-server answers for the fixed prompt framing. Accept it with these conditions?
> (1) Test code only, with no production runtime or setting change.
> (2) The context comes from the qualified 9B profile, not a typed number.
> (3) Before any capacity claim, the stand-in must reproduce every recorded real-server token count exactly, and any mismatch stops the work.
> (4) It refuses any count request it can't attribute, rather than guessing.
> (5) The results are labelled offline capacity evidence, not native qualification; the later accepted live run is still required.

The source owner for context is the qualified `qwen35-9b-q4km-v1` profile in
`pipeline/model_settings.rs`. The test adapter must read that profile, including
its tokenizer family and thinking setting. It may exercise the existing native
framing loader and admission method with a loopback fixture transport; it must
never start a model server, generate text, use the GPU, duplicate framing logic,
or treat synthetic character-count routing tests as native token proof.

Before a section-capacity assertion, freeze the tokenizer metadata and the
complete recorded token-count inventory used to validate the adapter. Match
all recorded real-server counts exactly through the real owner. An unattributed
request, absent receipt or count mismatch stops this proof; retain the failure
and report it without guessing or tuning. All benchmark-label, live-generation,
production, readiness, merge and PR116 holds remain unchanged.


## Accepted quote-owner amendment: contextual citation anchors

Operator acceptance was relayed verbatim by oversight in mailbox `128db446`
on 2026-10-17: Juan selected **Accept (Recommended)** for the proposal at
local contract-only commit `33fc0f8`, with the delivery condition below.
This authorizes the source-owner repair and its offline proof. The dependent
build remains stopped until oversight independently rechecks every required
fact against the new anchor inventory. No live budget or hold changes.

### Accepted delivery condition: partial excerpts

A displayed anchor that does not cover its whole governing sentence must be
labelled **partial excerpt**. The label is derived from range metadata created
by the quote owner and consumed at the delivery/presentation owner, never by
searching the source text. It does not alter source bytes and must not be
applied to an anchor that covers the whole sentence. Public controls must prove
both directions: a fragment is labelled, and a complete anchor is not.
Persisted/reloaded citations must preserve and validate that distinction.

Acceptance and this additional condition are recorded in a contract-only
commit before behavior implementation. All other scope boundaries remain.

### Root cause and reproduced evidence

The new, uncommitted `coherent/whole_document.rs::SourceInput::new` reused
`quote_segments::segment_spans`, the current standalone analysis-quotation
policy, as its full-document citation inventory. This was my porting assumption.
The existing quote owner groups sentences, qualifications and form fields, then
omits incomplete or over-limit units. That remains intentional for standalone
quotation evidence. Full source bytes in the writer input do not repair the
missing citation identities.

The public `whole_writer_keeps_uncitable_text_and_reports_existing_quote_omission`
control reproduces this policy on a long sentence between citable siblings.
Code controls the omission in `quote_segments.rs::segment_spans` at its
`!unit.complete || ... > MAX_ANALYSIS_QUOTE_CHARACTERS` packing branch.
The separate substring-offset defect in my adapter has already been reproduced,
repaired by exposing the quote owner's ranges, and caught under mutation; it
must not be reintroduced as part of this amendment.

Oversight's count-only review `aee2d213` found, on corrected capacity v2:

| Input alias | Required facts without any citable quote | Partly citable facts | Governing units with zero citations |
| --- | --- | --- | --- |
| preproduction-1 | 6 of 37 | 4 | 6 of 191 |
| preproduction-2 | 10 of 30 | 3 | 7 of 65 |

These are independently reported counts; this implementation session has not
opened the frozen benchmark fact text or labels. The review used a lenient
any-overlap rule, so the blocker counts are lower bounds. All normalized source
bytes were present. The current adapter's 26/15 counters are **omitted quote
segments**, not whole governing units; reporting must distinguish them.

Evidence: corrected private draft-capacity report alias `source-writer-capacity-v2`,
SHA256 `3c582e82476e641dec07a9eabce27e391e554a5d53765747e7c7c92328a06dfc`.
The exact executed source receipt is
`1aa395dac001894c35d26b167fe446a1fc7953170ff15299f3c94b71d91ab4b3`.
Original native-count inventory remains
`4acafb6e49cc954d9f586f1020e44f7c4b35210ff69f29e780d8a6a6b8c9b9d2`;
all original recorded counts match. This is a source-representation gap, with
no new generation or evidence against the proven 9B C9 behavior.

### Proposed required change surface

1. Add an explicit contextual-anchor policy at the **existing quote owner**,
   selected only by the new General writer revision. Keep the current standalone
   policy and every historical caller unchanged. Do not call the standalone
   policy first and patch its omissions in the writer.
2. Share the existing bounded range-building implementation and its preferred
   boundary helper (`analysis_quote_segments_v12` /
   `preferred_analysis_quote_boundary_v12`) through the quote owner. Expose exact
   ranges at creation; preserve historical string outputs. This is reuse of an
   existing implementation, **not** prior qualification of the proposed role.
   Do not copy a second cutting loop into `whole_document.rs` or infer positions
   by searching text.
3. Contextual anchors cover every non-whitespace scalar of each owned primary
   source span, in order. Each anchor stays inside its normalized block and
   owning governing unit, uses the existing 600-scalar quotation ceiling, and
   retains original UTF-8 byte boundaries and bytes. Prefer the existing bounded
   punctuation/whitespace cuts; where none exists, the scalar boundary is an
   explicit citation cut. No source normalization, semantic claim splitting or
   source omission is permitted.
4. Eligibility requires the app-owned complete governing unit, parents and
   resolved dependency closure. Bind each anchor's exact range and evidence ID
   to that owner. Uncertain/unresolved ownership still blocks at its source.
   Generated IDs, guesses, neighbouring clauses and external text cannot supply
   missing context. The writer consumes these owned ranges directly.
5. A contextual anchor may be a sentence fragment, split a long word, or end
   inside a qualification. **It is never standalone evidence.** The new Verify
   and saved-evidence owners must reconstruct and validate the same complete
   governing source for every selected anchor. Missing or mismatched context is
   an error, with no quote-only fallback. Keep source order, complete conditions
   and exclusions, existing scalar/schema/enum limits and typed size withholding.
   An anchor cut does not authorize shortening a claim or its Verify context.
6. Keep diagnostics separate: omitted standalone quote segments; contextual
   anchor coverage gaps; governing units with zero anchors; and independently
   reviewed required-fact citation coverage. Never describe any of these as a
   fidelity pass or an atomic-claim count.

Remove the new writer's direct reuse of standalone quote admission and its
substring-recovery workaround (already removed); do not stack an omission-filling
scanner or per-layout exception on top. The standalone qualification/form rules
remain necessary for their historical evidence role and are retained there.

### Departures, trade-off and explicit non-scope

This is new, unqualified citation behavior. Original C9 already interprets exact
excerpts within complete governing source; it does not prove this new writer or
anchor inventory. The deliberate trade-off is that the visible quote may be
incomplete, while its full app-owned governing source remains mandatory for
verification. This proposal explicitly supersedes the new writer's current
standalone-quotation restriction only for contextual anchors.

Request differences are the anchor IDs/ranges, schema ID vocabulary and any
resulting draft packing/output allowance; the new writer's input description
must identify the contextual-anchor role. That description change is limited
to this representation. Record actual differences and remeasure the final
requests. No C9 prompt/decoder protocol, old writer prompt, native framing,
thinking, sampler, model/profile/context, quotation length cap, heading/reference
reader, comparison limits, source text, benchmark labels or production gate
changes. No scan/OCR, migration, public DTO, fallback weakening or new live call.
The accepted ordinary-build and PR116 holds remain.

### Verification plan and stop rules

- Before implementation: oversight review and explicit operator acceptance of
  this amendment. Then public fail-first source-owner controls: an over-limit
  sentence, a long qualification group, a multi-field form, an unterminated tail,
  repeated text after an omitted unit, and mixed citable/uncitable text. Show
  failure under the current contextual inventory, not a renamed passing test.
- Probe ceiling-minus-one, ceiling and ceiling-plus-one; whitespace/empty input;
  unbroken words; decomposed Unicode; source-unit/block boundaries and repeated
  identical text with different owners. Range union must cover every original
  non-whitespace scalar exactly once without inventing or changing bytes.
- At the actual new source/Verify/saved validators, prove complete context is
  retained for a valid fragment, and reject absent context, an unrelated owner,
  changed bytes/IDs, unresolved dependencies and tampered/reloaded bindings.
  Do not claim these syntactic tests prove semantic fidelity.
- Mutations must catch restoring standalone omission in the contextual path,
  skipping an anchor, substring position recovery, changing a governing owner,
  or permitting quote-only fallback. Restore and verify candidate hashes.
- Reuse the accepted offline native-count seam, actual budget/planner, both
  frozen source inputs and the genuinely over-context public control. Freeze
  new receipts. Report every draft's exact token/schema/output bounds and source
  coverage; stop on a genuine indivisible-unit or consolidation capacity limit.
- Oversight independently rechecks all required source quotes and qualifications
  against the owned anchor/context inventory without exposing frozen labels to
  this session. Any remaining missing/mismatched ownership blocks; do not tune
  toward a count. The full-draft/fidelity gate is still separate.
- Preserve historical quote outputs, C9 and SAME60 complete verdict lists and
  validations unchanged. Run the already-required final gates once the complete
  accepted implementation exists. No generation is authorized by this proposal.

### Gap audit

**NOT DONE:** proposal only. No contextual-anchor implementation, benchmark
coverage pass, consolidation, section-progress storage, typed Verify/delivery,
full gate run or final exact-head review is claimed. Approval of the earlier
source-dependency rule or count seam does not accept this amendment.


## PROPOSED consolidation-owner amendment after measured STOP

**NOT ACCEPTED. No behavior change is authorized by this proposal.**
The dependent offline build stopped under section 4's explicit consolidation
capacity rule. This is a measured request limit, not a model-fidelity result.

### Evidence and origin

The same public over-context control used for draft admission produces two
admitted section requests. Forty authored public atomic candidates were frozen
with their exact request/source bindings before consolidation admission. These
are synthetic size controls, not generated responses or benchmark labels.

The actual `SourceInput::consolidation` request, admitted through `budget::admit`
and the accepted native-count seam, has **48,634 input tokens** against the
unchanged **32,768-token** profile. Input alone exceeds the context by 15,866,
before any output. It returned `WHOLE_DOCUMENT_CONSOLIDATION_TOO_LARGE`. No live
model calls or server starts occurred. No candidates or governing source were
removed to obtain a pass.

Durable public aliases under `whole-document-build-20261016`:

- Consolidation boundary measurement: SHA256
  `f196a2e4081f278781b9d50a395dc62aa917c4d82c2b0722f1515819411c9f6c`.
- Frozen public section responses: SHA256
  `71e6cdc9ea6c8284e4ca2f422db9df930eac25695f6958ccc8ce1db7a81c6c1a`.
- Executed source receipt: SHA256
  `52677a7d63535ed7b7ea1ed10a6b48e9944f31d28f327ab95ce9e40fb12d555b`.
- Stop receipt: SHA256
  `0a74e3788ccbf4f060d856536871cffaf381ae9cfa6136c0ba5ae35594c26c91`.

The origin is the single consolidation request's source projection: it brings
all selected candidates' complete governing sources together again. Separately,
the candidate implementation deliberately establishes lineage only by exact
text and original citation identities; its parser rejects rewrites, missing
candidates and additions. This avoids guessing which rewritten claim descends
from which section candidate. The model step therefore only copies an already
known canonical candidate inventory. This is a conservative implementation
choice, not a previously qualified consolidation model behavior.

### Proposed required change surface

Replace that model-copy step with deterministic assembly at the same owner:
validate every completed section against its frozen request/catalog; retain all
candidates in section order; deduplicate only exact text, identical excerpt IDs
and identical complete governing-source sets; retain every candidate's lineage;
materialize final claims from the original global source registry. Keep wording
variants and different bindings separate. If the distinct final inventory
exceeds the accepted claim ceiling, stop without selecting a subset.

Remove the unpublished consolidation-only model request, its admission branch
and response parser when this amendment is accepted. Preserve their failed
measurement and frozen bytes as evidence; do not keep a redundant model call,
source truncation, recursive merge or a new fallback alongside the assembler.

Every final claim still receives its original complete governing context at
C9. Section progress must still be persisted/reloaded with request/source and
completion bindings. Missing, failed, stale or cancelled sections prevent
completion. Typed per-claim admission, partial-excerpt labels, complete supported
result delivery, and the independent whole-draft fidelity gate remain required.

### Non-scope, proof and authority

This explicitly changes section 4's one-model-consolidation requirement to zero
consolidation model calls; it needs oversight review and operator acceptance
before code. Section writer and C9 prompts, decoder protocols, runtime, model,
framing, thinking, context and limits do not change. All-fit behavior, historical
artifacts, frozen facts/labels, live budgets and all production/merge/ready holds
remain unchanged. No safe page ceiling or new fidelity claim follows.

After acceptance, replay the frozen public candidate packet through the actual
assembler with zero generation. Prove complete lineage, exact bytes/bindings,
no consolidation dispatch, missing/stale/mixed invalid section rejection,
0/1/8/9/63/64/65 candidate boundaries, exact duplicates versus wording/source
variants, and real DB reload. Reuse the original rejected request as the capacity
regression; do not reframe it as an admitted model request. Finish the remaining
accepted offline build and its original C9/SAME60/full verification gates before
the round's single code push and exact-head review. Until acceptance: STOP.


## Recorded operator acceptance — deterministic assembly

ACCEPTED: oversight mailbox `671e6ba2`, 2026-10-17, relays Juan's explicit
**Accept (Recommended)** for the proposal at local contract-only commit
`4884e453c1db4823ba8d41b53da65553fee069ee`, contract SHA256
`29cb08a2eba2423cd4f7030a91fac84771be0dcf691b5f16e5c71597b007aaa0`.
This supersedes that proposal's pending-acceptance stop only.

Replace the consolidation model call with deterministic assembly: every section's
claims remain in document order; only exact duplicates with identical text,
quotes and governing sources may be removed; every origin retains full lineage.
Each final claim still receives complete governing source at C9. More than the
accepted distinct-claim ceiling stops the summary without selecting a subset.
Near-duplicates remain separate and may make the result repetitive; this is the
operator-accepted trade-off.

Record this acceptance in a contract-only commit before assembler behavior code.
Proceed test-first offline, replay the frozen public candidate packet with zero
generation, retain the rejected-request boundary regression, and complete the
amendment's boundary/invalid/stale/reload proofs and remaining original offline
build. The one code push and exact-head review wait for the complete original
C9/SAME60/full gates. PR116, production, live, merge, ready and freeze holds all
remain unchanged. No live budget is granted.

## Accepted source repairs and required baseline gate

Operator direction in this session: “remove the unnecessary constraints at the
source and use the proven preproduction setup as our baseline. Keep the fidelity
checks and prove the repairs before we run the model.” The subsequent direction
requires enforcing reuse of that proof rather than relying on reminders.

Root cause: writer `0e8ce54b` applies the document-wide 64-claim ceiling to every
section, then the response estimator reserves that whole shape. The frozen
input-A rejected unit uses 12,173 framed input tokens, leaving 20,083 output
tokens; the 64-claim reservation costs 23,243. The same estimator fits 55 complete
1,200-character claims. Of 191 A units, none lacks room for one full claim.
The second STOP originates in the dependency owner: after removal of the title
word lookup, an explicit named qualifier is ambiguous and blocks two dependent
units, which the constructor escalates to whole-document failure.

Required change surface:

- The section planner derives its claim count from real admitted output space
  at the full existing character allowance; its prompt, schema, parser and saved
  validation agree on that count. The final document ceiling stays unchanged.
  A response filling its section allowance is not proof of complete coverage:
  split complete primary units and re-plan, retain the superseded response and
  forward-only durable ownership, and STOP on indivisible saturation. Do not
  clip claims, drop context, retry the same request, or deliver a silent subset.
- At the existing dependency resolver, an explicit `<kind> <n> of the <Name>`
  qualifies as an unowned named external only if neither the numbered target nor
  a declared document, annex/part or heading identity is owned. Any partial match
  remains blocking unless it resolves cleanly. Use structural ownership, no
  title-word/statute lexicon. Carry the existing external disclosure through
  source coverage; never assert absent text's content.
- Extend the existing qualification script with a mandatory offline baseline
  receipt and checker. Pin the existing frozen summary captures and writer
  input inventory; reuse the accepted counter, source gate, capacity, assembly,
  archived C9 and SAME60 owners. A receipt binds relevant source bytes, frozen
  inputs, commands and output hashes. Missing/stale/failed/partial proof or zero
  matching tests cannot pass. The supported writer launch path and local push
  hook require that proof. CI tests the gate's refusal cases and public runtime
  regressions; private text is never copied into GitHub or CI. Preserve any
  existing hooks. This is a workflow guardrail, not an anti-bypass security claim.

Baseline and departures: the preserved `summary_run.py`, wire captures and native
summary manifest above remain the reference: full text, 32,768 context, 1,500
output tokens and free prose. This candidate deliberately retains atomic claim
JSON, source citations, complete governing dependencies, deterministic assembly
and C9 verification. Those are fidelity requirements, not model capability
claims. No prompt/framing/model/sampler/slot change is authorized by this repair.
The historical 1,200-character and document-count bounds stay explicit safety
policies; removing the erroneous per-section reservation does not qualify a
larger model window or claim ceiling.

Verification: fail-first public reservation and external-identity regressions;
owned-name, owned-number, mixed, ambiguous, unsupported, empty and boundary
twins; saturated multi-unit split and indivisible STOP; DB reload/CAS/no-redo
tests; causal mutants; both frozen inputs plus public capacity control through
the approved offline counter; exact assembly/citation preservation and unchanged
historical C9/SAME60 lists. Gate tests must reject missing, stale, changed-input,
tampered-artifact and zero-test receipts before dispatch/publication. Run adjacent
Rust/Python, strict format/Clippy and applicable Windows compile checks, then one
grouped push with the Windows repair and independent exact-head review.

Non-scope: no model calls, GPU/server startup, new generation budget, fidelity
threshold changes, private-label tuning, partial delivery, ordinary-build
enablement, production adoption, ready or merge clearance. Existing holds remain.

### Source-owner consolidation clarification from offline evidence

Operator acceptance with condition is recorded in PR132 comment 6257990966
(2026-10-19), superseding the earlier all-namespace number rule. The local
candidate already contains the distinction described below; this docs-only
commit records its acceptance before any further implementation or publication.
Owned names are checked in every branch and resolve internally or block if
unclean. Existing accepted explicit-document forms may remain external despite
an identical local number. New opaque names require both name and number to be
absent. No new word or role list is allowed: explicit-document recognition must
come from the existing grammar. Proof includes every changed reference on both
frozen inputs (public shapes/counts, names redacted), second-side tests and a
causal mutant for each branch, and fresh measured counts. No live authority or
production/merge hold is changed.

The new both-unowned rule applies to the newly admitted **opaque-name form**.
It does not replace the established explicit-document/numeric-namespace grammar
with a global-number test. That attempted consolidation was a porting regression:
the actual frozen source measurement changed A to three unresolved references
and B to three, blocking both documents. No model was run. The reader now retains
`ExplicitDocument` versus `OpaqueName` as typed metadata; one resolver checks
owned names for both. Only opaque names also need an absent local number. An
explicit foreign document's section number does not acquire local identity just
because the same digits occur here. No document/law names or abbreviations are
looked up. Existing structural document-role words describe the supported grammar,
not proof of absence; they no longer bypass the ownership decision.

The same line reader retains headings, introductions and supported explicit
self-name declarations as distinct identity signals. Mentioning another document
in ordinary prose does not create a self-name. Known-name and opaque-name
references to an owned heading/self-name resolve in that owned numbering scope
when clean, and otherwise stay unresolved rather than becoming a false external. These source-only controls precede remeasurement of both inputs;
no test labels or source bytes change. The final private report must explain
every reference-record delta, not merely report zero blocking counts.
