# Permission diagnostic STOP

## Scope and observed result

Oversight issuecomment-6131605488 cleared the admission-seam amendment but required the obligation diagnostic to withhold. If it did not, the trial must stop. This checkpoint records that STOP; it does not amend acceptance or implement the permission repair.

At base 7e2c1b6234f27e5240d19ba209b238d5e87515c4 plus the preserved test-only patch, `cargo test --offline --lib permission_grant_diagnostics_report_actual_final_verdicts -- --nocapture` exited 101. The actual semantic-guard verdicts were:

| Owned evidence | Claim | Actual final verdict | Disposition |
|---|---|---|---|
| Apprentices carry crates. | Rules permit apprentices to carry crates. | Supported | Report-only preexisting gap; future-contract issue required |
| Apprentices must carry crates. | Rules permit apprentices to carry crates. | Supported | Required withhold failed; STOP |

Both fixtures inject the initial Supported verdict and call the production `apply_semantic_fidelity_guards` owner. This proves failure to downgrade a hypothetical wrong model verdict. It does not prove that a live model would approve either claim or that either reached a delivered summary. No generation, delegated preflight, or GPU use occurred.

## Reproduce, isolate, explain

The public single-sentence obligation fixture is the minimal reproduction. The permission qualifier implementation had not started.

Code trace, against the frozen current policy (line references in the preserved source):

- `semantic_support.rs:3473`: explicit permission claims receive an empty projection when sources have no permission/may. Neither diagnostic source yields a permission proposition, so this path returns `Some(Vec::new())`; that is no objection, not a support proof. The branch belongs to my earlier afabcb9e change.
- `semantic_support.rs:3358` calls `coherent.rs:6351` to construct an ordinary assertion with all preceding context as its subject. For the claim's `carry`, the subject context is `rules permit apprentices`; for the source's `must carry`, it is `apprentices`. `coherent.rs:6435` requires equal subject and object contexts, so they cannot match.
- `semantic_support.rs:3680` skips the source modal when no aligned assertion was recognized. Thus it never reaches the dropped-obligation rejection. This recognition/skip path belongs to my earlier 1abee71c change. Blame identifies the responsible edits; no historical runtime bisect was performed, so the earliest occurrence of the whole failure class is not claimed.
- `semantic_support.rs:3731` keeps an initial Supported verdict when these guards do not reject it. The diagnostic exercises that final owner, rather than only a helper.

The broken invariant is preservation of the proposition's actor/action identity when a claim wraps it in an explicit grant. The existing generic context reader includes the grantor and grant verb in the actor context, while the permission projection is unavailable for obligation sources. A downstream verdict special case would compensate for this lost identity. Any revised direction should address proposition ownership before permission or modal comparisons, and must inventory all readers and remove redundant projections. This is a root-cause note for independent review, not authorization to implement it.

## Fix, prove, prevent regression

Fix: not performed; explicit diagnostic stop rule applies. Production policy bytes from the first post-test type (`NumericRelation`) onward are identical to f7ca3f5. The remaining source diff is inside test modules and the approved fixture metadata.

Prove: no passing result claimed. The preserved regression is deliberately still failing. The amended production-admission test was updated but not rerun after STOP. No new permission matrix, mutation, frozen-60 inventory, C9/A1 replay, or broad-suite result is claimed.

Prevent regression: preserve the public diagnostic and its exact sources for the next independently cleared direction. Do not weaken the obligation expectation or relabel it as a precision cost. All PR116, production, qualification and merge holds remain. No policy push.

## Durable evidence

Directory alias: `c9-guard-origin-20261011`.

- `qualifier-diagnostics-before.log` sha256 `891ce63f89743dfdea73c6bfe870d620677c0369dbaf4622c6298de41b11c01e`.
- `qualifier-diagnostic-stop-proof.json` sha256 `1b492f4233c5e07ef403c5c8ab171571df2e93bb52a97507760bac377ad28abf`.
- `qualifier-diagnostic-stop.patch` sha256 `19d13125cffb476ec3add47f285f0150f95021b32011b8c5656ce3b4a0008e7f`.

The packet also hashes the complete source and fixture snapshots. Evidence lives outside the worktree.
