# Office Document Acceptance

This acceptance pass complements the deterministic unit suite with public,
real-world PDFs that resemble documents a small office may receive. The files
are downloaded into ignored `tmp/pdfs/office-acceptance/`; they are not copied
into Git, and no private customer document or OAuth material is required.

## Public corpus

| Document | Source | SHA-256 | Purpose |
| --- | --- | --- | --- |
| Form W-9 (Rev. March 2024) | `https://www.irs.gov/pub/irs-pdf/fw9.pdf` | `2d420cbb4123dcf1fb82595b2359cfbb5d81f00b9df9d359fcc7af361d093f53` | Six-page fillable tax form with dense instructions and line wrapping |
| Federal minimum-wage poster | `https://www.dol.gov/sites/dolgov/files/WHD/legacy/files/minwagebw.pdf` | `4ed7da20aefc976733f087244818dcd109fff0d96f6a2be40743e3b59c674712` | Rotated, visually dense one-page poster |
| Independent-contractor agreements | `https://www.grandcountyutah.gov/DocumentCenter/View/235/Agreements-for-Independent-Contractors-PDF?bidId=` | `8a849b7ebc5eb1f6a635d2a2e5b083033b74beea2bd47939c56551f73cd59013` | Eight-page office agreement packet |
| NARA records schedule | `https://www.archives.gov/files/records-mgmt/rcs/schedules/departments/department-of-defense/office-of-the-secretary-of-defense/rg-0330/nc1-330-78-07_sf115.pdf` | `290840e408f2769b9a0ed65b73aa15c116cae3685f125358717ad15cfbd29ec8` | Twelve-page scanned/OCR robustness fixture; delivery and warnings only, not table-accuracy evidence |
| DOL workplace training deck | `https://www.dol.gov/sites/dolgov/files/WHD/legacy/files/FLSA-MSPA-H2A-091925-Public.pdf` | `12097e00b956e8f387e2cd43dd609a9cecc1ca1580c32cc3b87b60518307382b` | 111-page size/page-count stress case |

Source hashes must be checked after download. A changed upstream document is a
new corpus revision and must not silently inherit these observations.

## Reusable harness

`src-tauri/tests/office_acceptance.rs` contains three ignored tests:

- `office_pdf_deterministic_checkpoints_survive_reopen` accepts one or more
  platform-delimited paths in `DOC_SUM_OFFICE_PDFS`. It verifies unchanged
  source bytes, page topology, visual-routing markers, 100% normalized-block
  coverage, artifact equality, ordered events, state/version, and an independent
  SQLite connection reopen through `CHUNKED`.
- `office_pdf_live_ollama_analysis_satisfies_evidence_contract` accepts one path
  in `DOC_SUM_OFFICE_PDF` and isolates live evidence extraction.
- `office_pdf_live_ollama_summary_has_exact_durable_evidence` runs the complete
  selected-model pipeline and verifies exact quotations, provenance, integrity,
  terminal state, immutable events, unchanged source bytes, and artifact
  equality after an independent reopen. It requires at least 50 percent raw
  native-text-page coverage and 60 percent material-omission-adjusted coverage.
  Technical paraphrase omissions remain in both denominators. The known NARA
  source hash must also deliver `OCR_TEXT_LAYER_STRUCTURE_RISK`.

Raw model responses and summary text are hidden by default; reports contain
only lengths, hashes, request schema/size metadata, and source-page sets.
Setting
`DOC_SUM_OFFICE_TRACE_MODEL_RESPONSES=1` opts into displaying them for a known
non-private fixture. `DOC_SUM_OFFICE_TRACE_SOURCE_CONTAINS` displays only nearby
source lines containing the requested phrase.

Example from `src-tauri/`:

```bash
DOC_SUM_OFFICE_PDFS='/absolute/a.pdf:/absolute/b.pdf' \
  cargo test --test office_acceptance \
  office_pdf_deterministic_checkpoints_survive_reopen \
  -- --ignored --exact --nocapture
```

On Unix the list separator is `:`; Rust uses the platform's native path-list
separator, so Windows uses `;`.

## Combined summary-profile release acceptance

`scripts/profile-release-acceptance.sh` runs the four live profile gates in a
fixed order against the configured Ollama model:

1. automatic routing across an agreement, a contract article, a legal story,
   an anecdotal report, and mixed content;
2. bounded long-Story selection and source-bound synthesis;
3. bounded long-Contract selection and source-bound synthesis; and
4. the complete persisted long-General pipeline on the public DOL training
   deck.

