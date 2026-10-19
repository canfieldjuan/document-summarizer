# Completed-section resume: backend admission belongs to work still to run

## Status, branch and scope

ACCEPTED directly by the operator: "I accept". Contract-only acceptance
recorded before source or test edits. Tracks issue #142. Use `codex/completed-section-resume`, based on
published writer head `b9de4f737bef82dfcca3f11f833a29214917df28`; later publish as
its own small PR with base `codex/c9-qualification-readiness`. Keep #132 frozen
and #141 separate. Do not merge this work into either branch. The specific
accepted #132 freeze supersedes the generic R3 branch-placement redirect.

One concern: resuming complete saved Synthesize responses must not contact the
backend for requests that this stage will not dispatch. Returning an unverified
Synthesized artifact is not delivering a summary. Verify still must admit and
check that artifact before any user-visible completion.

## Root cause and evidence from code

My original writer commit `0e8ce54b` introduced both backend dependencies:

1. `whole_document.rs:1333-1343` preflights every stored section, including
   Completed and Split history, before its state-based execution loop.
2. After `finalize_progress`, `whole_document.rs:1466-1475` calls
   `summary.rs::coherent_verification_exceeds_runtime_context:2124`. For the
   whole-document writer that builds the Verify plan and invokes
   `comparisons.rs::AdmissionPlan::preflight:488`, which contacts the backend
   for future Verify requests. Filtering the historical loop alone therefore
   cannot make saved-work Synthesize resume offline.

Native preflight tokenises through the server (`llama_cpp.rs:528-544`). Binding
validation (`whole_document.rs:824`) uses retained metadata and pure validation;
its checks stay mandatory. Analyze validation at `summary.rs:4228` checks source
and model identity, not backend availability. All backend calls must be counted
by the new regression, rather than making only Synthesize-stage calls fail.

The existing Verify owner already performs ledger-request preflight and
`plan.preflight` before generation (`summary.rs:1390-1420`). Do not remove that
gate. The earlier issue trace also shows that Synthesize failure changes run
state but does not delete its saved artifact row. The ordinary retry creates a
new run. This is code-trace evidence; the offline-resume failure has not yet
been reproduced end to end in a test. That is the first implementation step.

## Proven setup to reuse, and every departure

Reuse the writer stage fixtures, saved progress/response validators and
DB-reopen tests in `coherent/readiness.rs`, including
`whole_writer_stage_fixture`, `partial_progress_envelope` and the existing
persisted-Saturated recovery test. Do not make another assembly implementation.

Reuse the mandatory `scripts/c9-preproduction-parity.py` seven-owner offline
lane and its checked-in whole-writer baseline manifest. Retain the original
summary capture, source packets, C9/SAME60 verdict lists and native counter.
The native-summary manifest pins model SHA256
`cd76ec205963b3b33350093e6904d9de16c4e666fd104e1f632d25c7f15f2a13`, server SHA256
`0ca399edd758decd825a71823b04ba7ddbc8b2e10d2309d8bf623ee3c2283099`, context
32768, seed 4294967295 and captured output allowance 1500. Its frozen request,
framing/template and response hashes stay authoritative; this repair does not
run that model or change any of those settings. The writer's existing approved
differences from that capture remain recorded in its inherited contract.

Accepted departures:

- Historical Completed/Split section requests cease receiving backend
  admission. Pending requests still require admission; a Saturated parent's
  response is immutable, and its newly planned Pending children still require
  admission through the existing budget owner. Never redispatch the parent.
- Whole-document Synthesize stops preflighting future Verify requests.
  Their existing Verify-stage admission remains mandatory. This changes when
  a backend/admission failure appears: Synthesize may finish with an unverified
  saved artifact, while Verify remains unable to proceed without admission.
  Apply the same stage ownership to fresh and resumed whole-document synthesis;
  do not add a resume-only exemption or a hidden flag. Other synthesis policies
  keep their current behavior. This is an amendment to early Verify admission,
  not a claim that the old guard was purely local or that fidelity is waived.
  A future Verify admission failure is persisted by the Verify stage rather
  than aborting Synthesize; the complete synthesized artifact remains durable.
  Keep backend failure codes: MODEL_VERIFICATION_ADMISSION is the diagnostic
  context, not a replacement for the backend code. No extra generation.
  Verify admission remains the sole backend admission owner for Verify work;
  its existing gate is unchanged. The non-whole-document path through
  coherent_verification_exceeds_runtime_context, including the
  VERIFICATION_INPUT_TOO_LARGE fallback, remains untouched.

## Required change surface and non-scope

At `whole_document.rs::run_sections`, restrict the historical request preflight
to Pending work. Keep health checks for work still requiring a backend and keep
admission of newly split children. Remove the redundant future-Verify backend
preflight from Synthesize; retain offline complete-source, request/hash,
response/model, schema, lineage, profile/context, capacity and cancellation
validation. The actual Verify code stays unchanged unless the fail-first probe
reveals that it lacks the guard just traced; report and revise this contract
before expanding the surface in that event.

