# Corpus and native Ollama evaluation — 2026-09-04

## Latest result: larger output allowances help progress, but both Muse runs fail

**No product closure: neither document delivered a summary.** Contract
`9996ed4` precedes implementation `253a6e4`. Analysis now allows 2,048 output
tokens, direct/hierarchical synthesis 4,096, and verification remains 4,096.
Input admission reserves output and framing within the unchanged 8,192-token
context assumption. All validators, page/evidence/claim floors, temperature,
reasoning request, and the 900-second request timeout remain unchanged.

The requested `Muse-Glimmer-30B-KQuant-17GB-Q4_K_M.gguf` ran directly in standalone
`llama-server` (installed build `c1d0e7a`), using its embedded ATEM template with
Jinja, one slot, context 8,192, and loopback port 11435. **No LM Studio runtime
was used.** This is the same GGUF/runtime configuration as the preceding Muse
probe, not a weights-only comparison with the earlier Qwen/Ollama runs. Each
acceptance run creates a new run-derived seed; these are individual observations,
not a same-seed reproducibility or causal-quality benchmark.

| Run after increase | Delivered claims | Validated evidence | Final cited-page fraction | Requests | Completion tokens | Prompt tokens | Model time | Test time |
| --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: |
| NARA | Not produced | 8, persisted | Unavailable: no summary | 12 | 16,996 | 16,571 | 454,859 ms | 455.60 s |
| DOL | Not produced | 1 transient; no complete artifact | Unavailable: no summary | 2 | 569 | 1,100 | 15,386 ms | 17.26 s |

Both test commands exited 101. All requests used Primary transport, which does
not establish schema enforcement. NARA completed all eight page analyses and
both synthesis attempts. One accepted analysis response used 1,139 output tokens,
above the old 1,024 allowance; re-synthesis used 2,116, above its old 2,048 limit.
The first verification supported six of eight claims and marked two ambiguous,
triggering the existing bounded retry. Final verification consumed all 4,096
allowed generation tokens and returned truncated JSON, producing
`MODEL_VERIFICATION_RESPONSE_INVALID`. Its reported prompt plus completion was
6,529 tokens; server logs show `truncated = 0` (no context truncation). The
observed exhaustion is output, not the full context. No final supported summary
or citation artifact was delivered. Increasing analysis/synthesis allowances did
not establish that verification's unchanged allowance is sufficient for Muse.

NARA completion usage by stage was 5,076 analysis, 3,814 synthesis and 8,106
verification. DOL's second response instead failed
`MODEL_EVIDENCE_RESPONSE_INVALID`: its claim was 196 characters against the
unchanged 192 limit, using only 261 completion tokens. The two quote IDs were
valid in their separate page-local requests; this was length, not duplicate-ID
failure. More generation space cannot enforce the missing runtime length bound.

For comparison, the preceding Muse run with 1,024/2,048/4,096 allowances failed
NARA during analysis: three requests, two transient validated items, 2,034
completion tokens and 55,007 ms model time. Its failing response used all 1,024
generation tokens without final text. DOL previously failed after two requests,
one transient validated item, 538 completion tokens and 14,451 ms model time;
a separate diagnostic found a 218-character claim. Neither baseline delivered
a summary. The additional spend after this increase is not a measured coverage
win, and no claim of successful Muse model-quality comparison is made.

The earlier runtime probe also matters: the installed Muse-specific handler in
`common/chat.cpp` enables grammar for tools but does not apply the JSON response
schema on this path. A strict enum negative probe returned the forbidden value.
The template defaults to high reasoning despite the adapter sending
`reasoning_effort: "none"`. Output allowances are therefore only one limitation;
runtime schema enforcement and model-specific reasoning control remain open.
No runtime patch, reasoning change or further context/output increase was made
to hide this result. The standalone server was stopped cleanly after both tests.

Local gates on the implementation passed: `cargo test --all-targets
--all-features --quiet` (220 library tests, four ignored; one deterministic
acceptance and three release tests), `cargo clippy --all-targets --all-features
-- -D warnings`, `cargo fmt --all --check`, and `git diff --check`. New coverage
checks request allowances, exact input boundaries including system text,
hierarchical partitioning and preserved historical v3 quotas. These gates do
not override the failed live acceptance.

Reproduction: set `DOC_SUM_MODEL_BASE_URL=http://127.0.0.1:11435/v1/`,
`DOC_SUM_MODEL_NAME=doc-sum-muse-glimmer`, `DOC_SUM_OFFICE_PDF` to each public
corpus path below and `DOC_SUM_OFFICE_TRACE_MODEL_RESPONSES=1`, then run
`cargo test --test office_acceptance
office_pdf_live_ollama_summary_has_exact_durable_evidence -- --ignored --exact
--nocapture` from `src-tauri`. Public diagnostic logs are retained locally in
`/tmp/doc-sum-output-budget.1lTgFh/`; the preceding Muse probe is in
`/tmp/doc-sum-muse-eval.MizXyU/`. Raw source/model text is not committed here.