The script accepts only the documented DOL deck whose SHA-256 is
`12097e00b956e8f387e2cd43dd609a9cecc1ca1580c32cc3b87b60518307382b`.
That fixed public input makes it safe for the script to reveal the General
summary needed for semantic review. It also requires the selected model to be
preloaded in Ollama at `100% GPU` on the exact loopback endpoint
`http://127.0.0.1:11434/v1/`; a different endpoint, missing model, CPU offload,
or wrong fixture fails before any Rust test runs. The command runs the
TypeScript/Vite production build itself and stops if current frontend sources do
not compile. Binding the checked endpoint to the endpoint used by the tests
prevents a local GPU process from masking execution against another server. The
command also rejects an inherited `OLLAMA_HOST`, product model-settings path, or
qualification-model override because those variables can redirect the CLI
check or the General integration harness away from the runtime being accepted.

From the repository root, after `npm ci`:

```bash
scripts/profile-release-acceptance.sh \
  /absolute/path/to/dol-workplace-poster.pdf
```

The test results mechanically check routing enums, schema parsing, bounded
selection, source IDs, exact quotations, profile-specific validation,
citations, durable artifacts, and warnings. A passing command does not by
itself establish that the generated wording is useful or semantically
supported. Review the printed source and summaries for:

- General: the main message, important supporting points, qualifications, and
  whether headings or surrounding context change the meaning of a sentence;
- Story: chronology, causality, character identity, conflict, resolution, and
  invented motivations; and
- Contract: parties, obligations, conditions, exceptions, dates, amounts,
  clause references, and invented legal conclusions.

Record the representative-output judgment and any defect in the canonical
build ledger. A semantic defect gets its own repair slice; do not change product
behavior while running this acceptance gate.

These live tests are complementary rather than one native-desktop interaction.
The automatic case ends after classification, the Story and Contract cases
exercise bounded selection and synthesis directly, and the General case runs
the complete persisted pipeline. The normal Rust suite separately checks
expected-source-hash admission, immutable effective-profile persistence,
profile-specific warnings, citations, and reopen behavior. The production
frontend build checks the compiled Automatic handoff, but this harness does not
drive the native file dialog or prove classification-to-result behavior in one
live UI invocation.

## Evidence gathered on 2026-08-30

The deterministic harness passed all five public documents. Observed topology:

- W-9: 6 pages, 5 chunks, no visual-only pages.
- Minimum-wage poster: 1 rotated page, 1 chunk, no visual-only pages.
- Contractor agreements: 8 pages, 2 chunks, no visual-only pages.
- NARA scanned/OCR schedule: 12 pages, 2 chunks; page 9 was retained and marked
  `NO_NATIVE_TEXT` / visual processing required.
- DOL deck: 111 pages, 3 chunks, with every page retained.

Each run ended at `CHUNKED`, state version 11, with 11 ordered events. The
source bytes and persisted parsed, normalized, structured, and chunked artifacts
matched after an independent database-connection reopen. This is not a desktop
process-restart claim.

The first live W-9 run exposed a legitimate PDF line-wrap mismatch: Qwen copied
the same words but replaced a source newline with a space, so the exact-quote
validator rejected it. The validator now permits only whitespace-run
reconciliation and persists the literal source substring; punctuation, case,
word, order, ID, duplicate, and size changes still fail.

The first live poster run completed with exact durable evidence but took
187.30 seconds and produced only one claim. A direct bounded probe of W-9 page 2
returned eight material evidence items in JSON mode in 121.14 seconds. Those
timings are not release benchmarks: a separate LM Studio Qwen3.6 evaluation was
occupying most GPU memory, causing Ollama to report 76% CPU and 24% GPU
execution. The full W-9 analysis accepted its repaired first response but timed
out on the next request under that contention.

An initial five-to-eight-item, 768-token poster rerun improved coverage but
truncated its JSON during item six, so it correctly produced no artifact. The
aligned runtime contract now uses temperature zero, fixed seed `42`, and no
reasoning effort; retains its JSON-Schema attempt; falls back to JSON-object
mode for the selected model's exact vocabulary error; and limits new chunk
analysis to five evidence items under a 1,024-token budget with
shortest-sufficient-quote guidance. If one complete chunk response is invalid,
analysis may replace it once using a stricter one-block,
one-contiguous-passage prompt. The rejected response is never partly persisted,
successful repair is warned, and a second invalid or unavailable response still
fails the run.

