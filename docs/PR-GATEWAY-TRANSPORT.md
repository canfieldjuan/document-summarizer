# Authenticated Local Inference Gateway transport

## Why this slice exists

Schema v17 now owns one durable request identity and completed response per pipeline model step, but
Document Summarizer has no client for the gateway's actual protocol. The existing direct runtimes
cannot safely absorb transport yet: `ModelRuntime::generate` receives no pipeline run identity or
database handle, so doing so would route around the ledger.

### Problem-derived contract

The root cause is the missing authenticated protocol boundary between a durable application request
and the gateway. A correct transport must:

1. build only the gateway's version-1 `document.summary.step@1` request from a structured-output
   `ModelRequest`, and reject a request whose stage or ordinal disagrees with its durable ledger key
   before reservation;
2. reserve/reload the stable request identity, persist the exact outbound JSON digest before any
   POST, reuse the same identity/body after transport ambiguity only while its immutable expiry
   remains open, refresh the clock at submission and immediately before the transport effect, and
   fail terminally rather than dispatching an expired unresolved identity;
3. require a direct HTTPS origin, disable redirects and environment proxies, trust only system
   roots plus an explicitly configured bounded CA bundle, and read the bearer token from a bounded
   owner-private regular file on Unix without logging or persisting it;
4. bound request and response bytes and reject malformed, mismatched, or unsupported success/error
   envelopes;
5. persist validated output and producing deployment/task-policy provenance locally before sending
   `persisted` acknowledgement, returning that local output without ACK if persistence reaches the
   immutable expiry;
6. retry a lost acknowledgement from the local completed row without repeating inference, and
   return completed local output after the remote retention window or acknowledged local output
   without network access;
7. bind the admitted generation seed into both semantic identity and the exact outbound request;
   and
8. expose an authenticated health check that recognizes only the authorized
   `document.summary.step@1` task and discloses no model/runtime identity.

This slice must not change runtime selection, model profiles, prompts, pipeline state transitions,
settings/UI, or the existing Ollama/llama.cpp runtime behavior.

## Scope

- Add one private `gateway_client` module over the merged gateway ledger.
- Add strict request/response/error/health codecs for gateway protocol v1.
- Add production reqwest transport with HTTPS, no proxy, no redirects, bounded response reads,
  operating-system roots plus explicit CA trust, and Unix-owner-private token loading.
- Add deterministic fake-transport tests for completed replay, ambiguity, acknowledgement recovery,
  credential/config boundaries, and malformed envelopes.

### Acceptance criteria

- First submission writes the exact request-body SHA-256 before the transport observes the POST.
- Request stage and ordinal must match the durable ledger key before any reservation or transport.
- Fractional creation times round expiry upward to the next whole second, so even the minimum
  accepted lifetime remains fully available.
- A transport failure leaves one submitted identity; retry sends byte-identical JSON with that ID.
- An unresolved submitted request stops before transport at its exact expiry boundary.
- A valid response is integrity-checked and durable with deployment/policy provenance before ACK.
- Persistence that reaches the exact remote-retention expiry returns the same durable local output
  without attempting an impossible acknowledgement.
- If ACK fails after local persistence, retry sends only ACK while the request remains remotely
  retained; after expiry it returns the same completed local output without network access or a
  false acknowledgement transition.
- If ACK transport or protocol handling fails while another caller acknowledges the row, or while
  the immutable retention window expires, a ledger reload returns the authoritative durable result
  instead of reporting a stale acknowledgement failure.
- ACK eligibility is checked before credential I/O and rechecked immediately after token loading;
  a successful acknowledgement is timestamped only after its response, so stale entry time or slow
  credential I/O cannot ACK expired work or backdate the durable transition.
- A failed token read reloads durable state and may return only an acknowledged result or a
  completed result whose immutable retention window has expired; nonterminal state preserves the
  credential failure.
- Once the remote ACK succeeds, a transient failure to mark the already-durable local completion
  does not fail the pipeline; the Completed row remains eligible for later idempotent reconciliation.
- A local persistence error reloads and accepts only the same concurrent durable completion; a
  missing or conflicting row preserves the original failure.
- If reservation or submission is write-blocked, one shared fallback may return only a matching
  terminal Completed or Acknowledged row through the read path; nonterminal, missing, and
  conflicting rows preserve the failure.
- An acknowledged request returns its local output without another transport call.
- If another caller completes or acknowledges the row between reservation and submission, the
  reloaded transition state returns that local output without another inference POST.
- If another caller completes or acknowledges the row while a replay is in flight, a failed replay
  reloads and returns that durable local output instead of propagating the stale remote failure.
- Seeds from zero through signed 64-bit maximum are transmitted and bind semantic identity;
  larger values fail before reservation or transport.
- Wrong IDs, versions, media types, statuses, provenance, retry directives, oversized bodies,
  redirects, unsafe Unix token/CA files, and non-HTTPS origins fail closed without secret
  disclosure.
- Failure envelopes are admitted only for the protocol-v1 HTTP-status, error-code, and retryability
  combinations actually owned by the gateway contract.
- An authentication failure is always non-retryable, including an ownerless failure before the
  gateway can bind a request identity.
- Health succeeds only for an available or degraded `document.summary.step@1` declaration.

## Mechanism

`GatewayClient::execute` validates and deterministically serializes the task envelope, then uses the
schema-v17 store for reserve, submitted, completed, and acknowledged transitions. The HTTP adapter
is behind a small private trait so lifecycle tests can observe the exact call order without opening
a weaker production URL. The production adapter is always direct HTTPS with rustls, operating-system
roots plus the configured private CA, explicit redirect refusal, identity encoding, and bounded
reads.

## Intentional

- This client is not yet a `ModelRuntime`: the next slice must give runtime construction an admitted
  run identity and database path rather than smuggling them through prompts or global state.
- The gateway manages model/runtime identity. The client persists only producing deployment ID and
  task-policy version returned by the protocol.
- `application_rejected` is not sent here. Locally persisted output is a durable receipt; pipeline
  semantic validation remains independent and restartable from that private row.

## Deferred

- Runtime-factory/run-context wiring and immutable gateway model-profile snapshots.
- Settings persistence, Health/UI selection, installer defaults, and direct-runtime deprecation.
- Windows ACL validation for the gateway token and CA files; the gateway runtime must not be enabled
  on Windows until that owner boundary lands.
- Cancellation while a blocking gateway request is in flight.
- Live appliance and Windows release proof.

## Verification

- `cargo test gateway_client -q` — 25 passed.
- `cargo test gateway -q` — 30 passed.
- `cargo test --all-targets -- --skip connect::provider::tests::entitlement_gates_manifest_jobs_and_status_while_registration_stays_owned`
  — 471 library tests passed, 13 ignored, 1 filtered; 3 office tests passed and 3 ignored; 3
  release-contract tests passed.
- Exact isolated execution of the pre-existing skipped Connect test — 1 passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- `cargo tree -e features -i reqwest@0.12.28` — confirms
  `rustls-tls-native-roots` and no reqwest WebPKI-root feature.
- `cargo fmt --check` — passed.
- `git diff --check` — passed.

## Estimated diff size

Actual: five files, 2,736 added lines, and 12 removed lines. This exceeds the draft estimate because
the production security/credential boundary, protocol codecs, durable lifecycle, and two-sided
failure probes are one reviewable transport unit; splitting its tests from the private client would
reduce neither risk nor total surface. Runtime/UI work remains explicitly excluded.