## Latest result: both corpus acceptance gates still fail after per-page selection

**The product blocker remains open.** Contract `2c0bb13` precedes implementation
`75df3bd`. The new single-item page-local analysis succeeds on both public
documents, but NARA still delivers insufficient verified coverage and DOL fails
in synthesis before producing a summary. No validator, floor, timeout, endpoint,
or model was relaxed. PR #30 is ready for review, not draft, and is not merged.

| Run | Delivered claims | Validated evidence | Cited native-text pages | Requests | Completion tokens | Model request time | Test time |
| --- | ---: | ---: | --- | ---: | ---: | ---: | ---: |
| NARA first | Below floor 4; exact count not printed | 8 | Not printed before assertion | 12 | 3,112 | 36,751 ms | 37.56 s |
| NARA diagnostic rerun | 4 (budget 8, floor 4) | 8 | 4/11 (36.36%) | 12 | 3,054 | 31,701 ms | 32.51 s |
| DOL first | Not produced | 67 | Unavailable: no summary | 68 | 3,586 | 43,751 ms | 45.84 s |

All three test commands exited 101; all requests used Primary transport with no
schema fallback. All planned analysis responses passed the unchanged exactness,
192-character, identifier-membership and provenance checks, plus the new
single-item/page-local checks. No invalid or repeated quote ID reached an
accepted analysis. DOL's 67 evidence items follow from its 67 validated one-item
responses and completed analysis checkpoint before synthesis started; there is
no delivered citation artifact for that run.

NARA first passed analysis in eight requests using 360 completion tokens. It
completed synthesis and verification twice, then the live gate rejected the
delivered claim count. The test now prints delivered metrics before quality
assertions rather than hiding them when an assertion fails. A separate public
diagnostic rerun on the same production code delivered four supported claims,
citing four of eight validated evidence items and four of eleven native-text
pages. It failed the evidence-coverage assertion and also falls below the
60-percent page target. The durable warnings included `SEMANTIC_CLAIMS_WITHHELD`
and `SUMMARY_COVERAGE_SHORTFALL`. Both synthesis responses were identical to
each other within each run, as were both verdict responses; a different attempt
seed does not guarantee different output at temperature zero.

DOL completed all 67 analysis requests in 37,394 ms using 2,870 completion
tokens. Its first synthesis batch then failed `MODEL_CLAIMS_RESPONSE_INVALID`:
"Every supplied evidence ID must be cited by at least one synthesis claim."
No verification or re-synthesis ran. The analysis-stage uniqueness and page
coverage blockers are removed; synthesis coverage is a separate live blocker.
Deterministic positional quote selection was not activated: the page loop met
its evidence target on both documents, and positional selection does not itself
enforce synthesis references or semantic support.

The diagnostic also shows that structural validity is not semantic quality:
one bounded NARA claim ended mid-thought, and another described illegible source
content rather than a substantive document finding. Exact quotation/provenance
does not make every paraphrase useful or correct. No quality improvement is
claimed from analysis counts alone.

The selected model remained `qwen3-30b-a3b:latest`, with context 8,192 and the
unchanged 900-second timeout. Both input hashes match the corpus hashes below.
Local gates on this implementation passed: 219 library tests with four ignored,
one deterministic acceptance test, three release tests, strict clippy, and
formatting. Boundary tests cover one/zero/two response items, foreign/mixed IDs,
192/193 characters, page-only enums, tail-inclusive stopping, sparse/dense and
chunk-independent plans, empty unselected chunks, and historical v3 artifacts.
These local gates do not override the failed live acceptance.

## Latest result: the product blocker remains after the bounded-generation fix

Both corpus runs still fail before delivering a summary at implementation
commit `8160be8`, following contract commit `6cf34c4`. The code now preserves
`maxLength` up to and including 192 and explicitly instructs the model to keep
claims within that limit and use each quote ID at most once. All validators,
evidence floors, budgets, the model, and the endpoint are unchanged.

| First run after the change | Claims | Evidence artifact | Cited-page fraction | Requests | Completion tokens | Request time | Test time |
| --- | --- | --- | --- | ---: | ---: | ---: | ---: |
| NARA | Not produced | Not produced | Unavailable; 11 native-text pages | 1 | 125 | 6,510 ms | 7.09 s |
| DOL | Not produced | Not produced | Unavailable; 111 native-text pages | 3 | 995 | 9,840 ms total | 11.68 s |