With the final five-item bound, the minimum-wage poster completed the full live
pipeline in 395.07 seconds with five evidence items, five supported claims,
state version 18, and 18 ordered events. Its summary and citation artifacts,
including exact source quotations, survived an independent SQLite connection
reopen. A two-chunk contractor-agreement analysis also passed with ten evidence
items in 349.67 seconds. A subsequent full contractor run did not complete: its
first response was invalid and the bounded repair request returned
`MODEL_RUNTIME_UNAVAILABLE` after 406.48 seconds. No full W-9 or contractor
completion is claimed.

These were correctness observations under contention, not performance
acceptance. During the latest failure Ollama reported 78% CPU / 22% GPU while a
separate LM Studio evaluation retained most GPU memory. At this checkpoint the
complete live W-9 and agreement runs remained unproven.

## Live corpus closure on 2026-09-01

The five-document deterministic harness was rerun after the installed two-app
acceptance slice and again preserved 6, 1, 8, 12, and 111 pages, including NARA
page 9 as visual-only. All five runs reached `CHUNKED` at state version 11 with
11 ordered events, unchanged source identity, and equal durable artifacts after
an independent SQLite connection reopen.

Uncontended live W-9 and contractor runs exposed two additional fail-closed
boundaries rather than false success. Qwen could still rewrite or associate a
familiar sentence with the wrong W-9 block during the single repair request.
The repair contract now gives the model a bounded catalog of application-
derived exact quotations and accepts only supplied quote IDs plus claim text;
Rust remains the sole owner of quotation bytes and block/page provenance. The
catalog is capped at 48 candidates and 12,000 quotation characters. A larger
51,063-character repair payload reproduced an invalid model response under the
8,192-token deployment context; bounded W-9 repairs measured 13,752 and 13,903
user-prompt characters and passed.

Ollama's selected model also returned eight claims for an intermediate request
whose schema allowed four when server-side grammar was unavailable. Both
evidence and candidate synthesis prompt envelopes now carry the exact dynamic
`maximum_claims` used by the unchanged Rust validator and JSON Schema. The
application limit was not raised.

With `qwen3-30b-a3b:latest` on `127.0.0.1:11434`:

- The six-page W-9 completed in 60.85 seconds with 6 supported claims and 6
  cited evidence items. The reopened run was `CompleteWithWarnings`, state
  version 18, with 18 ordered events and
  `MODEL_EVIDENCE_RESPONSE_REPAIRED` recorded.
- The eight-page contractor packet completed in 34.71 seconds with 6 supported
  claims and 6 cited evidence items, the same terminal state/version/event
  counts, and the explicit repair warning.
- The twelve-page NARA schedule completed in 32.42 seconds with 6 supported
  claims and 6 cited evidence items. Page 9 remained visual-only, while the
  accepted evidence cited pages 1 and 5 only. `NO_NATIVE_TEXT` and the repair
  warning survived the independent reopen.

Each live test reloaded identical summary, citation, and immutable event
artifacts from a new SQLite connection and re-read the original source bytes.
The reported timings are single correctness observations with a warm local
model, not release benchmarks or process-restart claims. OCR/vision remains
deferred; a visual-only page is retained and warned, never invented or cited as
native text.

## Product boundary: paid Connect

Connect is optional only for standalone resilience. The commercial requirement
is that cross-application capability discovery and job submission require a
paid Connect entitlement. The provider's process-registration file deliberately
remains present while the provider is live, including while entitlement is
denied, because it represents process ownership rather than a sellable
capability. Authenticated manifest discovery, job submission, and job status are
the entitlement-gated product boundary. A denied or expired provider therefore
retains its live registration but advertises no capability to Email Watcher.

Google OAuth stays private to Email Watcher. Email Watcher uses its credential
store to retrieve an attachment, then explicitly streams the selected PDF bytes
and bounded metadata through Connect. Document Summarizer must never receive a
Gmail token, mailbox permission, or private Email Watcher database path.

The provider and consumer activation slices have now proved all four states
without changing either standalone workflow:

1. entitled plus provider available: capability appears and a job succeeds;
2. not entitled: capability is absent and job admission fails closed;
3. provider absent or Connect unavailable: capability is absent while Email
   Watcher remains healthy;
4. entitlement/provider restored: capability returns without an Email Watcher
   code change.

The proof used the accepted test authority and local packages/processes; it was
not a production-key issuance, live Gmail OAuth, or human-click acceptance run.
The product/security decision is now an offline Ed25519-signed entitlement with
build-time issuer trust, request-time evaluation, and no grace period. Production
issuer-key custody, license delivery, billing/account management, workflow
automation, and a marketplace remain outside that proof.