Tests belong beside the existing stage/DB-reopen regressions in readiness.rs.
No storage schema/migration, retry/reset behavior, request regeneration,
resume of Dispatched/Failed sections, model fallback, prompt/schema/framing,
sampler, model, limits, source-identity rules, Verify verdicts, user-visible
completion rule, benchmark labels or production adoption change. Do not edit
#132 or #141. Keep this PR one concern, roughly 1,000 changed lines or fewer.
No hook-tooling repair is part of it.

## Fail-first, proof and regression plan

After acceptance, first persist valid canned section responses using the real
progress owner, close/reopen SQLite, then call the real Synthesize entry point.
No live inference; all recorded bytes are public test fixtures.

- All Completed, and Split parents with all children Completed: a runtime with
  matching metadata whose preflight, health and generation methods fail/trip
  must still return and persist the same assembled unverified artifact.
  Assert zero backend calls across every stage, state Synthesized, no Verified
  or completed summary, and unchanged saved response bytes. Before the fix,
  record the observed failure and durable state/row; do not guess its code.
- Pending, plus mixed Completed/Pending: reject a Pending admission failure
  before generation. Historical completed requests are not preflighted and
  their bytes remain intact. Preserve original errors and failure persistence.
- Saved Saturated: the parent is never preflighted for redispatch or generated;
  children still require admission. Reject a child admission failure with zero
  generation; retain parent evidence and the existing split/CAS protections.
- After offline Synthesize completes, invoke real Verify with failing admission:
  it must stop before generation, produce no supported verdict and no delivered
  summary. This guards the moved stage boundary, not just the removed call.
- Tampered response/request/source bindings, mismatched model/profile/context,
  cancellation and Dispatched/Failed protections still reject. Reuse adjacent
  regressions rather than adding a second validator.
- Causal mutants: restore historical preflight; restore final Synthesize-side
  Verify preflight; bypass Pending/child admission; bypass Verify admission.
  Each must be caught by the corresponding public regression.

Then run adjacent readiness tests, formatting, strict Clippy and applicable
platform compilation. Required CI owns duplicated broad suites. Run the same
seven-owner frozen offline gate on the exercised source and compare full
reference/verdict/assembly results; explain every change and stop for unexpected
changes. Do not edit the baseline to obtain a pass. This slice is not final
combined-stack qualification; that remains on the eventual topmost head.

Store durable public proof under codex-evidence/document-summarizer and private
frozen reports under doc-classify-corpus/heldout, outside worktrees, with private
permissions. Publish one grouped repair push after verification; identify the
last reviewed base and new head for delta review. No merge before independent
exact-head review, required CI and clearance of the existing holds.

## Implementation and cold diff audit

Tested source `87f4992b60daf4a2ccbffaf4466af2bee8af2cc0` repairs my `0e8ce54b` at
`whole_document.rs::run_sections`. At line 1333 only Pending historical requests
receive preflight. Saturated parents remain immutable; existing child admission
and health checks stay in place. At line 1469 pure finalization and at line 1470
saved-artifact validation remain; the duplicate future-Verify call is removed.
The actual Verify gate and non-whole-document helper are byte-unchanged.

`readiness.rs:4600` adds public saved-progress fixtures, an all-stage backend
tripwire and six regressions at lines 4899-5051. It reuses the existing stage,
progress, response, assembly and DB owners. No existing test is weakened or
rewritten. No model, prompt, schema, sampler, framing, limit, storage, retry,
source-identity, workflow, dependency or script change.

## Proof and remaining gates

Both fail-first probes fail with PUBLIC_BACKEND_OFFLINE before the repair: one
at historical Synthesize admission, one at future Verify admission. The run is
Failed and the saved bytes are preserved. After repair, DB-reopened Completed
and Split-with-completed-children progress assembles and persists as Synthesized
with zero backend calls. No Verified or delivered artifact is created.
Pending/Mixed/Saturated admission, Pending health, both real Verify admission
kinds, and fresh-run stage ownership are tested. Verify failure preserves the
completed synthesized artifact and stops before generation or delivery.

82 readiness tests pass, 5 explicit adapters ignored. All seven causal mutants
are killed at their intended behavioral assertions, with source restored.
Strict Clippy, formatting and Windows GNU library/test cross-compilation pass.
Native Windows execution and broad duplicated suites remain required CI work.

All seven unchanged frozen owners pass with zero live model calls. Complete
dependency, capacity, assembly and SAME60 JSON files are byte-identical to the
published writer source. Complete reference classification delta: zero.
This is offline regression evidence, not combined-stack/live qualification.

Durable public alias completed-section-resume-repair-20261018/repair-proof.json,
SHA256 `de01377c11f3ca13f584075f4bc555552ab46c0834cbe2a609b940afa328fe49`.
Private offline receipt SHA256
`73ec78181ddb2f8d7835a9f365575faffdde79d6e873d1e3c1ccab22471242ca`.
This prose-only proof update reuses the tested source; code tests are not rerun.

**NOT DONE:** publication, required native CI and independent exact-head review
remain. All PR116, qualification, live, production, ready and merge holds remain.
No new model-run budget or production adoption is authorised.
