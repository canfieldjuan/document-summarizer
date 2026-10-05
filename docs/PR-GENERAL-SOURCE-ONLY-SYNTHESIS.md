# General synthesis from exact source text

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

- Version new synthesis as 14.0.0. Keep 13.0.0 and older saved artifacts readable
  with their existing catalog, furniture, evidence and verification behavior.
- New General catalogs carry exact quotes, framing and governing clause context,
  without generated drafting hints. Story and Contract request content stays the
  same. Preserve the optional hint machinery only for historical catalog replay.
- Remove the current path's draft lookup and framing-dependent draft suppression;
  they remain within the historical branch because old artifacts need them.
- Regression at the catalog owner: supplying short, long, or mixed extracted
  drafts cannot change a new General request; source content and identifiers
  remain intact. Historical catalogs retain their previous hints and policies.

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

Verification plan: declared fail-first owner regression; source and historical
version tests; adjacent summary tests because version dispatch crosses the
summary artifact boundary; formatting and strict Clippy; exact public synthesis
request replay and a live candidate confirmation. Keep all raw evidence outside
worktrees with private permissions. CI owns duplicate broader suites.

## Evidence

Durable alias `public-synthesis-draft-guidance-20261005/summary.json`, SHA256
`4c46bef58db515517a5d45bd806f5607864ee4c9dd07e12d2ed673ec574b951e`.
Earlier homogeneous/varied isolated probes did not reproduce the defect.
Short-writing and field-order alternatives reduced cited-page coverage and were
rejected. The actual worker and exact request reconstruction supplied the public
reproduction; no private qualification input was used to tune the change.

## Implementation summary

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

## Cold diff audit

| File and line | Actual change and contract trace | Verification |
| --- | --- | --- |
| `summary.rs:51` | New synthesis version; retain historical 13.0.0 dispatch and budgets | Summary version and artifact tests |
| `summary/coherent.rs:7767` | Omit draft enrichment at the source catalog owner; preserve historical enrichment | Fail-before/pass-after catalog regression at line 13974 and public B live match |
| `summary/coherent/page_furniture.rs:353` | Keep 13.0.0 running-furniture behavior when current becomes 14.0.0 | Historical furniture tests |
| `summary/coherent/verification/tests.rs:687` | Accept saved 13.0.0 pairings; advance unknown-version negative case | Summary suite |
| `docs/PR-GENERAL-SOURCE-ONLY-SYNTHESIS.md:1` | Record scope, evidence and remaining gaps | Diff audit |

Every changed source file serves the origin fix or version compatibility.
No parser, transport, completion recovery, C9 rules, limits, model settings,
dependencies or installed application changed. The full clause list is untouched.
This documentation follow-up does not rerun code tests; its Rust tree must equal
the tested source tree above.

## Gap audit

NOT DONE for merge or full-document qualification. The draft-copying mechanism
has a fail-before/pass-after regression, controlled public replay, and actual
worker confirmation on public B. The live A/B qualification remains red because
both fall back at comparison admission; A also retains decoder clipping.
Independent review and CI for the published head remain required.

Next separate slice: reproduce the comparison enumeration/schema admission
constraint with a small public fixture and derive an origin fix while retaining
source ownership and bounded requests. Do not enlarge limits or tune generation
on the held private batch. PR116 remains fidelity-held.