Both commands exited 101. Every request used Primary transport. NARA passed
the claim-length and quote-ID checks but failed the distinct-page floor in its
first scope. DOL passed two scopes, then failed the unique-ID/bounded-text
check on the third. The complete analysis schema compiled successfully in
Ollama; no schema fallback occurred. No synthesis or verification request ran.
These measurements supersede any expectation that the requested prompt and
projection changes alone close the original complaint.

One diagnostic rerun per public document used the same code with response
tracing enabled. NARA again failed distinct-page coverage: its three different
quote IDs had claim lengths 71, 70, and 126. It used 116 completion tokens in
one request, 1,825 ms model time, and 2.41 s test time.
DOL again failed in its third scope. Its first two scopes each returned nine
items that passed validation (18 transient evidence items, not a persisted
analysis artifact). The third returned nine items but repeated both `q6` and
`q7`; all claim lengths were within 192. That diagnostic used 992 completion
tokens across three requests, 9,839 ms model time, and 11.70 s test time.
Several claims in its second scope reached the 192-character boundary and
ended mid-word; decoder length enforcement alone is not semantic quality proof.

The remaining gap is selection structure: an enum permits repeated members,
and distinct quote IDs can still cite the same page. Explicit prompt wording
did not reliably satisfy either requirement on this corpus. Closing the
product blocker needs a further contract for reliable distinct-page/quote
selection; this change does not silently deduplicate, lower a floor, or relax
the validator. The narrow fix was opened for review, not claimed as a successful
corpus closure, and issue #29 remains open.

Local gates passed: 216 library tests, one deterministic acceptance test,
three release tests, strict clippy, and formatting. The new projection test
failed before implementation and passed afterward; it covers 0, 191, 192,
193, 2,000, and 4,000. Existing negative evidence/provenance tests still pass.
Local success is separate from the failed live acceptance above.

## Corpus result: both large-document live runs fail

The live acceptance runs used the tree of merged `main` at
`ca828dadadedc31c78359915e8798ce41e27a2af` (tree
`9abce419f1ac7db214dd0ef688c7c609cd711393`), Ollama `0.24.0`, and
`qwen3-30b-a3b:latest` digest
`1eda56426671cdf365913097543c2253a73c57e35b12741306689968d7f70292`.
Both local PDFs matched the SHA-256 values in [the office corpus](OFFICE_ACCEPTANCE.md).
Each run used the existing 900-second request timeout without overrides.

| First live run | Claims / accepted evidence | Cited native pages | Requests | Prompt tokens | Total completion tokens | Request time | Test time |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: |
| NARA schedule | No artifact | No artifact; denominator 11 | 1 | 3,179 | 174 | 5,654 ms | 6.25 s |
| DOL deck | No artifact | No artifact; denominator 111 | 1 | 2,602 | 403 | 4,266 ms | 6.34 s |

Both commands exited 101. Every request used Primary transport with no schema
fallback and a 1,024-token output allowance. Both stopped during the first
analysis scope with `MODEL_EVIDENCE_RESPONSE_INVALID`: "Evidence items must
contain unique quote IDs and bounded claims". No analysis artifact, synthesis,
verification, or summary was produced. A supported-claim count or successful
cited-page fraction therefore cannot be reported; user-delivered coverage is
absent. These results do not establish that the original thin-summary complaint
is fixed.

The deterministic checkpoint test passed for both source files. NARA retains
12 pages, 11 normalized native-text blocks, 2 chunks, and visual-only page 9;
the DOL deck retains 111 pages and blocks, 3 chunks, and no visual-only pages.
The source hashes, durable artifacts, and ordered events survived reopening.
That checkpoint run also passed after the test-only cleanup change.

Reproduction from `src-tauri`, using the locally downloaded corpus paths:

```bash
DOC_SUM_OFFICE_PDF=/absolute/path/to/nara-scanned-records-schedule.pdf \
  cargo test --test office_acceptance \
  office_pdf_live_ollama_summary_has_exact_durable_evidence \
  -- --ignored --exact --nocapture
```

Repeat with `dol-workplace-poster.pdf` for the DOL deck. That historical local
filename refers to the 111-page training deck, not the minimum-wage poster.

## Diagnostic reruns and controlling code

One rerun per public document enabled `DOC_SUM_OFFICE_TRACE_MODEL_RESPONSES=1`.
These are separate attempts, not replacements for the first-run measurements:

| Diagnostic rerun | Requests | Prompt tokens | Completion tokens | Request time | Test time |
| --- | ---: | ---: | ---: | ---: | ---: |
| NARA | 1 | 3,188 | 182 | 2,555 ms | 3.14 s |
| DOL | 1 | 2,594 | 408 | 4,335 ms | 6.24 s |

Both exited 101 through the same validator. NARA returned distinct `q1`, `q5`,
and `q9` selections, but its third claim had 334 characters against the
192-character bound. DOL returned overlong claims and repeated `q2` twice.
Neither diagnostic response contained a foreign quote ID.

