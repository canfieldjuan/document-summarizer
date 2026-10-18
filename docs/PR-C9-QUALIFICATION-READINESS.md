# C9 qualification readiness — Phase A

Status: **Offline seams accepted and measured; exact-head review pending; NOT QUALIFIED.**
The original Phase A proposal below is retained as its historical inventory.
The accepted measurement results at the end supersede its A2/A3 unknowns. No implementation,
generation, GPU use, runtime startup, activation or hold release is authorized.
Audit base: `4fdb5191d35b43f1dc810527d1ecc68fc30852a5`, the passed PR128 head.
All code citations below refer to that revision. Historical receipts are identified
as historical; their results do not become exact-head live qualification.

## Gaps first

- **Confirmed:** ordinary builds cannot use C9. `candidate_enabled()` is
  `cfg!(test)` and the other path returns `VERIFICATION_NOT_QUALIFIED`
  (`src-tauri/src/pipeline/summary/comparisons.rs:34-49`). General drafting
  returns the existing source-claim fallback before requesting prose
  (`src-tauri/src/pipeline/summary/coherent.rs:2939`). Green CI does not clear this.
- **Confirmed:** the successful frozen diagnostic was much smaller than the
  required full-document inputs. The restoration record explicitly keeps both
  A/B admission and unseen qualification incomplete
  (`docs/PR-C9-PREPRODUCTION-PARITY.md:173-182`).
- **Could not determine:** the current final verdict for the exact A1 raw-response
  replay. The available A1 test injects Supported before running guards; it does
  not execute the saved-response parser. This audit does not substitute that test
  for the requested replay (`semantic_support.rs:964-988`, path expanded below).
- **Could not determine:** current per-claim admission counts on DV07–DV18.
  These frozen PDFs have no saved claims/source sets. Producing those through the
  writer requires generation, outside Phase A. A source-only lower bound also
  needs an input-capable inventory seam that the current clause owner lacks.

## A1. Gate inventory

Short paths in this table: `comparisons` means
`src-tauri/src/pipeline/summary/comparisons.rs`; `coherent` means
`src-tauri/src/pipeline/summary/coherent.rs`; `guard` means
`src-tauri/src/pipeline/summary/coherent/semantic_support.rs`.

| Required condition, contract quotation and citation | Actual state at audit base | Evidence | Operator decision? |
| --- | --- | --- | --- |
| Diagnostic parity: “exactly 30 calls on unchanged cases”; “30/30 exact enums” (`docs/PR-C9-PREPRODUCTION-PARITY.md:162`) | Historical gateway restoration matched the bounded packet. A later native run passed its existing 30-case checks but stopped later in the stage lane. Offline PR128 regressions preserve the archived cases. None proves current live full-document qualification. | E1, E2, E3 | No relabelling or stricter retrospective baseline. New dispatch needs accepted scope/budget. |
| Relevant-change freshness: “Bind the receipt to the exact tested head, tracked source hashes, input/label manifest and runtime identities” (`docs/PR-C9-PREPRODUCTION-PARITY.md:47`) | Historical live receipts predate this guard head. Exact-head live qualification is absent. | E1/E3 retain their tested heads; no current live receipt | Accept protocol and budget first. |
| Full-document admission: “Full-document admission is new design requiring separate operator acceptance” (`docs/PR-C9-PREPRODUCTION-PARITY.md:182`) | A/B historically failed before generation. Current source still requires complete claim plus deduplicated governing sources and bounded excerpt enums; no source-selection repair has been made (`comparisons:147-204`). | E4; current A3 measurement incomplete | **Yes:** A4 design and scope. |
| Native profile scope: “Native profile scope, unseen qualification and the operator hold remain” (`docs/PR-C9-PREPRODUCTION-PARITY.md:182`); actual added requirement in hold reply `discussion_r4212014481` | Still-admitted direct GGUF profiles are Jack 27B and Qwen3.5-9B; the 30B entry is also admitted as OllamaNative (`model_settings.rs:75-130`). A 9B-only proposal cannot qualify other profiles. The direct framing repair has two GGUF consumers; any required Ollama stage evidence must be reconciled explicitly, not silently exempted. | E5; profile catalog; native stage receipt E3 stops before synthesis completion | **Yes:** qualify retained profiles or separately accept admission removal. No removal proposed here. |
| Unseen qualification: “until C9 passes full-document proof and unseen qualification” (`docs/PR-C9-QUALIFICATION-GATE.md:25`) | No candidate results on DV or ContractNLI have been generated in this lane. No accepted unseen protocol/labels yet. Existing SAME60/C9 replays are regressions. | E6; dataset fingerprints below; no unseen result receipt | **Yes:** A5 protocol, labels and budget. |
| #127 seed: real Verify falsely supported and delivered the independently WITHHOLD A1 unit (issue #127, frozen evidence) | Raw response preserved and fingerprinted. Guard-only historical evidence says Unsupported; direct raw parser→comparator→guard replay at this head is **NOT RUN**, not passed. | E2, E7; A2 below | Accept missing offline seam before implementation. No new model call needed. |
| Ordinary safety gate: “ordinary library/application builds … cannot enable it via settings, environment or features” (`docs/PR-C9-QUALIFICATION-GATE.md:45`) | Confirmed in `comparisons:34-49`. Verify rejects at `summary.rs:1299`; pending completion rejects at `summary.rs:876`; drafting falls back at `coherent:2939`. Historical reads remain distinct from pending completion. | Current code; no activation change | Activation remains a later explicit decision after proof. |
| Hold release: “the operator explicitly clears … the hold thread”; “Green CI, review … do not authorize merging” (`docs/PR-C9-QUALIFICATION-GATE.md:25-30`) | Capacity thread and operator hold both remain unresolved. This audit does not edit or resolve either. | E5; hold `discussion_r4196409571` | **Yes; operator only.** |

