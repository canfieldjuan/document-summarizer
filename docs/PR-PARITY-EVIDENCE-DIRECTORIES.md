# Accepted parity evidence-directory repair

Operator acceptance: "i accept", responding to PR116 root note comment6048832533.
Frozen proposal SHA256: 934665bf3072cf58cab56b47f08f31f71a27ee9a71c3fe95dac06f3e8dcc6770.
This acceptance record precedes source and regression edits. The proposal below is retained verbatim; its proposed status describes the frozen record before this acceptance. Implementation must satisfy its required surface, admission policy and verification gates. PR116 admission and qualification holds remain.

# Parity evidence-directory origin repair

Status: PROPOSED; user "ok, lets start there" authorizes investigation of the identified permissions finding. Rule21 requires this concrete root note accepted before another implementation/consolidation. No model calls or production changes authorized here.

## Root cause
At source456fa8aa9ee5e677997a3c46132c86e583777326, summary_lane at scripts/c9-preproduction-parity.py:267-268 creates an output then changes its existing parent. I introduced it in f55dd606024a5ecf4a1fa4a6eea01a04510cae98. It confuses ownership of a newly created run directory with ownership of its ancestor. Permission denial occurs after an irreversible no-retry path marker has been created even though no call budget was reserved. The second main harness path at :346 creates a leaf directly; separate creation paths permit inconsistent privacy/preparation policy.

## Reproduce / isolate / explain
Public actual summary_lane probe, with input-pin checks substituted and Popen blocked: existing parent mode2775 becomes0700; denied parent chmod leaves output_exists=true. Both expected regression assertions fail, two tests, generation0. Alias parity-output-permissions-20261007/reproduction.json SHA256 a0025728e2f746168ee0d67c5c98414fdceaf70930e4dee8891a5cc728e4ba81. No real metadata/source text/model/server call or existing operator-directory permission was modified by this probe. Temporary fixtures only.

## Required change surface / one owner
Only scripts/c9-preproduction-parity.py and scripts/test_c9_preproduction_parity.py, plus a separate acceptance-first repository contract record.
One prepare_evidence_output function owns path admission, parent validation, leaf creation and preparation cleanup for summary and existing native/preflight/gateway runs. Every caller uses it before freeze files, fixed budget reservations or subprocess launch.
- Require output beneath the existing durable public document-summarizer evidence namespace or private heldout namespace, outside all worktrees. Do not reinterpret arbitrary /tmp outputs as durable evidence.
- Require an existing immediate parent directory, owned by the current user, mode exactly0700. Validate before creating anything. Reject shared/missing parents without chmod or ancestor creation. Operators prepare a dedicated private slice folder first. Never chmod an existing parent/root.
- Create only the new run leaf exclusively (existing outputs, including symlinks, remain rejected). Set exactly0700 only on the leaf created by this invocation, including under restrictive umask. If that permission preparation fails, remove only this owned empty leaf and propagate the error. No freeze, reservation or launch occurred, so later corrected preparation is permitted. Once freeze/reservation/generation begins, existing no-retry behavior is preserved.
- Remove recursive mkdir(parents=True) and parent.chmod; route the second raw mkdir through this owner, not a second copy. Keep the established token/schema/prompt/runtime/provenance and budget rules unchanged. Do not add a later output filter or reset/delete old evidence/reservations.

## Verification plan
Before fix, committed regression inputs must reproduce both observed classes against the actual summary_lane before any generation. Public fixtures substitute only validation/input pins and block Popen. Valid private output must be admitted; shared2775/1777/0750 parents remain unchanged and are rejected before creation; missing parents, outside-namespace paths, worktree destinations, existing files/directories and broken symlink outputs must refuse without mutation. Denied leaf chmod leaves no new output or budget; restrictive umask still creates exactly0700; original parent modes never change. All four phase routes consume the shared owner. Existing receipt/budget/no-retry tests remain.
After fix: same public original cases have unchanged parent and no stranded output on permission denial; original shared parent may now be explicitly refused by private-parent admission. That admission difference must be recorded, not hidden as a byte-identical scenario. Run the existing Python test file and relevant Python syntax/diff checks. No Rust/full suite to duplicate green CI for Python-only tooling changes. One consolidation push, then direct thread reply/resolve only after pushed regression proof. New exact-head review/CI and all PR116holds remain.

## Explicit non-scope
No model calls, qualification reset, acceptance-receipt reset, output reuse, model/prompt/settings/profile/launch/decoder/native changes, dependencies, workflows, installed apps/services, host migration, Email/Invoice work, merges or hold releases. No general file-system race-proofing project; this owner is a local harness privacy and mistake guard, not protection against a hostile same-uid operator.

## Assumptions/blockers
Explicit Rule21 root-note acceptance is required before source/tests edits. Durable namespace and private parent are intentionally strict; no auto-chmod of a shared root. Unexpected compatibility requirements require a contract revision. Existing output failures after reservation remain evidence and cannot be retried.

## Implementation summary
Reproduction and source-owner inventory only; no tracked source/test edits. Proposed acceptance record precedes implementation once accepted.

## Cold diff audit
No production diff. Both creation readers identified, introducing commit traced, repeated output-creation/privacy policy consolidated in proposal. Failed public evidence remains durable.

## Gap audit
NOT DONE: root-note acceptance, implementation, passing regression, publication and review. No live model run needed or authorized. Admission and operator/native qualification holds stay open.
