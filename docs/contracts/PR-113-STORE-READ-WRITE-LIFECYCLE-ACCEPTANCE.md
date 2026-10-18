# Store lifecycle contract: operator acceptance

Recorded from oversight mailbox message 3ff50ff5, 2026-10-18T14:37:38Z.
This is a relayed operator decision; oversight's own review is not operator authority.

Accepted contract: 7ea27053cbd6a51ad71b403f22a1659016d8c66a,
`PR-113-STORE-READ-WRITE-LIFECYCLE.md`, SHA256
08a6c0ca7de516636aedbccd9bd1d47c6cffe1a87dd6c6a2175baa2d646ce983.
The exact reviewed document is preserved unchanged; this acceptance supersedes
its PROPOSED status, not its safety or evidence limitations.

## Decision relayed verbatim

On 2026-10-18, Juan answered **"Accept (Recommended)"** to this question:

> Accept Document Summarizer's fix contract for the stuck status checks (issue #113, commit 7ea2705)? It changes how the app opens its database. The database is set up and checked once at startup instead of on every request. Status checks read without waiting. The database switches to write-ahead mode so reads don't wait behind saves. A busy database answers 'busy, try again' (HTTP 503) instead of an internal error. It first upgrades the app's built-in SQLite to a version with a known corruption bug fixed (3.51.3 or later), and stops if no such build is available. Includes Linux and Windows proof, and no model or summary changes. #137 stays held until this lands.

Selected option:

> I relay your acceptance word for word. Document Summarizer records it in its own commit, then builds the fix test-first on a separate branch, after first checking that nothing copies or deletes the database file directly (write-ahead mode keeps recent changes in a second file). It pushes once for my review and native Windows CI. No install or promotion.

## Accepted file-handling condition and boundaries

Before enabling WAL, inventory every file-level handling of the store: copy,
move, rename, delete, hash, backup/export, reset-data, uninstall/installer scripts,
and test receipts, across source, desktop packaging, scripts and installers.
Use SQLite backup, or checkpoint plus close, or prove whole-directory deletion;
never retain a main-file-only snapshot of a live WAL database.

Use a separate fix branch from main. Import reviewed document content, not PR138
history. Build fail-first, run local gates and mutants, then push once for
oversight's exact-head review and native Windows CI. No fixed bundled SQLite
for both platforms means STOP. No install or promotion. PR137, the writer,
PR138 and PR116 holds remain until their explicit gates are satisfied.

This records acceptance; it does not upgrade the synthetic EXCLUSIVE-lock proof
into proof of the historical Windows timeout's cause.