Related trackers remain OPEN in the captured issue inventory: #93 (claim-ledger
delivery/capacity), #124 (whole-document fallback for a single over-limit claim),
#127 (A1 false approval), #129 (invented permission), #130 (reading-order
measurement), #131 (passive/nominal prohibitions). E8 captures their state.
This document neither fixes nor waives any of them. In particular #129/#131
remain separate lanes; unseen failures in those classes still count as failures.

### Proven setup first; departures remain visible

The original comparison mechanism is a joint four-dimension response, exact
token-aligned excerpt choices, 240-scalar maximum excerpts, four selections per
side per dimension, and application aggregation. Current owners are
`comparisons:100-132`, `:172-204`, `:222-259`, `:306-315`. Inputs are capped at
4,096 Unicode scalars for the claim plus unique complete governing sources;
each enum is capped at 8,192 entries (`comparisons:5-9`, `:165-171`). The native
schema cap is 1 MiB (`contracts.rs:850`); the gateway restoration used its
250,000-byte cap (`docs/CONTRACTS.md:701`). These limits are independent.

Historical gateway restoration: 30/30 verdict matches, 29/29 adjudicated cases
and one unresolved label, 111/120 relation matches, 227/250 reference containment;
native token/framing equality there used zero generation
(`docs/PR-C9-PREPRODUCTION-PARITY.md:173`). Later native diagnostic E3 passed the
existing 30-case checks, with six containment losses, nine improvements and nine
relation differences; it then stopped at its preflight ceiling. It does not
retroactively turn containment into 100% or complete all-stage qualification.

Current differences from those proofs: later profile/default integration, repaired
native message boundaries, changed writer/planning requests, and the PR128 semantic
guard. The stages use their current owners rather than the historical whole-contract
freeform runner. E2 exercises the current guard source offline. The 9B freeform
boundary proof E9 reproduced two outputs byte-for-byte; its own receipt explicitly
leaves the other native stages and retained Jack profile open. Neither that proof
nor the earlier archive qualifies the current cited writer's fidelity. The
departures are reasons to preserve parity checks, not reasons to blame the model.

## A2. Frozen #127 response: inventory, not a claimed replay

The actual seed is `native-verify-safety-net-20261011/live/wire-1/response.raw.json`,
SHA256 `3c03705f9c98e8c1b94efc0ee9a6863f9d27c78844210377a2841dc49b5b79a4`.
The persisted draft is `native-verify-safety-net-20261011/draft.json`, SHA256
`d89e071e23ad4f741b4395e148939aa1947e55efca5f1771ec794d9834e14fe2`.
Both were re-hashed. The raw response is untruncated and reports preserved for
all four dimensions. E7 also binds normalized/analyzed/chunked inputs and the
saved logical request. The historical result records Supported and delivery;
the independent WITHHOLD label remains unchanged.

Actual current path: `comparisons::Prepared::parse` checks response shape, exact
catalog ownership and relation validity, then aggregates
(`comparisons:222-259`); `summary.rs:1352` passes the resulting summary verdicts
to `guard::apply_semantic_fidelity_guards`. That owner checks claim/evidence
identity and demotes a Supported verdict when `semantic_fidelity_supported`
fails (`guard:4834-4881`). This describes control flow, not an executed A2 result.

Existing seams were checked:

- `guard::original_a1_unit_is_withheld` (`guard:964-988`) fabricates initial
  Supported verdicts. E2 reports Unsupported from that guard-only check.
- `comparisons/parity.rs:1260` replays its fixed archived C9 fixture through the
  parser and guards; it accepts no external A1 input.
- `coherent.rs:9659` accepts saved writer-response cases, rebuilds a draft through
  the writer parser, and only then verifies it. That changes the starting point
  from the required persisted A1 draft.
- The archived safety-net export has a live driver, not a current-head offline
  raw-response replay entry point. Running that driver is outside Phase A.

**Current final A2 verdict: unmeasured.** No parser pass or delivery outcome is
claimed. Oversight explicitly directed using an existing seam only, otherwise
reporting this gap. Proposed later seam: test-only saved-byte input at
`comparisons::Prepared::parse` and the existing summary semantic guard caller,
with the persisted draft/sources unchanged and generation made impossible.
It must not duplicate parser, comparator, ownership or guard logic. This is an
offline prerequisite, not permission to implement it now.

## A3. Admission inventory and measurement gap

The frozen DV manifest was re-hashed to
`d618b0708173dc13f765c319e58789d60cbdccd536ee54f03c97827202b7bcba`.
All 18 frozen PDF hashes match their manifest entries. Twelve are designated
text-layer PDFs and six scans; those designations are manifest metadata, not
fresh parser results. E6 records only aliases, fingerprints and counts.
No private text or original filenames appear here.

| Alias | Saved summary claims | Current over-4,096 / enum / schema / context count | Current clause-size distribution / clauses exceeding 4,096 alone |
| --- | ---: | --- | --- |
| A | 3 | Unmeasured; historical planner rejected document | Unmeasured |
| B | 6 | Unmeasured; historical planner rejected document | Unmeasured |
| DV07 | Unknown | Unknown — no saved claim/source set | Unmeasured |
| DV08 | Unknown | Unknown — no saved claim/source set | Unmeasured |
| DV09 | Unknown | Unknown — no saved claim/source set | Unmeasured |
| DV10 | Unknown | Unknown — no saved claim/source set | Unmeasured |
| DV11 | Unknown | Unknown — no saved claim/source set | Unmeasured |
| DV12 | Unknown | Unknown — no saved claim/source set | Unmeasured |
| DV13 | Unknown | Unknown — no saved claim/source set | Unmeasured |
| DV14 | Unknown | Unknown — no saved claim/source set | Unmeasured |
| DV15 | Unknown | Unknown — no saved claim/source set | Unmeasured |
| DV16 | Unknown | Unknown — no saved claim/source set | Unmeasured |
| DV17 | Unknown | Unknown — no saved claim/source set | Unmeasured |
| DV18 | Unknown | Unknown — no saved claim/source set | Unmeasured |

