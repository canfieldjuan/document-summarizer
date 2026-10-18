# Issue 113: store lifecycle, WAL, and nonblocking status reads

Status: PROPOSED; requires Juan's acceptance before implementation.
Contract-only change. No runtime adoption, installation, inference, or merge is authorized.

## 1. Decision and evidence boundary

Oversight message 3a041753 relays the operator decision to write this contract;
it does not accept this text. Oversight reviews this exact commit and takes it
to Juan. Implementation starts only after his acceptance.

The confirmed defect class is a request-time store opener that conflates
initialization, integrity validation, ordinary reads, and writes, plus a store
error mapper that loses contention identity. We reproduced a real authenticated
status request waiting behind a **synthetic exclusive lock**, then returning
HTTP 500 after the store timeout. We did not establish that a real worker held
such a lock during the original Windows failure. The historical cause remains
UNKNOWN; do not describe this contract as proving it.

PR137, the writer, PR138, and all prior PR116 holds remain held. PR138 is a
non-mergeable diagnostic. Its three-run native budget is exhausted; no fourth
measurement, including an accidental push-triggered run, is authorized.

## 2. Proven inventory before design

Code inventory is against diagnostic baseline b95b4564b4b8e736636f91d6fc767d1d9acbea9d,
with local-only diagnostic commit 3b035f0e661269fb29752252e208f854529431d2.

- `src-tauri/src/connect/provider.rs:1538-1567`: authenticated V2 status
  calls `db::init_db` at 1553 before reading the saved job. V1 does likewise
  at 1523. This V2 call was introduced by ff40d0b2.
- `src-tauri/src/pipeline/db.rs:238-243`: every `init_db` opens a writable
  connection, installs a five-second busy timeout, enables foreign keys, and
  invokes `schema::migrate`.
- `src-tauri/src/pipeline/schema.rs:1021-1022,1305-1306`: a current schema
  returns `validate`, starting with `PRAGMA quick_check`; it does **not** run
  migration writes on every request. The validation owner predates this arc
  (91dce3c). The observed journal mode in the reproduction was `delete`.
- `ProviderHttpError::store`, provider.rs:2468: accepts only a Display value
  and maps every store error to HTTP500/PROVIDER_INTERNAL_ERROR, retryable=true.
  `ProcessJobError::public_error` at 2098 also collapses store variants.
- Three native Windows measurements of the unchanged mock runtime passed:
  69 passed, two pre-existing ignored in each. V2 receipt totals (ms) were
  1691.4216/815.4067, 1304.4566/1040.1482, and 1196.2301/967.4505.
  These are fixture-runtime results, not 9B model qualification. A stage
  snapshot is sampled after the response, not during the slow request.
- The unchanged local diagnostic at provider.rs:5353 passed. No lock:
  HTTP200 in 4.344189ms. RESERVED write: HTTP200 in 3.258854ms. Synthetic
  EXCLUSIVE released after a 750ms hold: HTTP200 in 832.988946ms. EXCLUSIVE
  retained: HTTP500/PROVIDER_INTERNAL_ERROR in 5004.866121ms. The stored job
  was unchanged. The ten-second diagnostic HTTP timeout exposes the unchanged
  five-second SQLite timeout; no existing client timeout was changed.
- Uncontended initialization measured 3.049726ms on 430080 bytes and
  12.40621ms on a 33992704-byte synthetic copy. One sample at each size
  establishes neither an operating rate nor attribution of native delays.

Durable public-fixture evidence aliases and SHA256:

| Alias | SHA256 |
| --- | --- |
| local-contention-result.json | be0f991943e7c356d1fa83d9eb3c7f6e5c4986d5c4b71750d9f542c38ee0ddc1 |
| contention-provider-source.rs | 43942733e0c63cf735ef03887bdd0acfcca9124bbb5747f2656c1577f6b367b3 |
| native-1-receipts.json | d3eafe48908c16e69a8a77805add7858eb3767636c9e6e6f361eb88a2e7d603f |
| native-2-receipts.json | fda4def04067af1425872a23a6c36a9a2ae8727dee1330c6127f4350fcbcf76a |
| native-3-receipts.json | 117dfe18b4ee4660c77888a37483aee71706cb911fe8ff2f96c9a2e0a8967735 |

## 3. Required change surface and departures from the inventory

The store owner remains `pipeline/db.rs`; schema migration and integrity
validation remain in `pipeline/schema.rs`. Split initialization from normal
connection opening there, with explicit reader/writer roles. Do not add a
provider-local copy, a downstream error filter, or an unkeyed global initialized
boolean. Readiness must identify the actual database and its lifetime; replacing
a file/path or starting a second store cannot inherit another store's readiness.

Initialization occurs once per owning store lifetime, before requests, desktop
jobs, OCR recovery, or gateway persistence can start. Serialize competing
initializers across threads/processes; use OS-released ownership, not a stale
marker file. Existing provider lifecycle/registration ownership is preserved.
A second process must not migrate beneath an active incompatible owner.
Startup failure leaves the store unavailable; ordinary openers must never
silently initialize, create, repair, or change journal mode.

