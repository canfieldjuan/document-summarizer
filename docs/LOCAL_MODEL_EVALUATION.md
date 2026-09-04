# Corpus and native Ollama evaluation — 2026-09-04

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