At the measured tree, `summary.rs::analysis_output_schema` supplies
`maxLength: 192`; `model.rs::decoder_compatible_schema` removes every
`maxLength` and `uniqueItems` recursively. The analysis system prompt asks for
concise text without stating the numerical character limit.
`summary.rs::parse_evidence_response` independently rejects overlong claims
and reused quote IDs. The enum only restricts membership, not uniqueness across
items. The later verification re-synthesis path cannot repair an analysis
failure. Runtime safety holds, but successful large-document coverage remains
blocked at analysis.

A follow-up fix should expose the bound to generation and prove that the actual
analysis schema compiles with its small string limits. Distinct-page selection
also needs proof on a complete scope; silently deduplicating items or relaxing
the evidence floor would not satisfy the contract. No production model adapter,
prompt, validator, parser, or budget was changed in this evaluation.

## Native endpoint capability and memory measurements

The [native chat API](https://docs.ollama.com/api/chat) accepts JSON Schema in
`format` and runtime `options`. Ollama documents per-request `num_ctx` in
[its FAQ](https://docs.ollama.com/faq#how-can-i-specify-the-context-window-size).
Its [OpenAI compatibility documentation](https://docs.ollama.com/api/openai-compatibility#setting-the-context-size)
states that context size cannot be set through that API; its documented
workaround creates a different model configuration.

Small local `/api/chat` probes used the same installed model, `stream: false`,
`think: false`, `options.temperature: 0`, `options.seed: 42`,
`options.num_predict: 96`, and `options.num_ctx` as shown below. The schema
required `quote_id` in `['q1', 'q2']` and non-empty `claim_text`. The user
message supplied two synthetic quotations and requested `q1`. Every response
was HTTP 200, valid JSON selecting `q1`, `done: true`, and `done_reason: stop`,
with 54 prompt tokens and 22 completion tokens. `/api/ps` confirmed each
requested context and reported model size equal to GPU-resident size.

| Requested / observed context | Ollama size_vram (bytes) | Total GPU used (MiB) | GPU free (MiB) | Request time (ms) | Model load time (ns) |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 8,192 | 18,957,908,096 | 19,026 | 5,090 | 442 | 54,847,209 |
| 16,384 | 19,750,631,552 | 19,764 | 4,353 | 5,672 | 5,257,460,405 |
| 32,768 | 21,432,547,456 | 21,375 | 2,741 | 8,128 | 7,717,632,672 |

The device is an NVIDIA RTX 3090 with 24,576 MiB. These are sequential
single-request residency observations, not long-input throughput benchmarks.
Most elapsed time after changing context was model loading. They do not prove
safe concurrency or memory availability while another application uses the GPU.

A final native probe restored `num_ctx: 8192` and added `maxLength: 192` to
the claim string. It also returned HTTP 200 and valid bounded JSON: 6,970 ms
including 6,570,430,437 ns loading; 18,957,908,096 bytes model GPU residency,
18,995 MiB total GPU usage, and 5,121 MiB free. This proves the small bound can
compile in this probe. It does not prove the full multi-item analysis schema
or the two failing corpus documents are fixed.

Model metadata reports 48 layers, 4 KV heads, and key/value head dimensions
of 128. Assuming an FP16 KV cache and one sequence, the payload estimate is
`2 * 48 * 4 * 128 * 2 = 98,304` bytes per token: 768 MiB at 8,192 tokens,
1,536 MiB at 16,384, and 3,072 MiB at 32,768. This excludes model weights,
compute buffers, allocation overhead, and concurrent sequences. It is an
estimate, not a measurement of the running cache type; the residency table is
the observed machine cost.

## Adapter decision

Moving to native chat is technically feasible and would let the application
explicitly request its declared context. The first adapter change should keep
8,192 as the default and retain the deadline, loopback/auth restrictions,
cancellation boundaries, output caps, and Rust validation. Larger contexts
should be an explicit, coherently budgeted follow-up; 16,384 left more memory
headroom than 32,768 in this experiment. Increasing context does not fix the
observed overlong and repeated evidence responses.

The adapter change would map `max_tokens` to `options.num_predict`, move the
seed/temperature into `options`, use `format` for the schema, and read
`message.content`. Native `prompt_eval_count` and `eval_count` map to existing
usage fields; `total_duration`, `load_duration`, `prompt_eval_duration`, and
`eval_duration` offer additional timing breakdowns. It must test failed/truncated
responses, usage absence, transport diagnostics, auth, health, and request
timeouts, and rerun the complete fixture and corpus before claiming adapter
equivalence. The installed model reports completion/tools capability, not
thinking; a probe accepted `think: false`, but that is not evidence for other
models. No model configuration or daemon setting was changed persistently.
