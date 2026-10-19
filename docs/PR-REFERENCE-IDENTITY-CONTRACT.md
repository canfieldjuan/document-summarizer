# Source reference identity: self names before outside-text classification

## Status and scope

Contract-only follow-up based on published writer head
`b9de4f737bef82dfcca3f11f833a29214917df28`. PR132 stays frozen there.
The preserved local proposal `f0bd7fb` will not be pushed onto PR132.
Use branch `codex/reference-identity-containment`, with PR base
`codex/c9-qualification-readiness`. Do not merge this branch into that base.

The operator directly confirmed the containment proposal in this coding chat:
**"Confirm outside-text rule"**. That policy is accepted. The operator also directly accepted the
self-alias scope: **"Accept self-alias repair"**. Both changes below are accepted.
No model calls are authorised.

## Root cause

`whole_clauses.rs::declared_source_name` recognises only the existing is-name
and is-called declarations. It loses a parenthetical self-name before
`dependencies.rs::owned_namespace_origin` resolves ownership. The caller then
uses the ExplicitDocument grammar tag to classify an unknown name as absent.
That tag identifies wording, not proof of another document's ownership.

Public reproduction using the existing source-measurement runner:
`This contract (the “Service Agreement”) governs the services described below.`
under SERVICE TERMS makes Section 3 of the Service Agreement External despite
local Section 3 being present. Change only the declaration to
`The name of this contract is “Service Agreement”.` and it resolves locally.
The complete report is the durable public alias-report.json under
reference-identity-assessment-20261018, SHA256
`5f9d3455e2eebc476199898d7bab714a720eb7a88150ebb031a1c07fcc97a285`.
This is observed behavior, not a model-capability hypothesis.

## Required change surface

One concern: preserve source identity before resolving a qualified reference.

1. In the existing source inventory, retain a parenthetical quoted self-alias
   in the opening region: `This contract (the “Name”) ...` and the existing
   document/agreement nouns; also `This <complete opening title> (the “Name”)
   ...`, requiring exact complete title identity before the parenthesis.
   Accept matched straight or curly double quotes. Use the existing lexical
   identity normalisation; never substring, fuzzy or semantic matching.
   A foreign noun phrase elsewhere in an introduction is not a self-subject.
   Body declarations must not become whole-document identities. Retain the
   opening provenance at the inventory owner; consumers must not rescan prose.
2. Resolve exact self-name/title/annex identities before the accepted fallback.
   Conflicting scopes, malformed targets and unclean owned targets still block;
   an ownership error must never fall through as absence.
3. With no exact owning scope, names only contained in titles/headings are
   External for the already established agreement/contract/document forms,
   with the existing outside-text disclosure, regardless of local numbering.
   Opaque names still block on partial ownership. Do not expand qualifier
   grammar, invent title/role lists or claim missing external contents.
4. Keep one identity owner and one fallback decision. Remove the superseded
   blanket containment blocker for established forms; preserve the opaque
   blocker. Extend the existing declaration reader rather than add a consumer
   alias scan or a downstream correction. Update the previous containment
   expectations explicitly; do not silently weaken unrelated tests.

## Proven baseline and differences

Reuse the inventory in PR132's accepted whole-writer contract and the checked-in
whole-writer-baseline-manifest.json. Its summary source/settings/framing pins,
C9/SAME60 evidence and frozen source inputs remain authoritative. No new runtime,
model, prompt, sampler, context limit, output limit or fidelity criterion.

The only proposed source differences are retained self-alias identity and the
accepted typed containment fallback. They change dependency classification;
record the complete per-reference delta against the published writer source.
The baseline demonstrates capacity/reproducibility, not complete-fidelity gold.
Reuse scripts/c9-preproduction-parity.py and its seven offline owners unchanged;
no substitute harness and no new generation. Do not change the manifest merely
to make a delta pass. Unexpected frozen dependency changes STOP publication for
isolation and review under the existing rules.

## Verification plan

Before the fix, add public fail-first tests demonstrating the parenthetical
false External and the accepted containment cases currently blocked. Isolate
failures separately. After the source repair prove:

