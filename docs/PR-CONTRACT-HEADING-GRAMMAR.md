# PR-B: Contract heading grammar at the clause reader

## Root cause

`summary/coherent/whole_clauses.rs` already admits decimal markers with an
optional section sign. It does not strip the explicit Section/SECTION keyword,
and its ARTICLE grammar requires the ordinal and title on one line. These
layouts reach the existing heading-like uncertainty path, causing abstention.
The term aliases are not the cause and will not change.

## Required change surface

- Extend the Contract line reader with Section/SECTION followed by the existing
  decimal marker grammar. Preserve section-sign support, decimal children,
  original byte ranges, parent/peer scope, and tab-as-whitespace recognition.
- Admit uppercase ARTICLE plus a canonical ordinal on one line, followed by an
  uppercase title on the next nonempty line of the same page. Lines may occupy
  separate blocks. Both lines belong to one article source heading. Cross-page,
  missing-title, mixed-case-title and malformed-ordinal cases remain uncertain.
- Use the existing inline ARTICLE ordinal grammar once for inline, split, and
  bare-Roman recognition. Remove the artificial ARTICLE/TITLE string round-trip
  used to validate bare Roman ordinals. Do not introduce a parallel number parser.
- Keep the existing three-way line classification, unit-boundary checks,
  TOC handling, parent tracking and boundary_uncertain signal. Selection consumes
  that signal; no text re-scan or alias changes downstream.
- Version Contract extraction3.3.0 with the expanded grammar. Saved3.2.0 uses the
  old grammar with new furniture; saved3.1.2 uses the old grammar and old furniture.
  Preserve the existing source-ID recipe. General and the PR-A owner stay frozen.

## Earlier rules and cleanup

Retain the single decimal-marker parser, component-length admission, ordinal
canonicalization, whole-unit admission, leading-title handling and uncertainty
propagation. Remove the synthetic ARTICLE/TITLE call for Roman validation.
Section/ARTICLE look-alikes remain the fallback for unsupported forms, not an
extra filter over new recognized headings. No category-specific patch is added.

## Non-scope

No new term aliases, inline lower-case prose inference, heading reconstruction
across page breaks, TOC redesign, full-list suppression, model/UI/schema change,
furniture-rule revision or v5 tuning.

## Assumptions and blockers

Authorized by PR111 comments5964081366/5964091129. This branch depends on frozen
PR118 at1a0eaabae330ffee73067e69ff7240b13c83804d and targets main. Its unique first
commit is contract-only. Review the PR118 dependency and this grammar increment
separately; after PR118 merges, the main diff contracts to this increment.

## Verification plan

1. Fail-first public Section parent/child and split-ARTICLE fixtures, checking
   actual selected source extents and complete retained source lines.
2. Negatives for malformed/oversized numbers, prose references, missing/mixed-case
   titles, TOC entries, split blocks/pages, continuation boundaries and nearby
   peers. Clean existing decimal/section-sign/inline ARTICLE forms stay correct.
3. Old3.1.2 and3.2.0 artifacts retain their exact behavior; unknown or forged
   versions are rejected. No alias or furniture-fingerprint change.
4. A-AH saved-source regression against both prior3.1.2 and frozen PR-A3.2.0;
   explain every inventory/selection change and review all newly selected extents.
5. Adjacent tests, strict Clippy and formatting. Freeze the combined candidate
   for independent review and the operator-supplied fresh v5; zero wrong labels.

## Implementation summary

Pending. This commit contains only the contract.

## Cold diff audit

Pending implementation and proof.

## Gap audit

NOT DONE: reproduction, implementation, regressions, independent review, lock
and combined unseen v5.
