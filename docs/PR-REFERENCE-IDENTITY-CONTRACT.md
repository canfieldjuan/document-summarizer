# Source reference identity: self names before outside-text classification

## Status and scope

Contract-only follow-up based on published writer head
`b9de4f737bef82dfcca3f11f833a29214917df28`. PR132 stays frozen there.
The preserved local proposal `f0bd7fb` will not be pushed onto PR132.
Use branch `codex/reference-identity-containment`, with PR base
`codex/c9-qualification-readiness`. Do not merge this branch into that base.

The operator directly confirmed the containment proposal in this coding chat:
**"Confirm outside-text rule"**. That policy is accepted. The additional
parenthetical self-name grammar below is PROPOSED pending direct acceptance.
No source or test implementation precedes that acceptance. No model calls.

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

**NOT DONE:** direct acceptance of the proposed self-alias grammar is pending;
source/tests, fail-first and repaired proof, frozen delta, cold diff audit,
publication and independent exact-head review remain. Containment acceptance
alone does not authorise a different alias grammar by implication.