All production provider `init_db` callers must be classified and routed:

| Existing site (provider.rs baseline lines) | Required role |
| --- | --- |
| 765/767 provider startup | bootstrap at store owner; share readiness |
| 1251/1337 standalone reconciliation after ownership | owner initialization followed by writer |
| 1285 identity lookup before Windows ownership | retain observational read-only behavior; no bootstrap or side effects before ownership |
| 1523/1553 V1/V2 status | read-only, zero SQLite busy wait |
| 1695 idempotency lookup | read-only |
| 1800/1815 runtime-admission race lookup | read-only |
| 1890 guarded acceptance | writer; preserve atomic admission recheck |
| 1980/1982 worker job/pipeline persistence | writer |
| 2024 failure persistence | writer |

Read-only means SQLite READ_ONLY with no CREATE or immutable override, no
migration, quick_check, journal change, or writable fallback. Do not reuse a
shared mutable connection across worker threads. Writer opens retain foreign
keys and the existing bounded write wait; remove setup and integrity scans
from them as well. There is no change to model/admission/generation deadlines.

The same store also has non-provider callers: `lib.rs::open_database` (196)
and suggestion-owner setup (264), `DesktopJobManager` connection opens
(desktop.rs:177/196/227/272/304/345/355/537/634/693), and
`GatewayRuntime::generate` (gateway_runtime.rs:115). Route those through the
same initialization/opening owner, with a fresh source inventory at implementation;
do not leave another migration-on-every-operation path or move it into a runtime
factory. Update fixture initialization only where needed; isolated in-memory
migration tests remain explicitly initialized tests, not a production fallback.

Changes from proof are explicit: journal mode, connection roles/lifecycle,
contention classification, a narrowly justified SQLite dependency prerequisite,
and corresponding tests. No model, prompt, framing, sampler, summary/verifier
logic, document input, or prior qualification criterion changes.

## 4. SQLite safety prerequisite

The locked build uses rusqlite 0.31.0 / libsqlite3-sys 0.28.0; its bundled source
identifies SQLite 3.45.0. Upstream lists that release in the affected range of the
WAL-reset concurrency bug. Before enabling WAL, select a supported bundled
SQLite release containing the upstream fix (3.51.3 or later, or a documented
fixed backport), and record crate/source/runtime versions on Linux and Windows.
The minimal dependency/lockfile adjustment needed for this prerequisite is
in scope only after acceptance; no unrelated upgrades. Do not enable WAL on
the currently pinned vulnerable library or claim platform parity from Linux's
system SQLite version. Inability to supply a fixed bundled build is a STOP.

