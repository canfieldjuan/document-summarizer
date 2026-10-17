# Draft native summary parity amendment

Status: proposed, awaiting operator acceptance. This extends the accepted earlier-synthesis clause in docs/PR-C9-PREPRODUCTION-PARITY.md:97-101. It authorizes a test-only native adapter parity check after acceptance. PR116 remains held and v2 waits.

## Recovered baseline

The original whole-contract freeform runner was preserved unchanged and reproduced both saved summaries byte-for-byte. Its model is Qwen3.5-9B Q4_K_M at a 32768-token context, temperature 0 and a fixed 1500-token output allowance, with thinking disabled. The runtime reports b1-c1d0e7a. The runner omits seed; the captured server default is 4294967295. Captures include requests, transmitted JSON, raw responses, template, token IDs, runtime properties and loaded-file identity.

The two inputs used 8587 and 18420 prompt tokens; outputs used 773 and 939 tokens and ended normally with no reasoning content. The summaries contain 467 and 597 words despite a requested 350-word ceiling. Existing ratings retain omissions and a modality issue. These outputs are reproducibility targets, not faithful drafting acceptance labels.

The executable freeze, model and runtime hashes, input aliases and message hashes are in inventory.json. The baseline runner hash is fb2fff0b00079aa359eda1d634ec9c0ce552c53d912527e29ce4b297de284015. Model hash is cd76ec205963b3b33350093e6904d9de16c4e666fd104e1f632d25c7f15f2a13; launcher hash is 0ca399edd758decd825a71823b04ba7ddbc8b2e10d2309d8bf623ee3c2283099. The fresh full captures supplement incomplete historical provenance; they do not recreate it.

## Production differences

The inventory lists each audited departure with its owner and source hash at 43d3fca56a6950a2e4ea9df27965c33b53aac225. The main distinction is task representation: the candidate writer samples a versioned source catalog and requests cited JSON paragraphs, while the baseline supplies whole contracts and requests freeform text. Ordinary General delivery-policy routing also preserves prior paraphrases, and the coherent candidate remains behind its deliberate qualification hold.

The writer's 2048-token value is a starting allowance. Its budget owner can raise the effective allowance to available runtime capacity. Historical app requests confirm larger effective allowances. The baseline's fixed 1500 tokens must be preserved for adapter parity.

Native transport and launch differ: production uses a private socket, one slot, manually composed token framing and /completion; baseline uses TCP, four slots, Jinja chat framing and /v1/chat/completions. Both use temperature 0. Native supports Text. The gateway document task requires structured output, so wrapping the baseline in JSON would change the task and is outside this amendment.

## Authorized surface after acceptance

Add test-only synthesis cases to the existing parity registry and reuse the existing native framing helper in src-tauri/src/pipeline/llama_cpp_framing_tests.rs. Refactor only the fixture handling needed to admit a distinct synthesis task; preserve the original C9 assertions and cases. Invoke the actual production ModelRuntime generation implementation, not a Python approximation or copied adapter.

Construct ModelRequest using the frozen system/user strings without normalization, truncation, segmentation, prompt addition or response schema. Use Text, fixed 1500 output tokens, the pinned 9B model/runtime/libraries, 32768 context and temperature 0. Because ModelRequest requires a seed, explicitly supply the observed omitted-seed default 4294967295. This is a listed adapter projection to be checked, not a claim that its effect is already proven.

Use production launch and transport unchanged. Capture process identity, loaded runtime hashes, server properties/sampler defaults, wire payload, rendered framing and token IDs before generation. Classify each departure as exact-preserved, listed-operational, or unproven. Stop on any unlisted difference. Compare all sampling options with the captured baseline; an effective sampling difference blocks generation and requires a revised amendment.

## Verification and call ceiling

The hard ceiling is two generation calls, one per recovered input. They are separate from the accepted verifier budget. No automatic retry, tuning, alternate seed, candidate or extra call is authorized.

Before spending a call, verify all artifact hashes and model/runtime pins, byte equality of original messages, complete production-rendered prompt equality and token-array equality against the captured template/tokens. Admission must preserve all tokens and the 1500-token allowance at 32768 context. Framing and admission checks generate zero tokens. Stop before live inference if any check fails; identify the owning port difference before proposing a repair.

Then call the actual native production adapter once per input, in captured order. Save full raw response, process/provenance identity, effective settings, prompt/output token counts, finish status, reasoning content, latency and output hash to durable private evidence. Acceptance requires both outputs to equal the frozen reproduced summary text byte-for-byte, with complete normal finishes and no reasoning content. Existing production trimming is recorded; it does not change either target. Report raw wire and exposed adapter text separately.

Stop at the first invalid response or output mismatch. Diagnose the first divergence in framing, settings, launch, transport or parsing against the successful runner. Do not infer model insufficiency or change behavior. If production repair is needed, reproduce and isolate it, explain its introducing change, revise scope before editing production, and prove the minimal regression fails before and passes after the origin repair.

Offline negative checks must reject missing/duplicate cases, altered messages, altered output target, wrong model/runtime/template hash, wrong token IDs, changed output allowance, changed effective settings and extra planned calls. Check exact equality rather than lexical scoring or containment. Run the affected test file and applicable formatting checks; no broad suite solely to duplicate CI.

## Non scope and completion

This amendment does not change the writer, input selection, schema, citations, gateway, ordinary presets, admission limits, verifier rules or qualification holds. It does not certify word limits, summary fidelity or page capacity. Faithful drafting and capacity measurement remain subsequent decisions after same-input adapter parity.

Evidence stays outside worktrees: source-bearing captures under Desktop/doc-classify-corpus/heldout and public receipts under Desktop/codex-evidence/document-summarizer. Directories are mode 700 and files mode 600. PR reports use aliases and SHA256 values, never contract names or source text. Record tested head and per-case status. Preserve partial failure evidence. This amendment completes only when both cases pass, or stops with a divergence report requiring a new decision.

## Recorded operator acceptance

ACCEPTED unchanged, relayed by oversight in PR116 issuecomment-6041530861 on 2026-10-07. The accepted original bytes have SHA256 e0a8bd2a925924b63b4c7f8b7ea8be05585597f84c4516124ddd5ef6fa720473 and alias synthesis-port-inventory/CONTRACT.md. The operator then directed this session to address the threads and continue. This acceptance record precedes implementation. The original draft status above is preserved as part of those accepted bytes; the scope, two-call ceiling and stop rules are unchanged. PR116 remains held.
