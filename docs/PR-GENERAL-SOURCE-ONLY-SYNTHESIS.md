# General synthesis from exact source text

Status: the operator explicitly accepted this revised contract on 2026-10-05:
"Accept the revised contract." The removal and local verification are complete.
PR122 remains draft pending independent implementation review and CI. Acceptance
is not implementation approval or fidelity qualification.

## Contract

Root cause: General source-catalog construction enriches authoritative quotes
with already-generated `source_claim` drafting hints. The model can copy those
hints rather than synthesize the source. In the public worker reproduction, A
copies four hints verbatim and misses page coverage. B joins hints whose combined
length exceeds the decoder's 1,200-character ceiling, producing unfinished units
at and below that ceiling. The gateway returns the content unchanged.

Reproduction at `decf41007d29edc3d12a5cc06ecb91805ab3ebe0` used independently
written public workshop policies through the actual General desktop worker.
Both PDFs fell back. Offline reconstruction matched all saved synthesis request
hashes. The exact B request reproduced its output byte for byte. Removing only
optional `source_claim` fields produced six complete units, still citing all
24 source pages. This is structural synthesis evidence, not semantic fidelity.

The origin is `source_catalog_with_furniture_policy` in
`src-tauri/src/pipeline/summary/coherent.rs`, where extracted claims are joined
onto exact source candidates. Stop that enrichment for new General synthesis.
Do not add a transport filter or relax completion checks.

Required change surface:

- Version new synthesis as 14.0.0. Retain existing saved-artifact version,
  evidence-identity, furniture and verification validation for 13.0.0 and older.
  This does not require rebuilding generated drafting hints.
- Remove `SourceCandidate.drafting_claim`, `PromptSourceSegment.source_claim`,
  their constructor/serializer assignments, and the extracted-draft lookup from
  `src-tauri/src/pipeline/summary/coherent.rs`. Exact quotes, framing and
  governing clause contexts remain the source catalog's content.
- Remove the draft-versus-quote equality filter, framing-specific draft
  suppression, General-only hint serialization branch, and test-only hint
  assignments/assertions. Do not keep a second historical hint path.
- Keep current General, Story and Contract request content unchanged relative
  to published source commit `a070297`: the removed optional field already
  serializes as absent on those current paths. Keep the General prompt wording
  byte-identical for this cleanup; its optional-hint instructions do not require
  hints and preserving it avoids introducing another model variable.
- Regression at the catalog owner: supplying short, long or mixed extracted
  drafts cannot change current or historical source evidence, request payloads
  or schemas. Retain framing, split-section and source-order assertions when
  removing obsolete hint expectations.

### Contract revision after oversight

New evidence: the branch at `coherent.rs:7770` was introduced by this slice in
`a070297`. Its only generated-hint consumer is prompt serialization at line
6714. Saved coherent evidence validation at lines 8067-8093 compares canonical
`EvidenceItem` values, not `drafting_claim`. The earlier contract's claim that
old artifacts need the hint path was unsupported.

The named public reproduction is
`public-synthesis-worker-20261005/public-worker-20261005T223251Z` (A/B), with
request reconstruction in the sibling `reconstructed-requests.json`. That
reconstruction runs `synthesize` at recorded source commit
`decf41007d29edc3d12a5cc06ecb91805ab3ebe0`; it does not dispatch new synthesis by
an artifact's saved version. The frozen comparison requests are also preserved
in `public-synthesis-draft-guidance-20261005/cases.json`. Neither requires a
historical hint-generation path in the new production code. Preserve those
artifacts and commits; do not rewrite the evidence to fit the new code.

Frozen replay receipts (alias and SHA256):

- `public-synthesis-worker-20261005/reconstructed-requests.json`:
  `c9d3c55a44071142219f51fc3d8ab7c47ace1a8e0554ad01c4c1643b689ef696`.
- `public-synthesis-worker-20261005/coherent-reconstruction-probe.rs`:
  `77cb2feb02abc9eed7e7966eee062b58209b16b19a225ad0ed119ad7e1666d2b`.
- `public-synthesis-draft-guidance-20261005/cases.json`:
  `f01b6efe3bf7c68e49e88cf3457eba475309f1f114443d596d1a0f2dfc538c62`.

Revised root cause: copied extracted drafts caused the demonstrated synthesis
failure. I then unnecessarily kept that behavior behind a historical-version
branch. Remove the branch and its data fields at their owner, rather than
adding a later suppression check.

