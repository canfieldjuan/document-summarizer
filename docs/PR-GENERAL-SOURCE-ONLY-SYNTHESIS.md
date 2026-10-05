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

Pending.

## Cold diff audit

Pending.

## Gap audit

NOT DONE: implementation, fail-first regression, compatibility verification,
live candidate confirmation and independent review remain.