A/B counts are artifact metadata from the tracked public fixture, SHA256
`079211fe128c9bced0c52e8c36c4e5fd665f4c506b33995eaee7fa57097de627`.
E4 records both documents returning `VERIFICATION_INPUT_TOO_LARGE` with zero
generation. It is historical document-level evidence, not a current per-claim
measurement and not evidence that every claim fails. No count above is zero by
default. DV01–DV06 are excluded scans; no OCR request was made.

Why no substitute measurement was fabricated:

1. `summary/pages.rs:687` dispatches Analyze generation; `coherent.rs:4065`
   dispatches writer generation. New real claim/source sets cannot be obtained
   from these PDFs by just invoking the Verify planner.
2. The current A/B inventory entry point first constructs an admitted gateway
   runtime, then plans and can preflight requests (`comparisons/qualification.rs:141-152`).
   It is not a standalone pure planner command. It was not run in this audit.
3. The existing arbitrary-PDF deterministic test does parse, normalize, structure
   and chunk without a model (`tests/office_acceptance.rs:592-623`), but its
   report exports block/chunk metrics rather than the governing clause catalog
   (`:723-770`). The normalized artifacts are not exported by that seam.
4. General Verify uses `whole_clauses::Clauses::new` and `context`
   (`coherent/verification.rs:21-38`). The owner is private and has no
   arbitrary-input clause-size inventory entry point (`coherent/whole_clauses.rs:15-158`).
   The existing saved-source replay (`coherent/verification/tests.rs:500-603`)
   needs prior analyzed/drafted/verified/delivered artifacts and measures an older
   planner; it does not solve this missing-input problem.

The proposed offline measurement seam should reuse the actual parser,
normalizer, furniture and General `Clauses` owner, then export alias-only size
histograms and counts. It must not use the separate Contract heading reader or
reimplement clause splitting in a script. A clause exceeding 4,096 alone is an
unconditional lower bound on failures for claims citing it. A shorter clause
does **not** prove admission: claim length, multiple governing clauses, enum
growth, schema bytes and context can still reject it. Measuring this lower bound
requires no model and does not consume the unseen output labels.

## A4. Admission design proposal — operator choice required

Root cause: the writer can place several propositions and their citations into
one summary unit, while Verify admits a single bounded claim plus the union of
its complete source-owned clauses. The source owner is
`coherent/verification.rs:21-38`; the first admission rejection is
`comparisons::Prepared::new:165-204`. Enum growth can reject an input well below
the scalar cap (`comparisons::span_catalog:100-132`). A page limit cannot be
derived from the diagnostic or from currently unmeasured DV inputs.

