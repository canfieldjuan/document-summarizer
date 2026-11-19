# Issue 162 — regression coverage for untyped signature ByteRange

The checked PDF reader preserves a signature's cleartext `/Contents`. When the dictionary omits `/Type`, `is_signature_dictionary` recognizes a nonempty series of offset/length pairs made of nonnegative PDF integers. The existing untyped RC4 fixture covers the valid shape. The damaged AES annotation fixture has an explicit type and bypasses the untyped branch. As a result the committed suite can pass even if any one of the untyped range checks is removed.

## Reference experiment

At `f7f5d7b`, oversight exercised the vendored lopdf 0.42.0 checked loader with Rust 1.100.0 and the parser's synthetic AES-128 fixture. The reader and existing encryption tests are byte-identical at the merge on main, `be9e5a3`. The reference is the [delta review](https://github.com/canfieldjuan/document-summarizer/pull/158#pullrequestreview-5944340116) and its retained `reference-probe.py`, SHA-256 `7a0be34130319d68f3153f4bb0146696699a63e21f0674c6d3cf777e2df19850`. The control suite passed with four weakened implementations (M5–M8). Oversight reported that its temporary probe caught all four.

The probe adds a dictionary with no Type, a ByteRange array and a string Contents containing “note”. After encrypting, it flips the last IV byte of the one-block string. This gives a deterministic padding error if the reader attempts to decrypt Contents. Serialize with the existing fixture helpers and call `Document::load_mem_checked_encryption`.

| ByteRange (PDF object values) | Expected result |
|---|---|
| [0, 100, 200, 100] | Load succeeds and Contents equals the exact bytes supplied after the IV change. |
| [] | DecryptionError::Padding |
| [0, 100, 200] | DecryptionError::Padding |
| [0, -1, 200, 100] | DecryptionError::Padding |
| [0, Real(100.5), 200, 100] | DecryptionError::Padding |

## Implementation and acceptance

Port the five inputs and the existing oracle into `src-tauri/src/pipeline/parser_encryption_tests.rs`. Strengthen the positive assertion from successful loading to comparing the recovered Contents byte for byte. Make the failing case identifiable so each mutation can be tested independently. These are the intended differences from the temporary probe. The production owner is `src-tauri/vendor/lopdf/src/encryption.rs::is_signature_dictionary`. Its current behavior already matches the oracle. The implementation scope is test code only.

Run the five cases on the unmodified reader. In a disposable checkout, run the applicable case against each of oversight's four single mutations: omit the nonempty check, omit the even-length check, accept negative integers, and accept non-integer elements. Record an assertion failure for the empty, odd, negative and Real case respectively. Restore the original reader and verify the parser suite and applicable formatting and Clippy checks. Audit the final diff for test-only scope. This issue is complete when the five cases pass on the original reader and each named mutation fails its corresponding regression.
