# PR contract: Windows Connect storage security

## Outcome

Document Summarizer may start its optional Local Connect provider on Windows
only when `%LOCALAPPDATA%\LocalConnect` satisfies the accepted shared-storage
contract. The provider publishes v1 and v2 registrations under fixed paths,
holds their one-byte ownership locks for its lifetime, and installs the shared
entitlement through the same protected storage boundary.

## Change surface

- Add a Windows-only storage helper for non-reparse traversal, trusted ownership,
  protected DACL validation, bounded regular-file reads, fixed-temporary atomic
  replacement, secure removal, and non-blocking one-byte locks.
- Route Windows provider registration publication, ownership, cleanup, and
  entitlement status/install through that helper. Keep the existing Unix paths
  and synchronization behavior.
- Run the Connect test namespace and strict Clippy on a native Windows GitHub
  runner, including hostile ACL, junction, temporary-residue, atomic replacement,
  rollback, and lock-contention probes.
- Record the platform contract and the remaining installed-demonstration limit.

## Non-scope

- Connect wire schemas, capability behavior, loopback transport, and job storage.
- Summary prompts, profiles, source evidence, result identity, and UI labels.
- Installer packaging or an installed Linux/Windows cited-summary demonstration.
- A generalized cross-application Windows storage crate.

## Acceptance

- Missing, relative, reparse-based, unreadable, null-DACL, unprotected, or
  over-broad Windows Connect-owned storage fails Connect closed while standalone
  use remains available. A safe inherited `%LOCALAPPDATA%` boundary remains
  usable; protection is required for the descendants the application owns.
- Registration and entitlement files are complete, flushed, privately secured,
  and atomically replaced from their fixed same-directory temporary paths while
  the applicable ownership or activation lock is held.
- Fixed registration names and persistent one-byte lock names match the accepted
  shared contract; a competing owner cannot publish over a live provider or
  mutate its active job before failing busy.
- Native Windows tests prove safe-path acceptance and reject hostile ancestor,
  file ACL, live and dangling junction, temporary-residue, and lock-contention
  cases. Existing
  Linux Connect tests remain green.
- Documentation does not claim an installed Windows demonstration until that
  separate proof is collected.

## Verification boundary

Cross-compilation proves that the Windows implementation and tests type-check.
Only the native Windows CI job proves the Win32 DACL, reparse, replacement, and
locking behavior exercised by these tests. An installed app demonstration is a
later release slice and remains required on both Linux and Windows.

The PR is complete when the native Windows job and existing Linux gates pass,
the security-shaped diff has a clean review with every current-head thread
reconciled, and the published branch equals the reviewed commit.
