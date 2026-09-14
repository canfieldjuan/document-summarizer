# State inference locality truthfully for release

## Contract

### Verified root cause

- Direct Ollama accepts only unauthenticated exact-loopback HTTP and disables proxies and redirects.
- The separately selected Local Inference Gateway accepts an administrator-configured HTTPS origin with no network-locality restriction. Its authenticated requests contain the complete system and user prompts used for document analysis, synthesis, and verification, including selected source excerpts.
- Current README, runtime-contract, and setup copy describe inference as local or on-premises without disclosing that a configured Gateway may be remote. The runtime contract also lists only Ollama and reports superseded artifact versions.

### Outcome

- Keep arbitrary administrator-configured HTTPS Gateway origins as the intended boundary; do not infer network locality from DNS names or addresses.
- Before Gateway configuration, disclose that the configured service may be outside the workstation and receives complete document-derived prompts and selected source excerpts.
- Keep direct Ollama's exact-loopback admission unchanged and distinguish direct llama.cpp as an app-managed local child.
- Align the runtime contract with Gateway, Ollama, and qualified llama.cpp dispatch and with the current synthesis, verification, summary, and citation versions.

### Non-scope

- Do not change endpoint admission, transport, credential storage, model selection, pipeline prompts, persistence, Connect behavior, Windows credential validation, installer behavior, or release demonstrations.
- Do not rename the Local Inference Gateway product or claim that transport authentication proves the operator controls the configured service.

### Acceptance and verification

- README, the executable contract, and both initial and refreshed Gateway setup copy state the same locality boundary.
- The release contract rejects the stale on-premises-only claim and requires both the Gateway disclosure and direct-loopback distinction.
- Run the focused release-contract test fail-first, then the complete release-contract test target, frontend production build, Rust formatting, strict Clippy, and `git diff --check`.
- The PR is complete when issue #61's runtime list, data-egress disclosure, and current artifact-version criteria are all satisfied without changing runtime behavior.