Accepted scope: the source-only origin fix plus the hint-path removal above. Acceptance permits implementation and verification of this scope; it
is not merge approval or fidelity qualification. Independent review, CI, the
open oversight thread and PR116's fidelity hold still apply.

Explicit non-scope: no decoder ceiling, model, prompt wording, output budget,
completion parser, deterministic trim, C9 verdict, source coverage threshold,
labels, full clause list, public API, dependency or installed-setting changes.
The existing optional-hint prompt wording remains byte-identical to the public
ablation; it does not require hints to be present. Retain safety recovery for
other causes of incomplete output. The failed private qualification remains
frozen and PR116 remains held.

Assumptions/blockers: the controlled public reproduction establishes this failure
mechanism, not a guarantee for all model outputs. Independent review, a frozen
new candidate, and later full-document fidelity qualification remain required.

Revised verification plan:

- First demonstrate that a historical catalog still includes a generated hint;
  make the owner regression fail before removal and pass afterward.
- Check current serialized request content and schemas against the published
  source-only candidate without new inference. Retain all framing, split-section,
  evidence identity and historical version-pair tests.
- Reopen copied public A/B baseline artifacts under the revised code, using
  isolated storage, to check the retained saved-artifact validation path.
- Run the affected summary tests, formatting and strict Clippy. CI owns duplicate
  broader suites. No fresh private qualification or C9 change is authorized.

Keep all raw evidence outside worktrees with private permissions. Existing live
results below remain historical evidence for `a070297`, not test results for the
removal documented below.

## Evidence

Durable alias `public-synthesis-draft-guidance-20261005/summary.json`, SHA256
`4c46bef58db515517a5d45bd806f5607864ee4c9dd07e12d2ed673ec574b951e`.
Earlier homogeneous/varied isolated probes did not reproduce the defect.
Short-writing and field-order alternatives reduced cited-page coverage and were
rejected. The actual worker and exact request reconstruction supplied the public
reproduction; no private qualification input was used to tune the change.

## Earlier implementation and evidence (before the accepted revision)

`source_catalog_with_furniture_policy` now omits generated drafting hints for
new synthesis. The previous lookup, quote-equality filter and framing filter
remain only in the historical branch. No new downstream text filter was added.
The original hint enrichment dates to `85ff3b39`, before this slice; it was not
introduced by the current source change.

Synthesis 14.0.0 keeps 13.0.0 explicitly supported across artifact validation,
claim budgets, verification pairing, furniture and saved fallback policies.
The catalog regression varies short, long and mixed drafts and proves they
cannot change the new source request, schema or candidate evidence. Existing
historical tests retain draft hints and the prior furniture policy.

Verification at source commit `a07029736609547f1bc373009a81885602ce6d4d`:

- Declared catalog regression failed before the origin change and passed after.
- Adjacent summary suite: 353 passed, 0 failed, 17 ignored. Two stale version
  assertions initially failed and were corrected before this passing run.
- Formatting, strict all-target/all-feature Clippy and diff checks passed.
- Live public B produces six complete units citing all 24 source pages. Its
  response is byte-identical to the controlled source-only ablation. This
  confirms the correction through the actual desktop worker and gateway.
- Live public A still has two units clipped at exactly 1,200 characters. The
  existing recovery proceeds to verification admission. This change does not
  guarantee complete generation for every input.
- Both public documents complete and reopen identically, but fall back at
  bounded semantic verification. The full-worker test therefore fails with
  exit 101; neither result qualifies C9 or fidelity.
- An offline public B probe of the actual comparison preparer rejects all six
  units. Units 1/2/3/6 exceed the 8,192 source-span enumeration cap. Units 4/5
  pass that cap but their encoded schema exceeds even the local 1 MiB cap.
  Their input text is below 4,096 characters. These bounds are unchanged.
  The diagnostic test passed and its temporary source was byte-restored.

Candidate evidence alias
`public-synthesis-source-only-candidate-20261005/summary.json`, SHA256
`81da49cb2608a77c44fcadcdc41332047d9092b44090ed92947f1c39730a954c`.
The source Rust tree is `60915f3e34242bc5094d36d41745a38812935e91`.
Public probe artifacts are durable; private qualification was not rerun or
modified. These results do not authorize changing frozen C9 limits.

## Existing source diff audit

