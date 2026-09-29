# Qwen3.5-9B production preset (#71 / #100)

## Root cause

The previous default selected the 30B Ollama profile. The registry had no exact
9B entry; GGUF descriptors assumed 27.3B/IQ2_M, and raw completion framing did
not close the thinking block. A server chat-template option does not change
raw-completion framing.

The operator subsequently requested 32,768 tokens for the 9B. Raising the server
allocation alone had left the coherent source-character cap in place. Main now
owns the shared coherent budget and source-catalog fixes from the separate
reviewed slices. This branch composes those changes and changes the 9B context
at the existing profile owner.

Qualification also exposed two recorder defects: the selected-profile recorder
lost concrete stage identities, and the office recorder inherited permissive
preflight instead of forwarding runtime admission. Both could make tests behave
differently from production. Their regressions reproduce the missing identity
and missing context rejection before the recorder fixes.

## Required change surface

- `model_settings.rs`: make `full-qwen35-9b-q4km-v1` the fresh-settings default.
  Pin Qwen3.5-9B Q4_K_M SHA-256
  `cd76ec205963b3b33350093e6904d9de16c4e666fd104e1f632d25c7f15f2a13`.
  Analysis and verification use the same profile and 32,768-token context.
  Model metadata and thinking policy belong to the registry entry. Preserve
  the existing checksum-pinned managed llama.cpp binary and libraries.
- `llama_cpp.rs`: append `<think>\n\n</think>\n\n` after the assistant marker for
  this profile. Tokenize trusted framing with special-token parsing, keeping
  source text untrusted. Preflight and generation use identical framed tokens.
  Include thinking policy in the runtime cache identity. Preserve old framing
  for the older profiles.
- Shared test-only live selection: register the explicitly supplied GGUF in
  private temporary settings and call the production default-profile factory.
  Keep that directory alive for the run. Explicit settings/candidate selection
  remains possible; never silently substitute the old 30B verifier.
- Office and selected Story/Contract recording wrappers forward preflight and
  stage identities. Existing office coverage, exact-quote, modal, persistence
  and reopen assertions remain unchanged. Record the exact recovered-page count.
- Existing automatic-purpose and selected-profile tests, the release acceptance
  script, README and office instructions use the production default selection.

## Profile routing and persisted settings

Automatic already maps agreements to Contract and narratives to Story.
Informational, mixed, other and unknown map to General. Distributed uncertain
samples get one expanded inspection; invalid responses fail and ask for explicit
selection. An explicit user selection bypasses Automatic and remains authoritative.
This slice qualifies those existing paths; it adds no new classifier or UI.

Saved selections remain unchanged. There is no settings-version bump or migration:
the operator confirmed no deployed settings need migration. Missing/wrong model,
runtime or checksum leaves the selection unavailable instead of substituting an
unselected model. The older explicit presets keep their existing contexts.

## Explicit non-scope

Shared-host extraction, downloads, installer work, runtime replacement, Windows
GGUF enablement, installed settings, summary prompts/schemas/validators, OCR,
acceptance-threshold changes, layout follow-ups and hardware-floor claims.
The broader cross-app checks remain a later slice. No new 30B inference is part
of this requalification; its authorized prior comparisons are reconciled below.

## Assumptions and blockers

The exact model and runtime must pass production checksum verification without
overrides. Live qualification requires the existing inference lock and an
exclusive GPU. Reuse the owned loaded server where possible. The direct runtime
is Linux-only; Windows compilation does not prove Windows model execution.

A passing fallback proves validated delivery and disclosure, not generated-summary
quality. Keep every failed or incomplete attempt visible. Model-quality failures
cannot be fixed by weakening acceptance gates in this slice.

## Verification plan

- Fail first when fresh settings report 8,192 instead of 32,768; verify catalog,
  analysis/verification snapshots and stage runtime use the profile value.
- Probe absent/zero/below/at/above context limits, and reject wrong digest,
  tokenizer family and backend independently. Preserve old selections.
- Reproduce office preflight rejection being swallowed, then prove both accepted
  and rejected requests pass through without generation. Preserve stage-identity
  rejection and untrusted-token framing checks.
- Run adjacent settings/runtime/routing, coherent-budget and office checks,
  format, strict clippy and frontend build. CI owns the broad platform suites.
- Freeze production source and record the private qualification wrapper patch.
  Reuse the existing unchanged office assertions for contracts A/B in General
  and Contract modes, the structured fixture and public DOL General fixture.
  Run the existing Automatic counterexamples and selected Story/Contract lanes.
- Capture source/model/runtime/binary identities, requests, raw responses,
  request-attempt diagnostics, delivered artifacts, warning/failure reasons and
  durable reopen checks. Verify actual 32,768 allocation and exclusivity. Report
  observed peak GPU allocation and server RSS with sampling limits.
- Inspect rendered outputs separately from mechanical pass/fail. The raw
  completion API has no separate reasoning-token counter; report observed
  response form rather than inventing that counter.

## Implementation summary and cold diff audit