Reference: [SQLite WAL-reset fix](https://sqlite.org/wal.html#walreset).

## 5. Existing databases, migration, rollback, and sidecars

At serialized bootstrap: recover through SQLite, reject an unsupported future
schema without changing it, run the existing transactional migrations and
validation, then enable WAL outside a transaction and verify the returned mode.
Publish readiness only after all steps succeed. Do not add a schema-version
bump merely for journal policy. Migration rollback retains its existing SQL
transaction guarantee; it does not make the separate journal-mode transition
atomic. If migration committed but WAL activation fails, report startup failure,
preserve the valid migrated database, and do not publish or silently use DELETE.

Already-WAL databases reopen without conversion churn. Refusal/busy, read-only
storage, permission failures, invalid schema, and corruption stop readiness.
An in-memory fixture may use its supported journal mode, but is not WAL proof.
The operational deployment is same-host local app storage; unsupported storage
must fail readiness rather than pretend it has WAL guarantees.

Keep durability at least the previous effective setting: explicitly require
synchronous FULL and normal locking for operational writers. Retain SQLite's
bounded default passive auto-checkpoint policy; no blocking checkpoint or mode
switch in a request. Close read transactions promptly. Do not weaken sync,
disable checkpoints, or delete/truncate sidecars manually.

The database, WAL, and shared-memory files belong to the private application
directory. Prove Unix permissions and Windows ACL inheritance for newly created
sidecars and reopened stores. Read-only connections can still need WAL sidecar
access; do not use immutable=1 on a live store. Missing/denied sidecar access
must return a typed error, never a writable fallback or false not-found.

Crash/restart and downgrade tests must preserve committed job/result/lineage
and gateway-request records. Never back up a live store by copying only its
main file. Use SQLite's consistent snapshot/backup facilities for test receipts.
No automatic downgrade, destructive restore, or WAL-file deletion is authorized.
An explicit offline rollback to DELETE requires all users stopped, successful
checkpoint, closed handles, verified journal transition and integrity, and a
separate operator-authorized action. Until compatibility is proved, do not
claim that an older packaged binary is safe on the converted store.

References: [WAL persistence and sidecars](https://sqlite.org/wal.html),
[SQLite locking](https://sqlite.org/lockingv3.html).

## 6. Busy classification and concurrency invariants

Introduce one typed contention classifier at the store boundary. Preserve
SQLite BUSY/LOCKED identity, including extended codes and nested pipeline /
Connect store errors; do not parse Display strings. Both HTTP versions return
HTTP503, code PROVIDER_STORE_BUSY, retryable=true, through their existing error
envelopes. Keep auth/entitlement/UUID checks and existing 404 semantics. Missing,
corrupt, permission-denied, invalid-schema and disk/I/O failures must not become
busy. Genuine internal failures retain their existing internal-error category.
Reuse the classifier for persisted worker store failures without fabricating
completion or adding automatic retries, resubmission, or a new job state.

Operational status readers have zero SQLite busy timeout. A normal WAL writer
must permit a committed snapshot; unusual exclusive/recovery contention may
instead return prompt retryable busy. Do not promise that WAL can never be busy.
The controlled regression response ceiling is two seconds, while the writer
remains held; it must not wait for a five-second writer timeout or return HTTP500.

Preserve single-active-job uniqueness, transactional admission/idempotency,
request hash and protocol checks, state versions, cancellation, ownership,
restart recovery and result identity. Read-side lookups never authorize writes
without the existing atomic write-side recheck. Readers see committed data,
never dirty data or a stale cached completion. Writers remain serializable;
readiness is published only after initialization and torn down before ownership
is released. No async runtime thread is allowed to run full-store validation
as part of a status request.

Reference: [SQLite busy/locked result codes](https://sqlite.org/rescode.html).

## 7. Verification plan and settling evidence

Tests precede behavior changes. Reuse the public diagnostic fixture and actual
authenticated provider route; no replacement mock status server.

1. Fail-first: retain the old current-code EXCLUSIVE case returning500 at
   roughly five seconds. New regression must fail that implementation. Cover
   no lock, RESERVED write, active WAL writer, synthetic exclusive/recovery
   contention, and lock release. Label synthetic bounds honestly. Retained
   locks must not be released to manufacture a successful prompt response.
2. Assert READ_ONLY at SQLite level by attempting a write. Exercise both V1
   and V2, mixed concurrent reads/writes, unknown jobs, wrong protocol,
   unauthorized/expired entitlement, missing DB, corrupted DB and denied access.
   After readiness, an initialization/validation test spy must prove zero
   migration and quick_check executions per ordinary reader/writer open.
3. Test new/current/legacy/future/corrupt databases, repeated and concurrent
   initialization, migration failure rollback, interrupted bootstrap, WAL
   refusal, sidecar access failure, checkpoint/reopen, and an actual subprocess
   crash with committed plus uncommitted changes. Compare complete public
   fixture records before/after, not just row counts. Keep existing recovery
   and admission tests; include denied initialization and wrong-store readiness.
4. Reuse small/larger synthetic initialization measurements and record status
   time separately from bootstrap time. Correctness gates use unchanged saved
   values, zero request-time setup, and the held-lock response bound; no
   pre-registered throughput gain or invented historical-cause conclusion.
5. Kill targeted mutants: restore DELETE instead of WAL; return a writable
   status connection; rerun migrate/validate per request; restore the five-second
   read busy timeout; map contention to500/nonretryable; classify non-busy
   errors as busy; skip the atomic admission recheck; publish readiness before
   validation. Each must fail a real behavioral regression, not source text.
6. Run the isolated regressions, adjacent DB/schema/provider/desktop callers,
   format and applicable strict Clippy locally. Native Linux and Windows proof
   must exercise the same public fixture, actual SQLite runtime, concurrent
   held-writer status read and all contention/error invariants. Windows proof
   is required, not waived by a cross-compile or Linux success. Required CI
   retains its gates; no test ignores, raised existing deadlines or retries.

After acceptance, run the local regression/mutation gates, then publish the
separate fix candidate for its required native Linux/Windows CI qualification.
A native failure stops qualification for source attribution; do not rerun an
unchanged head to get green. These are future fix gates, not additional PR138
diagnostic measurements; no fourth diagnostic run is authorized.
No inference/GPU is needed. Archive source/receipts outside worktrees, with
private permissions and SHA256 aliases. Record failures and all changes from
the current baseline. Product implementation goes on a separate fix branch/PR;
no push of local PR138 diagnostic history just to trigger more native runs.

## 8. Cleanup, implementation summary, cold audit, and stop

Remove provider's Display-only contention collapse and every redundant
request-time initializer once its callers use the store owner; preserve other
error categories. Remove diagnostic-only success assumptions (the intentionally
expected500) when turning the reproduction into the regression. Do not add a
second read re-scan or a response filter while keeping the origin defect.

Current implementation summary: contract only; no production implementation.
Cold audit required before sending: commit contains only this document; every
named scope item traces to source or recorded proof; no code/test/dependency
change accompanies acceptance text. Prior local diagnostic remains a separate
commit and is not product repair.

Gap audit: NOT DONE. Acceptance, implementation, Linux/Windows regression and
mutation proof, review, adoption and merge remain undone. Send oversight this
contract's commit and durable fingerprint, then STOP. PR137 cannot merge until
the accepted fix lands and its required checks/reviews are satisfied; the
writer, PR138 and all PR116 holds remain.