| Design | Owner and cost | What it cannot establish |
| --- | --- | --- |
| Bounded source selection per claim | Change the drafting/source association owner (`coherent::source_catalog_for_profile:7562` and writer parser `:6843`). Preserve every genuinely governing dependency, including exceptions. Selection needs independent coverage labels; omission risks false approval. | Selecting convenient quotes cannot justify deleting governing clauses or turn oracle evidence into retrieval proof. A single oversized clause remains oversized. |
| Atomic source-bound claims at drafting | Change the writer request/claim materialization owner, keeping each separately verifiable proposition tied to its complete governing set. More units mean more calls and potentially the existing unit/batch caps. Preserve meaning and exceptions, not arbitrary character chunks. | Splitting a claim cannot shrink one indispensable full clause or guarantee enum/schema admission. It is new writer behavior requiring qualification. |
| Explicit whole-document fallback (#124) | Define the existing pre-generation fallback boundary and user-visible reason. Cheap and safe when admission fails. | It does not make C9 admit the document, qualify prose, or satisfy the operator's decision not to merge gated-off C9. Accepting this cost is a product choice. |

**Recommendation:** first accept the two missing offline owner seams and measure
the unchanged owners. For the behavioral direction, prefer atomic source-bound
drafting units with complete governing dependencies. Preserve unchanged caps;
report any over-limit unit as inadmissible. Do not commit to an admission success
rate before the measurements. This repairs the information association where it
starts rather than adding another phrase guard or selecting evidence after
seeing a verdict. It cannot promise all long documents will qualify.

If the measurements show that single governing clauses dominate the failures,
claim splitting is insufficient. Stop for a separately accepted representation
or disclosed fallback policy; do not silently stack either onto this choice.
No truncation, cap increase, coarse segment fallback or downstream rule is
authorized by this recommendation. Existing limits and the #124 decision stand.

## A5. Proposed unseen protocol and exact budget

**Proposed only.** Operator acceptance of the corpus, thresholds and budget is
required before a new execution contract. No Phase A call has been made. This
proposal qualifies a bounded C9 comparator scope; it does not waive full-document
writer fidelity, retrieval, retained-profile stage qualification or hold release.

### Inputs and independent labels

- Keep DV01–DV18 frozen to the E6 manifest. Use DV07–DV18 for the text-layer
  challenge. Keep six scans outside this protocol; OCR qualification is separate.
- Independent oversight/operator labels before candidate outputs: one faithful
  source-grounded proposition and one materially unfaithful twin per text
  document, 24 cases. Keep the complete owning clause/dependencies and labels
  for omissions, modality, condition, exception and actor/direction differences.
  Do not use #128 candidate results to choose easier rows. Freeze the human input
  manifest, labels and sources before admission or inference. These are human
  challenge claims, **not model-generated full-document summaries**.
- ContractNLI: use only its official frozen test split, with authors credited to
  Yuta Koreeda and Christopher D. Manning (2021), *ContractNLI: A Dataset for
  Document-level Natural Language Inference for Contracts*, Findings of EMNLP.
  Bundled LICENSE and TERMS identify CC BY 4.0. Preserve attribution, licence
  reference and a record of adaptations. Test JSON SHA256
  `460267b56052a2dc5aead98eb35eadef9e6734d5723d37b4a9790e410f812387`.
- Use the four preselected permission/prohibition families `nda-4`, `nda-7`,
  `nda-17`, `nda-20`. Within each family take the first five Entailment and first
  five Contradiction documents ordered by numeric dataset ID: 40 pairs, 20 of
  each label. Metadata counting confirmed the quotas and E10 freezes these exact
  proposed pairs before candidate outputs. Independent review must bind E10 and
  its source-scope decisions in the execution contract. No substitution is allowed.
- ContractNLI labels concern whole documents. A reviewer must check the selected
  complete governing sources actually establish the whole-document label before
  freezing requests. Gold-source selection is an oracle comparator diagnostic,
  not application retrieval proof. NotMentioned is excluded because absence over
  a whole document does not supply a governing source or map automatically to
  Unsupported. Conflicting scope or indeterminate labels stop readiness; no
  relabelling or substitution. No claim of all-ContractNLI coverage.

The private corpus and dataset have not been run through this candidate during
Phase A. Metadata inspection is disclosed; no candidate output has informed
labels. Public publication contains only aliases, counts and hashes.

### Thresholds and stopping

- Before any live call: complete A2; measure unchanged A/B admission; freeze
  every input/request/schema and independent label; obtain oversight readiness.
  Every required case must admit. Count all rejected cases in the denominator;
  do not replace them, crop sources or raise a cap. An admission failure stops
  before generation and fails readiness for this declared protocol.
- Re-run the existing 30-case native C9 regression using its original scorer and
  frozen baseline, without imposing a new absolute passage score on old cases.
  Report each relation and containment difference. No changed labels or missing
  cases; the original checks must pass.
- For the 64 unseen cases: require 32/32 faithful Supported and 32/32 unfaithful
  Unsupported, zero wrong approvals, exact owned strings, valid relation shapes,
  complete responses and relevant evidence. Ambiguous is safe withholding but
  fails this proposed exact-label gate; report it separately from false approval.
- Pre-register human source/claim spans for each unseen relevant dimension;
  require every recorded span contained in the exact selected excerpts. Report
  passage containment and relation validity separately. Extra selected text is
  reported; it cannot substitute for a missing required span. This stricter unseen
  threshold is an explicit proposed new standard, not a reinterpretation of C9.
- One invocation, no retry/resume/reset/refund. Stop at the first semantic,
  protocol, request-identity, provenance, output-limit or resource failure;
  retain partial results and unrun cases. No tuning to improve the label.

### Runtime and budget

Proposed hard ceiling: **94 Verify completions = 30 frozen regression + 24 DV
challenge + 40 ContractNLI**. No Analyze or Synthesize generation, retries or
replacement calls. One joint request per case; this does not raise the production
64-batch cap for any document (`summary.rs:103`, `comparisons::plan:392`).
Proposed delegated preflight ceiling: **188**, reserving one frozen-request
preflight and one classifier preflight per case. The classifier owns the latter
(`comparisons:337-353`); native generation separately checks schema/token/context
before completion (`llama_cpp.rs:742-779`). Tokenizer operations are recorded
separately, never described as model completions. An execution adapter must prove
this schedule offline before the live contract is ready; no hidden preflight
search/repair or extra generation is permitted.

Pin the production selected 9B factory: profile `qwen35-9b-q4km-v1`, GGUF SHA256
`cd76ec205963b3b33350093e6904d9de16c4e666fd104e1f632d25c7f15f2a13`,
Qwen3.5 tokenizer `qwen35-pre-cc5fb918-v2`, context 32,768, thinking disabled,
server SHA256 `0ca399edd758decd825a71823b04ba7ddbc8b2e10d2309d8bf623ee3c2283099`
and the complete `JACK_LLAMA_CPP_LIBRARIES` pin set
(`model_settings.rs:47-48`, `:113-145`; `qwen_tokenizer.rs:12`). Seed 7,
temperature 0, Verify output 4,096, current closed-thinking/message framing,
slot 1, offload 999, reasoning off, no warmup, cache_prompt true
(`llama_cpp.rs:326-347`, `:584-592`; `summary.rs:94`; parity contract:35,45).
Freeze all effective inherited settings, template/token arrays and runtime
arguments against the proven receipts rather than assuming omitted defaults.
An identity or framing difference stops readiness before dispatch.

Use the existing inference lock and serialized native owner. Never terminate
another owner's workload. Bind receipts to the accepted candidate head, all
relevant source/dependency hashes, corpus/label/request manifests, runtime/model/
library identities and actual before-send bodies. Runtime changes, source
selection repairs, writer repairs, other profiles, full-stage generation and a
gateway rollout are separate scopes with no budget here.

## Required change surface after acceptance; explicit non-scope

First prerequisite proposal is offline-only: expose test-only input seams at
the current comparison parser/guard caller and General parse/clause inventory
owner. Keep executable production bodies unchanged; use existing validators and
proof lanes. Public positive/negative controls and generation tripwires must
prove the seams before any private replay. Those are proposed changes, not part
of this docs-only commit.

Any accepted behavioral item then needs its own acceptance record, owner-level
fail-first regression, origin repair, mutations, required Rust checks and exact
C9/SAME60 comparison, one push and oversight exact-head PASS. No O3 work, #129/
#131 fix, prompt/schema/cap/model/settings change, installed app, merge, ready
transition, relabelled fixture, hold edit or production activation in Phase A.

## Evidence index

All aliases below are under the durable `document-summarizer` evidence namespace
unless explicitly the private heldout namespace. No artifact is worktree-only.

| ID | Alias | SHA256 |
| --- | --- | --- |
| E1 | `c9-preproduction-parity-20261007/gateway-restored/receipt.json` (historical pin recorded at parity doc:177) | `54977626b64ee78505f49d26e07b99c2d1e5655bcba8c67fc0eb11bc61e3cd47` |
| E2 | `negated-grant-contract-20261014/o2-final-offline-proof.json` | `f61ad7dc97c8bd3ebd9a812fc8ed796c739199be07c01fac777de41869de9f18` |
| E3 | `native-stage-qualification-contract-20261009/qualification-live-stop-summary.json` | `df7d928a2a9c9b2b8359401a7f6939825d2b5674cb192cd29b861791b8f9fdbf` |
| E4 | `c9-preproduction-parity-20261007/restored-public-admission/public-admission.json` | `36d966389fd06954bf77eb0712b78a37ded0fef89fe01032b61c68000ecc7251` |
| E5 | `c9-qualification-readiness-20261016/hold-threads.json` | `568e01ee6108904d166e468c92ed8626b06412444bbd5a82ae5d0df3ef69ead3` |
| E6 | `c9-qualification-readiness-20261016/dv-inventory.json` | `7f85fa668f5ccaeae34c07470bbb9c822f66847ef58c6e2c927473193fe3bfc4` |
| E7 | `c9-qualification-readiness-20261016/a1-seed-inventory.json` | `a1798aca57cb589eb32d1da24a89a9bc0235070152f3624794a9ae9d1a6bfc08` |
| E8 | `c9-qualification-readiness-20261016/prior-evidence-and-issues.json` | `b00dab44b015839b601fad46e3b09f23f7c710224a7c5cd3013f965879c62dbe` |
| E9 | `native-message-boundary-repair-20261007/result-summary.json` | `8cc43e004e2ec3a0f644a7a2d694b1be55ba6922757c38585bea4a11b497662e` |
| E10 | `c9-qualification-readiness-20261016/contractnli-proposed-selection.json` | `45ed774fd4ccf94c07e792e1f05683596631213cc36658b6713544415c4befdd` |

## Implementation summary, cold diff audit and gap audit

This commit adds this document only. No source, tests, input fixtures,
configuration, dependencies or workflows change. No code suite was rerun for
this audit document. Verified here: frozen-input fingerprints, saved raw-response
metadata, current owner/control flow, public fixture counts and current issue/
hold state. The previous offline results remain tied to E2; they were not rerun
or presented as live qualification.

**NOT DONE** for qualification and A2/A3 execution. A1 inventory and A4/A5
proposals are reviewable; A2 needs an existing-owner replay seam, A3 needs a
clause inventory seam and real saved claim/source inputs. Full-document admission,
fresh live evidence, retained native stage qualification, independent unseen
labels/results and operator release remain open. These gaps are not permission
to build a Phase A harness or consume the proposed budget.

**STOP A:** after this one docs-only draft PR, wait for oversight to relay the
operator's choices verbatim: admission direction (including offline prerequisites),
unseen scope/thresholds and call budget. Neither oversight advice nor a green
check supplies operator acceptance. No merge, ready transition or hold resolution.

## Accepted offline measurement slice (2026-10-16)

Operator acceptance is recorded verbatim in [oversight reply 6208395301](https://github.com/canfieldjuan/document-summarizer/pull/132#issuecomment-6208395301):
"Add both now (Recommended)"; "Test-only, no production change, no model or GPU. Gives the real #127 result and real size numbers before any design choice."
The operator then directed this session to "do it".

### Contract

- Root cause: the prior guard-only seam injected Supported instead of parsing
  the saved A1 answer (`semantic_support.rs:964-988`); the deterministic PDF
  checkpoint test does not expose the General governing-clause sizes. These
  are missing measurement paths, not authorization for a semantic repair.
- Required surface: a cfg(test) readiness module wired from coherent.rs;
  saved response parsing through existing comparisons::plan/Prepared::parse and
  apply_semantic_fidelity_guards; a test-only inventory adapter at the General
  whole_clauses::Clauses owner. Reuse PdfExtractParser, CanonicalNormalizer and
  Furniture, preserving their original bodies. Output aliases, sizes and hashes.
- Non-scope: no production body, prompt, schema, runtime, model, cap, label,
  delivery behavior, installed app or holds change. No GPU/model startup or
  generation. No unseen run or profile retirement. Do not open or inspect the
  frozen DV challenge-label file in this slice.
- Verification plan: commit this acceptance first; fail-first public controls for
  saved-byte ownership/identity/invalid responses and exact clause-size bounds;
  prove the generation tripwire; public controls must pass before private input;
  then replay the frozen A1 bytes and inventory public A/B and DV07-DV18 through
  actual owners. Missing OCR/claims remain explicit. Mutation checks must catch a
  bypassed parser, guard or clause inventory. Run focused tests, formatting,
  strict clippy and the existing C9/SAME60 regression lanes as applicable; prove
  production bodies unchanged. One push, exact-head review, then report and STOP.
- Assumptions: A3 measures source-context sizes and unconditional over-limit
  contexts, not complete claim admission or a page capacity. General and Contract
  clause readers remain distinct; use only the General owner consumed by Verify.

Admission design remains "Decide after numbers (Recommended)". Unseen protocol
and 9B scope are accepted directions; dispatch of the proposed 94 calls remains
unauthorized. Oversight drafts labels; the operator approves them. The mailbox
reports frozen DV-label SHA256
`ba5215a3731e6259dd71548e043d540c85dca4b1330af1344413b96b21d688d5`;
this is a relayed hash only, not a file inspected by this session. All existing
PR116/126/128 and draft/merge/production holds remain. After the two offline
measurements, return the numbers for the admission decision and STOP.

Measurement detail: A/B rejection below the scalar cap requires inspecting the
existing excerpt-catalog admission. A cfg(test) wrapper in comparisons.rs calls
span_catalog unchanged and returns only its size/error. The adapter compares
individual-source and union admission to identify an enum-union overflow without
duplicating tokenization or enumerating excerpts independently. No production
body or cap changes. This remains within the accepted offline measurement scope.

## Accepted offline results — return admission decision to operator

### A2: actual saved-response replay

The frozen A1 logical request matches the current planner exactly, including
system/user bytes, schema, decoder serialization, seed, ordinal and output cap.
The draft is unchanged. The real comparison parser returns **Supported**; the
existing semantic guard returns final **Unsupported**, with diagnostic reason
`evaluation_or_causal_relation`. The verdict-changing owner is
`src-tauri/src/pipeline/summary/coherent/semantic_support.rs:4878-4879`.
`coherent/readiness.rs:103-115` feeds the actual parsed verdict to that owner;
it does not inject Supported. This is offline evidence, not a live model or
full completion/delivery qualification.

### A3: General source-owner size inventory

Counts below deduplicate effective full source strings, just as admission does.
An expanded context comes from General `Clauses::context`; quote-only means that
owner supplies no larger context and the verifier retains its existing exact-
quote fallback. It does not mean the document has no logical clause or heading.
The inventory calls General `Clauses::segment/context` with the current Furniture
policy, not the separate Contract heading reader. Sizes are Unicode scalars.
P50/P90 use nearest rank. A/B use their frozen public normalized artifacts;
DV PDFs go through the actual parser and canonical normalizer, without OCR/model.

| Alias | Effective source values | Expanded / quote-only | Min / P50 / P90 / max | Values >4,096 | Omitted source units |
| --- | ---: | ---: | --- | ---: | ---: |
| A | 12 | 0 / 12 | 418 / 433 / 476 / 480 | 0 | 0 |
| B | 24 | 0 / 24 | 401 / 433 / 485 / 493 | 0 | 0 |
| DV07 | 33 | 0 / 33 | 81 / 441 / 569 / 595 | 0 | 9 |
| DV08 | 5 | 0 / 5 | 89 / 143 / 1070 / 1070 | 0 | 2 |
| DV09 | unmeasured | unmeasured | `ENCRYPTED_PDF_UNSUPPORTED` | unmeasured | unmeasured |
| DV10 | 6 | 1 / 5 | 344 / 495 / 4886 / 4886 | 1 | 1 |
| DV11 | unmeasured | unmeasured | `ENCRYPTED_PDF_UNSUPPORTED` | unmeasured | unmeasured |
| DV12 | 27 | 1 / 26 | 114 / 374 / 970 / 2207 | 0 | 0 |
| DV13 | 2 | 0 / 2 | 545 / 545 / 585 / 585 | 0 | 1 |
| DV14 | 211 | 0 / 211 | 5 / 352 / 544 / 600 | 0 | 485 |
| DV15 | 50 | 39 / 11 | 28 / 1626 / 5102 / 13569 | 11 | 3 |
| DV16 | 115 | 0 / 115 | 17 / 434 / 562 / 595 | 0 | 99 |
| DV17 | 30 | 0 / 30 | 29 / 444 / 566 / 589 | 0 | 9 |
| DV18 | 9 | 0 / 9 | 15 / 141 / 585 / 585 | 0 | 6 |

DV09 and DV11 are **unmeasured**, not zero. The real native parser refuses them
at `src-tauri/src/pipeline/parser.rs:82-87` with `ENCRYPTED_PDF_UNSUPPORTED`.
Under the accepted unseen protocol every case must admit, so their **four
required challenge claims currently fail readiness**. This is a product gap;
there is no decryption, replacement, scan conversion or label substitution here.

DV10 has one source context of 4,886 scalars; DV15 has eleven over-limit contexts,
maximum 13,569. Any claim needing those whole contexts is inadmissible regardless
of how short its prose becomes. The other observed zero counts do not establish
capacity: the owner often uses quote-only values and reports omitted source units.
No DV model-generated claim/source set exists, so DV claim-level admission counts
remain unknown. No page limit or admission success rate follows from this table.

### Public A/B: first rejecting limit for each real saved claim

All nine claim-plus-source totals are below 4,096. The existing `span_catalog`
owner rejects seven source unions: every individual source catalog admits, but
the union exceeds 8,192 values (`comparisons.rs:122-125`). For the other two,
both source and claim catalogs admit and the actual `Prepared::new` rejects at
its schema-byte gate (`comparisons.rs:198-204`), with native 1,048,576 and gateway
250,000 byte caps unchanged. The diagnostic uses the planner's full-source
projection; it does not enumerate spans or build a replacement schema.

| Claim ordinal | Claim + unique sources (scalars) | First rejection on native and gateway |
| --- | ---: | --- |
| A1 | 2897 | source enum >8,192 |
| A2 | 2971 | source enum >8,192 |
| A3 | 2653 | source enum >8,192 |
| B1 | 2527 | source enum >8,192 |
| B2 | 2635 | source enum >8,192 |
| B3 | 2689 | source enum >8,192 |
| B4 | 2534 | schema bytes >1,048,576 (native), >250,000 (gateway) |
| B5 | 2547 | schema bytes >1,048,576 (native), >250,000 (gateway) |
| B6 | 2762 | source enum >8,192 |

These are fresh deterministic planner observations, not the old document-level
A/B rejection reused as a per-claim count. No preflight or generation ran.

### Verification, cold diff audit and remaining gap

- Acceptance-only commit `3924ff4` preceded code. The two missing-measurement
  probes failed first: absent replay and empty size inventory.
- Eight public controls cover supported/unsupported paths, modality demotion,
  malformed/missing/foreign responses, exact request binding, empty input,
  4,095/4,096/4,097 Unicode scalar sizes, parent/cross-block ownership, partial
  visual pages, catalog boundaries and generation/preflight/startup tripwires.
  Public controls passed before the private measurements.
- All three mutations were caught: fabricated parser result, bypassed guard,
  and byte counting instead of scalars. Sources were restored. A mutation-runner
  selector initially matched two expressions and stopped; only its selection
  was narrowed before the remaining probe. A new negative assertion initially
  used the wrong parser error-code name; it was corrected to the existing
  `MODEL_VERIFICATION_RESPONSE_INVALID`, without weakening rejection.
- `cargo test --lib`: **924 passed, 0 failed, 35 ignored**, including the archived
  30-case C9 regression. Formatting and all-target/all-feature strict Clippy pass.
  Ordinary-library `c9_production_gate` with all features: **5 passed**.
- Fresh existing SAME60 replay: all **825 ordered verdicts and full JSON rows
  equal** the prior O2 result; **31/60** saved summaries validate. The result hash
  is unchanged. It was rerun because literal source-file identity was not met.
- Cold diff: `coherent.rs:9-10` adds only cfg(test) module wiring;
  `comparisons.rs:465-476` adds only cfg(test) catalog/Prepared inspection wrappers;
  `coherent/readiness.rs` contains the adapters, public controls and ignored
  private runner. Existing production bodies are byte-identical after removing
  only those added cfg(test) items. Per-file comparison covers every existing
  source file. This document records acceptance/results. No dependency, workflow,
  prompt, model, schema, cap, settings, saved-input or installed-app change.
- Actual offline measurement: **zero model completions, native preflights and
  runtime startups; no GPU**. Frozen DV label file was not opened or inspected.

Evidence aliases under `c9-qualification-readiness-20261016`:

| Alias | SHA256 |
| --- | --- |
| `seams-measurement-report.json` | `57ce54ed63953ad08883fb165c1a515b17e35740901bbedf4952f1e310275e5e` |
| `seams-production-source-proof.json` | `a2126fb9b15716ab814b8eb6f97862d2d0482a8568e708aab382078e8f29d4fd` |
| `seams-mutations.json` | `e3d1b17452a13df93d4a534e4e2464e177b6b6bfe0c1c91902124334606c815d` |
| `seams-lib.log` | `a98a023658cb2c5d1888c288c303931f3dc0bbb2dfffd3e9d5a4b60f37c64bc8` |
| `seams-clippy.log` | `971ad18b6a3701313515dc2c9dc287aa1f48af656214b4558ae80c58eb3f5e16` |
| `seams-ordinary-gate.log` | `3ca19da0a79ecf715e4dd2b0487587e584bcf3f860fb9fdc518ecf68e212b28f` |
| `seams-fail-first.log` | `ac39c3e79ac5066155e954039b728f19fc73910f5388928ddfd62a23b5f08943` |
| `private/seams-same60.json` | `ceee845cb539968ed40b87185484496e08f455057040665352559cf96b2e5b53` |
| `private/seams-input-packet.json` | `4372001f47546db0a8e74a42b8ef5fa7d0ed48056f3dfe294fb675f8cc74c630` |
| `tracked/readiness.rs` | `01f99f20dc96b499fc910eb0477b471c2b42e6f993f945b65c52c41e855a742d` |

**NOT DONE for qualification.** The accepted offline measurements and local
verification are complete; exact-head CI/review follows the single push. The
unseen run is not ready: four challenge cases cannot admit due to encrypted
inputs, and full-document admission remains unsolved. Return these measured
limits to the operator before selecting a design. Smaller claims alone cannot
solve an indispensable oversized source or guarantee enum/schema admission.
No behavioral repair, unseen generation, profile retirement, ready transition,
merge or hold release is authorized. **STOP after reporting A2/A3 and review.**


## Whole-document writer: accepted offline implementation refreshed on main

This supersedes the earlier pending-implementation status for the accepted
writer slice only. It does **not** grant qualification, activation, ready or
merge clearance. PR132 remains draft and stacked on PR128.

### Contract and actual implementation

The accepted contract and its recorded amendments are in
`PR-C9-ATOMIC-SOURCE-CLAIMS-CONTRACT.md`. The candidate was preserved as
`0e8ce54` before forward-merging main `8df9799` at `8e48679`; the tested code
head is `3500499475a9372aca9cdb93cb21644915d8c976`. No existing writer implementation was
rewritten during the refresh.

- `coherent/whole_clauses/dependencies.rs` owns explicit reference resolution
  and transitive governing-source closure. Unresolved internal references block;
  external references remain disclosed, without invented content.
- `coherent/whole_document.rs` plans whole-source or bounded section requests,
  seals their ownership, persists progress before dispatch, and assembles exact
  completed candidates in document order without a consolidation model call.
  Only identical text/excerpt/governing-source bindings deduplicate; all lineage
  survives. Exceeding the distinct-claim ceiling holds rather than selecting a
  subset.
- The quote origin retains exact UTF-8 ranges and partial-excerpt metadata.
  Contextual anchors retain complete owning context for Verify; they are not a
  quote-only fallback.
- Shared comparison owners produce typed per-claim admission decisions, consumed
  by Verify, saved-result validation and delivery. Held claims cannot dispatch
  or reappear through contribution trimming. Ordinary builds still cannot
  activate this unqualified writer.

### Main integration: reproduce, isolate, explain, fix, prove, prevent

The forward merge exposed stale test callers of the old path-based database
API: two unit-test helpers passed paths where `Store` is now required, and the
ordinary-library C9 integration fixture imported the now unit-test-only
`init_db`. Compiler logs retain E0308 and E0432 reproductions. These are our merge
integration omissions, not a model or writer-fidelity failure.

The fixes use the merged owner: retain a `Store`, use writer/read connections
for their existing roles, and retain the integration fixture's entire private
temporary directory. No compatibility initializer or storage bypass was added.
The profile suggestion binder keeps both main's `Store::writer` and this
branch's runtime-scoped request-owner contract. OCR conflict hunks preserve
main's reviewed timing repair; that test has no remaining diff against main.
The C9 integration assertions are unchanged and all five pass, guarding against
unqualified dispatch and broken reload. All-target compilation and the complete
suite cover the other adapted callers.

### Fresh verification

- Locked all-target/all-feature Rust: library **1031 passed, 0 failed,
  40 ignored**; C9 ordinary-library **5 passed**; office **20 passed, 3 ignored**;
  release **26 passed**. Both former external gate failures now pass.
- Strict locked all-target/all-feature Clippy, formatting, frontend build,
  **7 frontend tests**, and **20 receipt tests** pass. An initial receipt-test
  invocation inherited the evidence writer's restrictive umask and failed its
  fixture-mode assumption; the normal-environment run passes without edits.
- The existing offline counter first reproduced all **30** recorded native
  counts exactly at the registered **32768** context. No generation/startup.
- Source-only dependency report: complete JSON unchanged; inputs have
  **34 resolved / 30 external / 0 unresolved** and
  **20 resolved / 10 external / 0 unresolved**, with zero blocked units.
- Actual writer capacity/anchor report: complete JSON unchanged. Input1 needs
  three requests (**24514, 21431, 9224** input tokens), input2 one (**18896**),
  public size control two (**25590, 22000**). All are admitted by the unchanged
  budget owner. Zero anchor coverage gaps or units without anchors. The exact
  report match preserves the inventory previously independently cleared in
  source gate `72831dea`; this is not a new semantic fidelity review.
- Frozen public assembly replay: **2 drafts, 40 claims, 40 lineage entries**,
  complete JSON unchanged. The original **48634**-input-token consolidation
  request still rejects against **32768** context.
- Historical C9 manifest/projection/prompt/verdict/guard regressions pass in the
  full suite. Fresh SAME60 replay: **60 rows, 825 ordered verdicts, 31 valid**;
  complete JSON unchanged, not only the totals.
- The earlier **16/16** killed owner mutants remain evidence for the original
  frozen source; they were not rerun or claimed as new results. All 25 writer
  source hashes matched before preservation; 21 remain identical after main,
  and the four overlapping files retain main's storage/test-lifecycle changes.

### Evidence and cold diff audit

Durable namespace: `writer-main-refresh-20261018`. Private source-bearing reports
remain in heldout with private permissions; public references use aliases and
SHA256 only.

| Alias | SHA256 |
|---|---|
| offline-refresh-proof.json | `8a882cad9902b3d0a4971a1d8e60f6406dd1c420299a17098448227cb3637a93` |
| full-rust-final.log | `a0e9eb81531f728c9a672632cf021582911a37ceb4a444fa4b2c27a82ed4bd96` |
| offline-replays.log | `1d32f8ff292c6f849121ff48d791f925e9324e1f21fdc40ee1c9597a2bdf37ad` |
| dependencies.json (private) | `37076896e2f8348945fa1d204696060c0a1e63b875b833c1b2b8210e9c39678e` |
| capacity.json (private) | `8c5cc92d6fbd8b1dd45dec69268ba399f59250db79ed369ea7feb99bad97f765` |
| assembly.json | `2dfdd42ecbd1dc469b682c99988e0fdd6367f7b83efa0ba9d1a3476e110bcfd0` |
| same60.json (private) | `ceee845cb539968ed40b87185484496e08f455057040665352559cf96b2e5b53` |
| integration-audit.json | `0c06d240f92a84f37e4956b292323787c68b246201bc32a4ebe438431d8f7521` |

The existing 25-file cold audit is retained in `writer-offline-progress-final.json`
(SHA256 `db54e8d97d78305cd38eb5bf3cdb465fe448f9c4f76df4f31e7096a78691e0fc`).
The integration audit records each conflict/caller, its actual change, owner,
and proof. Core writer/planner/anchor/dependency/comparison bodies and frozen
runtime/count inputs did not change during integration. The final proof-doc
commit changes prose only; code tests are attributed to the tested code head
above and are not rerun for that prose commit.

### Gap audit

**NOT DONE for qualification.** Accepted offline implementation and refresh gates
are complete. Exact-head CI, operator/oversight review and any separately
accepted live qualification remain. Zero model generation, model-server starts
or GPU use occurred. No private benchmark labels were inspected. PR116/126/128,
production, live, ready and merge holds remain unchanged. One writer push, then
stop for independent review; no automatic ready transition or merge.


## Windows test-constructor repair after writer integration

The first writer push fails native Windows compilation at `0082404`:
`DesktopJobManager::new` is excluded by the Unix-only test gate in
`desktop.rs:141`, while the portable ignored live proof calls it. Native run
`39176443849` reports E0599. The original restriction came from `aa4373b`;
our merge `8e48679` and integration `3500499` added the portable consumer
without widening the helper. This is our integration omission.

The fix changes that one owner to `cfg(test)`, retaining all consumers and
excluding the helper from non-test builds. It adds a portable real-constructor
regression that retains the prepared Store path/generation and confirms that
construction creates no settings file or active worker. No Windows test is
hidden; no constructor or storage compatibility shim is duplicated.

Fresh Windows GNU target checking reproduced E0599 before the fix. With the new
regression but old gate it reports both missing constructor calls; after the
owner fix the complete lib/test target check passes. The focused regression
passes, the adjacent desktop suite has **21 passed, 2 live tests ignored**, and
strict Linux all-target/all-feature Clippy, strict Windows-target lib/test
Clippy and formatting pass. No live proof ran. Native Windows CI at the grouped
repair head remains **NOT DONE**; a cross-target check does not replace it.

Durable namespace `writer-review-repair-20261018`,
`windows-repair-proof.json` SHA256 `f28bc49ae94c30b1a46b9805db9659cc6ed3130a5b9e8c2710ce64537af1237d` records all six repair steps,
before/after logs, source freeze and per-hunk cold diff audit. The remaining
independent writer-review findings are grouped with this repair before one
push. All live, production, ready and merge holds remain unchanged.