The registry owns the 9B identity, default, metadata and 32k stage context.
The direct runtime owns thinking framing and cache identity. Test-only shared
selection uses the same factory as the app; both recorders preserve its relevant
identity/admission behavior. Documentation and the release script describe that
selection. No production summary behavior is added by this preset diff.

Fail-first context and preflight regressions reproduced their expected failures.
After the fixes: 25 settings checks, 39 runtime/routing checks, seven budget
checks and five office checks passed. Formatting, strict all-target/all-feature
clippy and the frontend build passed. These are local deterministic checks;
the opt-in live checks are separate. Earlier failed live and build attempts are
retained in private evidence, not erased or counted as successes.

## Gap audit

NOT DONE for merge: the current B/General run fails before delivery. Five of six
selected-mode document cases pass mechanical gates, but four use fallback.
Automatic A/B both route correctly and also deliver fallback. These results do
not qualify generated summaries on the real contracts. Public receipts
use only opaque A/B labels and aggregates. No private source names or paths
belong in Git. Merge this PR by squash only because earlier branch history
contained a subsequently removed private document name.

## B / General output-stop classification (fresh 32k reproduction)

The original failed ModelRuntime call retained usage and failure diagnostics but
discarded the partial body. A separate identical-request reproduction on the same
production source, model, runtime, context, schema, seed and 2,048 output limit
captured decoded content immediately before the unchanged rejection. The capture
is test-only in a private export; no production instrumentation was added.

Classification: legitimately long, advancing source material, with independent
unit-clipping and early-source coverage defects. The new response used 20,395
input / 2,048 output tokens, with `stop_type: limit` and no input truncation.
Seven complete JSON units were distinct, followed by a partial eighth. The 64
emitted source IDs were unique and advanced through the first 64 segments,
covering pages 1-7 of 14. Content progressed through identity/incorporation,
reciprocal rights, contractor information duties, subcontractor performance,
progress/payment duties, compliance, safety and tools. There was no looping unit
or repeated-source cycle in this reproduction.

Two complete units reached exactly 1,200 characters and ended mid-sentence;
other units also ended incomplete. More output room could finish the JSON, but
would not by itself repair those units or cover the later pages. #95 remains a
supported budget/completion follow-up, not a promise that increasing one constant
qualifies the model. Preserve the source/coverage/unit-completeness gates.

The reproduced text SHA-256 is
`1d1c5081e987e60662d31fde7082a7c525db365ea1a5056f864754cc84302cb1`.
Because the original partial body is unavailable, this classification applies to
the identical-request reproduction; byte equality with the original is not claimed.
Raw text and client identity remain private.

## Current-source qualification receipts

Exercised production source is `a192521a038bd73166d00cbd1df6758e84c5b3a8`.
Subsequent changes in this PR are documentation only. The private wrappers retain
the existing office assertion body, use the production default-profile factory,
and capture requests, responses, failures and durable artifacts. Every attempt
remains recorded. Actual 32,768 context, unchanged inputs and exclusive GPU use
were verified; installed settings were not changed.

| Document | Selected mode | Delivery gate | Delivered form | Text pages cited |
| --- | --- | --- | --- | --- |
| A | General | PASS | claim-ledger fallback | 10/10 |
| A | Contract | PASS | claim-ledger fallback | 10/10 |
| B | General | FAIL | No delivery | 0/14 |
| B | Contract | PASS | claim-ledger fallback | 14/14 |
| Structured fixture | General | PASS | coherent | 5/5 |
| Public DOL fixture | General | PASS | claim-ledger fallback | 83/111 |

PASS means unchanged exact-evidence, coverage, persistence and reopen gates;
page coverage is not material-term completeness. A/General and DOL generated
invalid drafts and used disclosed fallback. Contract A/B made zero Contract
generation calls because source-catalog admission rejected the incomplete
catalog. The structured fixture's omission/recovery warnings remain visible.
The original B failure is retained separately from its raw-output reproduction.

Automatic counterexamples and the selected synthetic Story/Contract lanes also
passed. Inspection found the fixture's sourced events, terms, amounts and
exceptions preserved; that small synthetic proof does not qualify real prose.

### Real-document Automatic path

Fresh A and B each returned `documentPurpose: agreement`,
`suggestedProfile: contract`, `sampling: distributed` with one suggestion call.
Neither needed an expanded pass. The wrapper calls the production preparation,
parsing, normalization, structure and suggestion functions, then passes the
suggestion's source hash and selected profile to production ingestion. Both
persisted Contract, completed delivery and reopened identical artifacts.

Their final output was claim-ledger fallback, citing 10/10 and 14/14 text pages.
Both disclosed `COHERENT_SUMMARY_SOURCE_CONTEXT_TOO_LARGE` and quote-boundary
warnings; B also disclosed its image-only page. Neither generated a Contract
overview. This qualifies Automatic routing and durable delivery, not generated
Contract prose or installed-GUI operation.

The Automatic result receipt SHA-256 is
`4b50213b79f9e82d95b5eae72c897e0e3a1f214e8ec553a92ddcf40f97712729`.

## Historical preflight audit