- Internal Service Agreement alias and exact full-title shortened alias resolve
  their local governing clause; a separate agreement named by an amendment or
  body sentence never acquires the amendment's local numbering.
- Amendment, Terms-heading and containing-declaration established-form twins
  become External; opaque Master Lease partial ownership stays blocked.
- Existing exact title, explicit declaration and annex positives resolve;
  malformed quotes, body aliases, foreign-subject phrases, duplicate scopes,
  missing targets and unclean boundaries cannot yield an incorrect local bind.
- Mutants dropping alias retention, restoring established containment blocking,
  substring-binding titles or promoting body declarations are killed by tests.
- Run adjacent readiness tests, formatting, strict Clippy and the existing
  platform compilation gate. Run the unchanged complete frozen offline baseline
  on the topmost head and compare full reference/verdict lists, not counts.

Store durable public proof under codex-evidence/document-summarizer and private
source reports under doc-classify-corpus/heldout with private permissions.
Publication cites aliases and hashes. Independent review covers only the range
from the last reviewed base to this follow-up head. Keep this PR one concern and
under roughly 1,000 changed lines; split before publishing if it grows beyond it.

## Non-scope, blockers and gap audit

No Contract-mode/full-clause-list change, storage migration, UI feature, model
run, prompt/framing/schema experiment, capacity change, benchmark-label access,
new qualification, production adoption, ready claim or merge-hold clearance.
Unrecognised legal-name forms are not promised general semantic resolution.
Deferred packing/recovery/diagnostic hardening is not part of this PR.

## Implementation and cold diff audit

Tested source commit: `d532ee2d40e4230007fae13d6a7436efbae87010`.
The limited self-declaration reader was introduced in `125837df`; this repair
corrects that earlier implementation at its source, rather than adding a scan
in the consumer. The accepted fallback replaces the blanket established-form
containment blocker from `bbe2405`; opaque partial ownership still blocks.

- `whole_clauses.rs:844` / `declared_source_name:1063`: retain opening titles
  while consuming the existing line inventory; read the accepted quoted
  self-alias forms and retain existing opening/body provenance. Clause text,
  splitting and full-clause-list construction are untouched.
- `whole_clauses/dependencies.rs:667`: receive the existing typed namespace;
  exact identities still own scope, weak title/heading containment blocks only
  opaque names. Reuse the existing noun classifier and identity normaliser.
  Owned errors do not fall through to External. No second identity parser.
- `readiness.rs:3902`: public self/foreign, exact/partial/opaque, quote,
  body-provenance, missing/duplicate/conflicting/uncertain-target twins.
  Earlier explicit containment expectations now follow operator acceptance;
  opaque and exact-identity controls remain.

## Proof and remaining gates

Both registered public fail-first probes failed in the intended way before the
repair. Original hash-bound packets now resolve both self-name forms locally
and distinguish the separate agreement from the document's own shortened name.
Readiness: 80 passed, 5 ignored. Four causal mutants were killed (alias removal,
blanket containment blocking, substring title binding, body declaration promoted
to whole-document identity), with source restored. Strict all-target Clippy and
formatting pass. Windows GNU library/test cross-compilation passes; native
Windows CI execution is still pending. Broad suites are left to required CI.

All seven unchanged frozen owners pass with zero model calls. Full dependency,
capacity, assembly and SAME60 JSON files are byte-identical to the published
writer source; complete reference classification delta: zero.

Durable public alias: reference-identity-repair-20261018/repair-proof.json,
SHA256 `fca6755fc36e873a02370bde032304ab3935e4728fc9f93106995aab7337f520`.
Private offline receipt SHA256
`926e73f76158cccf03b8894601d055f4521440575538f3b8323b8a163356c0cd`.
Source-only audit: no script/workflow, dependency, storage, model, prompt,
schema, sampler, framing, fidelity or capacity change. No tests rerun for this
prose-only proof update; receipt binds the unchanged exercised source bytes.

**NOT DONE:** source repair and offline proof are complete; publication,
independent exact-head review and native CI remain. All qualification, live,
production, ready, PR116 and merge holds remain. Acceptance covers only the
self-alias grammar above, not general legal-name interpretation or a model run.