| File and line | Actual change and contract trace | Verification |
| --- | --- | --- |
| `summary.rs:51` | New synthesis version; retain historical 13.0.0 dispatch and budgets | Summary version and artifact tests |
| `summary/coherent.rs:7767` | Omit draft enrichment at the source catalog owner; preserve historical enrichment | Fail-before/pass-after catalog regression at line 13974 and public B live match |
| `summary/coherent/page_furniture.rs:353` | Keep 13.0.0 running-furniture behavior when current becomes 14.0.0 | Historical furniture tests |
| `summary/coherent/verification/tests.rs:687` | Accept saved 13.0.0 pairings; advance unknown-version negative case | Summary suite |
| `docs/PR-GENERAL-SOURCE-ONLY-SYNTHESIS.md:1` | Record scope, evidence and remaining gaps | Diff audit |

Every changed source file serves the origin fix or version compatibility.
No parser, transport, completion recovery, C9 rules, limits, model settings,
dependencies or installed application changed in the existing implementation.
The full clause list is untouched.
This documentation follow-up does not rerun code tests; its Rust tree must equal
the tested source tree above.

## Accepted revision: implementation and verification

The accepted revision was committed as `856937bc9db323df8d9c86baa4a403744e2bc99a`
before further source changes. This correction removes the historical hint path
I introduced in the previous implementation; it is one rework round.

- Removed both hint fields, all constructor assignments, extracted-draft lookup,
  equality/framing filters, General-only hint serialization, and obsolete hint
  test assignments. No historical hint-generation branch remains.
- Current General, Story and Contract system prompts remain byte-identical.
  No model, settings, decoder, schema cap, C9 rules or full-clause changes.
- The catalog regression now covers current and previous synthesis versions
  with short, long and mixed extraction drafts. Framing, split-section, source
  identity and saved-version checks remain.
- The regression failed before removal with `drafts cannot alter the source
  request`; afterward it passed. An earlier selector matched zero tests and is
  not counted as evidence.
- Adjacent coherent-summary tests: 97 passed, 0 failed, 11 ignored. Formatting,
  strict all-target/all-feature Clippy and diff checks passed.
- Two offline proof tests passed. The saved public A/B synthesis-13 views reopen
  identically from an isolated database copy. The actual current synthesis
  requests reproduce both persisted candidate semantic hashes exactly. The
  source databases are unchanged, no inference ran, and temporary probe code
  was archived and byte-restored.

Durable evidence manifest alias `pr122-hint-path-removal-20261005/manifest.json`,
SHA256 `6f7e2c6806d78bab409987d964a107b17090accbf5a447f3c75c803684ffe733`.
It identifies the fail-before/pass-after logs, offline proof, test and lint logs.

### Cold diff audit of the accepted revision

| File and line | Change | Contract / evidence |
| --- | --- | --- |
| `summary/coherent.rs:155` | Remove optional prompt hint field and its serializer assignments | Current saved request hashes unchanged |
| `summary/coherent.rs:2711` | Remove source candidate hint field and constructor copies | No hint storage or consumers remain |
| `summary/coherent.rs:7620` | Remove extracted-draft lookup and historical branch at origin | Fail-before/pass-after owner regression |
| `summary/coherent.rs:13940` | Extend regression to historical catalog; retain source/framing checks | 97 adjacent tests and saved-view proof |
| `docs/PR-GENERAL-SOURCE-ONLY-SYNTHESIS.md:1` | Record accepted scope, correction and evidence | Source unchanged after the tested edit |

Effect trace: historical drafts no longer enter requests because the catalog
and prompt types no longer carry hints; the historical regression fails before
and passes after. Current request hashes remain identical.

Boundary probe: absent drafts, short/long drafts, mixed exact/changed draft text,
and current/historical catalogs retain the same source evidence and schema.
The adjacent suite retains invalid-source, framing and unknown-version checks.

## Gap audit

NOT DONE for merge or full-document qualification. The accepted hint-path
removal and local proof are complete. Independent review of the new source
head, CI and reconciliation of the open oversight thread remain.

The earlier live A/B qualification remains red at comparison admission, and A
also retains decoder clipping. This cleanup does not change those outcomes or
clear PR116's fidelity hold. No new private qualification was run.

Next separate slice: reproduce the comparison enumeration/schema admission
constraint with a small public fixture and derive an origin fix while retaining
source ownership and bounded requests. Do not enlarge limits or tune generation
on the held private batch.