Offline replay covered twelve receipts on their original production revisions,
using saved responses and restoring saved document/run identities in new local
databases. It regenerated 353 matching requests and observed every pipeline
preflight invocation, including admission before generation. All 63 preflights
and all sent requests fit. Therefore forwarding preflight would not change the
audited request sequence or source selection. No model inference ran.

The shadow recorder preserved the original no-op return while separately
evaluating actual admission. Zero differing decisions establishes equivalence
for these runs; a rejection would instead have marked the receipt changed.
The 30B check uses the historical production Ollama admission method, with its
fingerprint-verified tokenizer read from the installed model. The 9B check uses
the fingerprint-verified tokenizer and the framing count previously calibrated
against all 115 saved 32k requests. No new 30B inference or server was started.

| Receipt | Source revision | Context | Requests | Preflights | Verdict |
| --- | --- | ---: | ---: | ---: | --- |
| 9B A, original comparison | `584bf0f` | 8192 | 26 | 7 | Unchanged, reconstruction limit below |
| 9B B, original comparison | `584bf0f` | 8192 | 42 | 12 | Unchanged, reconstruction limit below |
| 30B A, original comparison | `584bf0f` | 8192 | 26 | 7 | Unchanged, reconstruction limit below |
| 30B B, original comparison | `584bf0f` | 8192 | 42 | 11 | Unchanged, reconstruction limit below |
| 9B A/General, context-only experiment | `584bf0f` | 32768 | 26 | 7 | Unchanged, exact request replay |
| 9B B/General, context-only experiment | `584bf0f` | 32768 | 41 | 12 | Unchanged, exact request replay |
| 9B A/Contract, context-only experiment | `584bf0f` | 32768 | 19 | 0 | Unchanged, earlier catalog fallback |
| 9B B/Contract, context-only experiment | `584bf0f` | 32768 | 29 | 0 | Unchanged, earlier catalog fallback |
| #104 initial A/General | `aa2def4` | 32768 | 19 | 1 | Unchanged, same invalid-unit failure |
| #104 initial B/General | `aa2def4` | 32768 | 33 | 4 | Unchanged, same fallback |
| #104 final A/General | `7f387b5` | 32768 | 20 | 1 | Unchanged, same fallback |
| #104 final B/General | `7f387b5` | 32768 | 30 | 1 | Unchanged, same fallback |

Reconstruction limit: the original 8k logs retained answers and request
shape/lengths, not full original request bytes. Earlier original-source replays
reconstructed their prompt text/schema. This audit matched those reconstructed
requests and outcomes. Per-run block IDs differ from the original live import,
but none occurs in these source-selection, synthesis or verification preflight
prompts. The original Ollama seed is unavailable; admission also passed with
the maximum signed seed length, using the Qwen3 single-digit tokenizer rule.
This supports unchanged preflight decisions, not byte-identical live requests
or a new inference comparison. The eight later receipts retain full requests,
including seed and source IDs, and were matched exactly.

Other receipts and dispositions:

- #106's four A/B General/Contract runs at `d5a4760` did not use the defective
  office recorder. Their frozen private recorder already forwards
  `preflight_request`; its file hash matches its source manifest. These remain
  unaffected, with all four delivering disclosed fallback. This is not a claim
  that that earlier live source includes every later #106 review edit.
- Selected Story/Contract receipts use the coherent module's separate recorder,
  which already forwards preflight. Automatic uses its direct runtime. They are
  unaffected by this omission; both lanes were also rerun on the current source.
- No successful archived invocation of the entire
  `profile-release-acceptance.sh` was found in this lane's evidence. Its General
  step uses the office recorder and was susceptible; its selected profiles were
  not. The current fixed-recorder public DOL run and the other constituent tests
  were executed separately. Do not describe this as a recorded whole-script run.
- Earlier structured/real-office diagnostic receipts without full request
  capture, and the superseded intermediate coverage runs, are not being certified
  as production-comparable. Their failures remain in the record; their old pass
  claims are superseded by the current fixed-recorder matrix. The extra context
  attempt whose ownership monitor missed execution remains unqualified as before.
- #104's response-remapping replay was a deterministic recovery probe, not new
  inference. Its production claim rests on the final live A/B receipts audited
  above, not on the replay's permissive runtime.

Decision impact: no audited preflight decision changed, so the observed #102
fallback result and #104 admission/recovery findings are not invalidated by this
recorder bug. They remain revision-specific. The historical comparisons do not
establish 9B quality superiority over 30B: both delivered fallback, and retained
prose coverage favored 30B. The operator's smaller-model direction remains a
candidate choice requiring qualification; #100 is not model promotion approval.
Any older un-auditable receipt is excluded from that decision. Current real
Automatic and selected-mode results above supersede it for this candidate.

Audit receipt SHA-256:
`dc1762307cda024c1c03acc8dbb8d0350587770865c799910e8512335550217f`.
The private audit preserves source manifests, replay traces, tokenizer identity,
test binaries and the initial helper-build/request-identity failures. It changes
no production code, installed settings or original evidence databases.
