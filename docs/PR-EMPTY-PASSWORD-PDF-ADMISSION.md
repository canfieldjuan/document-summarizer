# Read PDFs that open with an empty password

PROPOSED CONTRACT for [issue #133](https://github.com/canfieldjuan/document-summarizer/issues/133).
This revision requests review of the behavior and implementation surface below.
Implementation follows independent oversight review and Juan's acceptance of the
exact contract revision. The contract commit contains documentation only.

## Root cause

The original-document admission rule conflates encryption with inability to
open a PDF without a supplied credential. `PdfExtractParser::parse_inner` loads
the bytes successfully, then rejects either `was_encrypted()` or
`is_encrypted()`. The first condition rejects documents already decrypted in
memory with the empty password. The rule is present in the recorded baseline
commit `91dce3cb9039cea7f3e08ac7cbb3060d2f730821`.
[Native admission at the inspected source](https://github.com/canfieldjuan/document-summarizer/blob/e5000cf20fdd65d53804d17f93f6f7683e6e2aad/src-tauri/src/pipeline/parser.rs#L79-L98)

The locked loader attempts empty-password authentication. On success it records
`encryption_state` and removes `/Encrypt`. On unsuccessful authentication without
a supplied password it can return a partially loaded encrypted document. Thus a
successful `load_mem` alone does not prove readable content. `is_encrypted()`
also depends on resolving a correctly typed encryption dictionary, so it cannot
alone distinguish a malformed `/Encrypt` entry from an unencrypted document.
The dependency source inventory below binds the inspected implementation bytes.

The issue reports two frozen inputs whose four unseen claims cannot currently
pass admission. Their private contents and labels are outside this contract's
preparation. This is an admission repair. Summary fidelity remains separately
qualified after source admission.

### Inspected inventory

| Owner | Pin and existing behavior |
| --- | --- |
| Doc Sum | `e5000cf20fdd65d53804d17f93f6f7683e6e2aad`, the fetched main-based implementation target. `parser.rs:79-98,154-176` verifies original bytes, loads, applies the blanket rule, and extracts each page. |
| PDF dependencies | `src-tauri/Cargo.lock:2452-2455,3033-3044` locks `lopdf 0.42.0` and `pdf-extract 0.12.0`. Reader source `reader.rs:808-818,850-853,904-909,1147-1173` and `document.rs:257-271` controls the states described above. |
| Parser lifecycle | `parser.rs:902-981` uses the actual ingest record and atomically persists parsed output or the parse failure. Existing encrypted fixture at `parser.rs:2038-2073` supplies a nonempty user password and expects blanket refusal. It does not cover a readable encrypted twin. |
| OCR input | `connect/ocr_consumer.rs:351-420,1048-1053` rereads and hashes the original source, counts pages through a second raw PDF load, and builds the existing request. `pipeline/db.rs:384-448` verifies the exact root, parser eligibility, original byte size and hash before durable admission. |
| OCR output | `parser.rs:211-225,266-268` applies separate tagged-output admission, including its encryption restriction. The output is an admitted provider artifact, separate from the original NativeText source. |
| Document OCR | Inspected `bffe619b15239857b3b7cf73827fb2643268bed7`. `processor.py:153-162` opens original bytes with pikepdf for page count. `render.py:77-96` opens them with PDFium. `tagged_pdf.py:419-421,528-536` constructs and validates a derived artifact. Source inspection is not an empty-password compatibility receipt. |
| Invoice prior art | Inspected `f7281d71e050fd5e9c746bb5e45c55862af12c26`. `extract/pdf.py:475-496` makes blanket refusal opt-in. `tests/test_certificate_connect.py:167-186` creates an empty-user-password PDF, proves it is readable, then requires the COI route to refuse it. Reuse the public fixture/control pattern. Doc Sum's requested admission policy differs from COI's refusal policy. |

Evidence alias `issue-133-source-inventory`, SHA-256
`04d837c570f85c58d83b4a65fe45a52ae5799e7202b36229018c08bc4ba45d60`,
records the source pins and file hashes. Both cached dependency archives matched
their Cargo.lock checksums, and the selected extracted files matched those
archives. This inventory records source inspection, not a passing parser run.

The issue's reference to Doc Sum #108 as certificate prior art points to the
wrong repository. Doc Sum #108 concerns General-summary trimming. The Invoice
Processor source and test above are the relevant certificate boundary.

No existing 32k, OCR-quality or installed-product receipt establishes this new
admission behavior. No model, prompt, sampler or tokenizer participates in the
proposed parser proof. The implementation must re-pin the actual dependency and
producer sources before using this inventory if those sources have changed.

## Behavioral contract

### Original-document admission

The admitted class is a structurally readable original PDF that the supported
PDF parser can fully open using only the empty credential. The term
"owner-password-only" describes the common case, not a filename, metadata or
permission-bit classifier. Doc Sum never guesses a nonempty credential.

| Input | Required result |
| --- | --- |
| Ordinary unencrypted PDF | Use the existing native extraction and page path. |
| Supported encrypted PDF opened with the empty credential | Extract from the fully decoded in-memory document. A history flag recording original encryption does not refuse it. |
| Structurally valid, supported encryption requiring a nonempty opening credential | Fail Parse with `PDF_PASSWORD_REQUIRED`, recoverable=false, and the message "This PDF requires a password. Password entry is not supported." No parsed artifact, OCR handoff or model stage is produced. |
| Malformed encryption dictionary, missing/wrong-typed required fields, invalid reference, or corrupt encrypted contents | Fail Parse with `MALFORMED_PDF`, recoverable=false. Do not classify structural failure as a request for a password or as an unread page eligible for OCR. |
| Well-formed but unsupported security handler, encryption version or revision | Fail Parse with `PDF_ENCRYPTION_UNSUPPORTED`, recoverable=false. An unsupported encryption feature must not be reported as a wrong password. |
| Source bytes changed after ingestion | Preserve `SOURCE_CONTENT_CHANGED` before PDF admission. |

Classify failures using the existing typed PDF, object, encryption and password
APIs. For example, `InvalidPassword` and `DecryptionError::IncorrectPassword`
have different meaning from malformed dictionaries and unsupported handlers.
The loader can suppress an initial authentication error, so the owner must
validate any remaining `/Encrypt` entry through those APIs before deciding the
failure class. A returned partial document cannot reach page enumeration.
Inspect parsed objects, not regexes, raw `/Encrypt` substring searches, exception
messages or a second independent PDF parser. A dependency change requires its
own contract amendment if the pinned APIs cannot represent the necessary proof.

The parsed document supplies the existing page order, text, warnings and visual
markers. Owner-only permission flags do not become a new application-side
password test. The proof must cover restricted permission flags, including
copy/extraction restriction, on the same empty-password-readable class.

### Memory and provenance

Verify source size and SHA-256 before opening, using `verified_source_bytes`.
Only the parsed in-memory object is decrypted. Never save or reopen a decrypted
PDF copy, replace the ingested source, or bind identity to rewritten bytes.
The input file and original encrypted snapshot remain byte-identical.

Existing private parsed-text and database persistence remains the normal output
of parsing. "No plaintext copy" here prohibits a decrypted source-PDF file, not
the application's intended private extracted-text artifact. Tests inspect the
owned scratch tree and retained source snapshots, as well as their digests.

The encrypted and clear twins have different original hashes and document IDs.
Compare their complete page/text/warning/source-type projections after excluding
only those distinct input identities. Compare normalization and citation page
spans through existing downstream owners without generation. Never change a
hash merely to make the twins compare equal. Keep the parser's library identity
truthful and bind the application-policy change to the tested source commit.

### One owner and the OCR boundary

One app-internal native-PDF opening owner in `pipeline/parser.rs` returns
`Result<PdfDocument, PipelineFailure>` only after the admission rule above holds.
`PdfExtractParser::parse_inner` and the original-source OCR page-count path
delegate to that owner. The latter may translate its typed failure into the
existing OCR error envelope. Remove their independent original-source loading
and blanket native-encryption checks. No public API export is required.

A successfully read original PDF with a scan or mixed unread pages keeps the
existing parser-owned visual markers and `ParsedDocument::requires_ocr()` rule.
The existing whole-PDF handoff sends the exact original encrypted bytes, hash,
page count and root identity. It does not send a temporary cleartext surrogate,
splice native/OCR pages, or clear an unread-page signal. Password-required and
malformed originals fail before handoff preparation or provider submission.

The document-ocr owner must confirm compatibility on the public encrypted/clear
scan twins through its actual pikepdf page-count, PDFium rendering and tagged
builder owners. Source inspection alone does not establish this. Bind its exact
revision and evidence receipt. If it needs a production change, that owner must
prepare and obtain acceptance of a separate producer contract before the OCR
branch is claimed complete. This Doc Sum contract grants no producer edits.

The derived tagged PDF remains governed by its current provider-output contract,
including encryption, size, structure, text-pair and provenance checks. Reuse a
valid unencrypted tagged output for the offline consumer proof. Test an encrypted
tagged result as a rejected provider output. Changing tagged-output acceptance
would require a coordinated output-contract amendment, not deletion of its guard
as part of admitting original inputs.

### Existing records, concurrency and failure

Persist successful and failed parses through `parse_started_document` and the
current store transitions. No schema migration, historical failure rewrite or
automatic retry is introduced. Previously nonrecoverable encryption failures
remain historical records. A new ordinary ingestion of the original bytes can
use the repaired policy. Existing parsed runs continue under their stored state.
Recovery of already-Parsed mixed roots remains the separate issue #153.

The opening owner operates on a per-invocation buffer/document and owns no shared
password cache or scratch filename. Existing state-version and immediate-
transaction protections control competing parses and OCR handoff admission.
Repeated or cancelled handoffs retain the current stable job, root/child,
lineage and cancellation behavior. Rejected input produces no handoff row,
derived child or usable parsed artifact. No fallthrough to a partial summary.

## Required change surface

- `src-tauri/src/pipeline/parser.rs` owns native opening, typed failure mapping,
  removal of the blanket native rule, and synthetic parser/lifecycle tests.
- `src-tauri/src/connect/ocr_consumer.rs` delegates original-source page counting
  to that same owner and adds original-byte handoff/negative regression cases
  through its existing fake transport. Production routing and transport behavior
  otherwise retain their existing owners.
- A small test-only fixture module may be introduced if sharing the synthetic
  builders between those existing test modules requires it. Prefer existing
  lopdf builders and encryption APIs. Public synthetic fixture bytes or builders
  belong in the test tree, with original/clear hashes recorded in evidence.
- This contract and implementation/evidence receipts record the final source,
  owner delegation, error-code changes, test results, mutations and producer
  compatibility disposition. They include every difference from this inventory.

The expected implementation is confined to native admission and its direct OCR
reader. Larger library changes, another extractor, or a new serialized field
require an explicit amendment rather than an incidental patch.

## Explicit non-scope

Runtime/client/attach/settings/deprecation work remains connect-runtime's.
No model, prompt, context/output limit, shared-host canary, writer guard,
benchmark label, frozen expected fact or source-completeness rule changes.
Dependencies, PDF geometry defaults, the tagged-output wire format, database
schema and correction format are outside this repair. Password entry, storage,
credential guessing, external decryptors and user-facing decryption tools are
outside it. Existing summary/parser qualification and merge holds remain gates.

## Assumptions and blockers

The contract targets a new main-based branch at the inspected Doc Sum revision.
Before implementation, reserve the narrow parser and OCR page-count/test surface
with any active owner of those files. Do not stack on held PR116/126/128/132 or
port their runtime work to obtain this repair.

Producer compatibility is a required coordination receipt for the scanned path.
Native public-fixture proof can proceed after acceptance while that receipt is
pending. Issue #133 remains open until both the native and OCR requirements have
evidence and the authorized frozen-input inventory has been remeasured.

The pinned encrypted reader discards individual object-decryption errors at
`reader.rs:871-873`. This source observation does not establish a reproduced
corrupt-input admission. The fail-first investigation must test the relevant
corrupt-content fixture and identify the earliest observable failure. If a
required failure signal is lost inside the library, stop that dependent change
and amend the contract for an origin repair before admitting the affected
class. A downstream text heuristic cannot substitute for that missing signal.

Private input remeasurement needs a separately bound, parse-only approval for
the two previously blocked aliases and their original hashes. Unseen claims,
labels and benchmark facts remain closed. Real OCR, model/tokenizer calls,
installed execution and production cutover require their existing separate
authorities. This contract preparation neither requests nor spends those calls.

## Verification plan

All evidence cited by a review lives in a durable operator-private evidence
directory outside worktrees, with directory mode 0700 and file mode 0600.
Published references use synthetic aliases, counts, source pins and SHA-256.

1. Reproduce before changing admission. Use the existing `build_pdf`, ingestion,
   real `PdfExtractParser` and database lifecycle with an encrypted text twin
   using an empty user password. The intended positive assertion fails on the
   current blanket rule with `ENCRYPTED_PDF_UNSUPPORTED`. Retain that exact
   result. A different failure is investigated before the repair.
2. Isolate and explain. Show loader success, its decrypted/remaining-encryption
   state, and the first native-admission divergence. Record malformed and locked
   controls separately, including partial-load behavior. This draft has source
   evidence only; the runtime reproduction remains to be executed after acceptance.
3. Fix at the named opening owner and route both original-source readers through
   it. The original reproduction must pass afterward. Commit the regression.
4. Cover unencrypted and empty-password-encrypted text twins under supported RC4
   and AES cases using existing library APIs. Check full text, page count/order,
   page warnings, normalization and citation page spans, not a matching substring.
   Include one-page, multi-page, Unicode-capable existing fixtures, and restricted
   permission flags. Keep the new policy independent of filename and metadata.
5. Cover nonempty-required credentials, malformed `/Encrypt` type/reference,
   missing/wrong-typed required encryption fields, unsupported handler/version,
   and corrupt encrypted content. Check the distinct failure class, persisted
   failed state and absent parsed artifact/handoff/child. An explicitly empty
   credential is the positive case, not a falsy value replaced by another secret.
6. Cover encrypted image-only and mixed twins with unread pages first/middle/last.
   Check the existing visual markers and OCR eligibility. Through the real store
   admission and existing fake transport, verify exact original bytes/hash/size,
   page count, provider/job binding, one derived OcrText child and unchanged
   validation. Locked/malformed inputs make zero transport calls.
7. Recheck source-change rejection, the absence of decrypted source-PDF files,
   persisted/reopened records and existing cancellation/idempotency siblings.
   Competing admission must not create a second child/job. Keep stale completed
   artifacts historical; do not relabel them as fresh successful extraction.
8. Keep tagged-output negatives, including an encrypted tagged result and a
   malformed PDF/text pair, rejected through real admission. Keep valid tagged
   output accepted. Neither side may depend on a stubbed parser or validator.
9. Mutation proof restores only the original blanket refusal and must kill the
   empty-password positive by assertion. Independently defeat password refusal,
   malformed-entry refusal and original-source identity checking, and require
   the matching negative to fail. Verify both original-source readers use the
   named owner, with an owner-bypass mutation exercising the OCR reader. Compiler
   errors, setup failures and timeouts are not mutation kills.
10. Run focused parser/consumer regressions and affected normalization/lineage
    callers, `cargo fmt --check` and the applicable strict Clippy checks. Required
    hosted CI supplies the broader/native Windows matrix. Do not execute ignored
    frozen-corpus OCR dispatch, live providers, real inference or a broad suite
    merely to duplicate CI. Native Windows runtime proof stays separately held.
11. After separately authorized private parse-only remeasurement, report original
    alias/hash, actual admission result and retained failures for both blocked
    inputs. Do not read unseen labels, substitute easier inputs or claim summary
    quality from successful PDF parsing.

Before sign-off, provide `boundary-probe` and `effect-trace` evidence for this
admission gate. The final PR records reproduction, isolation, explanation,
origin fix, passing original/isolated cases and the committed regression.

## Implementation summary

Current change is this contract document only. Application behavior, tests and
dependencies have not been edited. Runtime reproduction, fixtures, the origin
fix, producer compatibility and qualification results are future deliverables
under their accepted scopes.

## Cold diff audit

Read the final contract diff independently against the issue and inspected
owners. Verify every cited source at its recorded revision, the two original-
source readers, the separate tagged-output boundary, typed failure meanings,
original-byte custody, and every named pending gate. Check Markdown links and
`git diff --check`. Code tests are not rerun for this documentation-only commit.

## Gap audit

NOT DONE for implementation acceptance and issue closure. Pending are independent
contract review, Juan's exact-revision acceptance, any shared-file reservation,
fail-before/pass-after parser and OCR-consumer proof, producer compatibility,
the separately approved private inventory remeasurement, and normal review/CI.
The held shared-runtime G0/G1/G2 sequence remains separate from this work.
