# Independent review and integration

Two fresh Astra reviewers assessed the feature and its source-availability correction. Both worked read-only.

The initial review covered the feature diff against `7495f8c120c8cb8acb6f2f5c943fbedcb75d11f6`. It found one availability defect. The last 100 messages globally could exclude every valid source for a selected bot. The review found no other correctness or security defect. Native authorization, source ownership, revision-zero writes, preserved edit provenance, exact journal recovery, and disposed callbacks passed source review.

The parent added a regression using a captured real-manager workspace filled to the manager's 256-message limit with later messages for another bot. The [before run](integration/source-limit-before.log) failed with `Later messages from another bot must not hide an existing Memory source`. Native projection now includes the manager's full bounded message set. The renderer accepts 256 previews and rejects 257. The [after run](integration/source-limit-after.log) passes that regression and the complete initial-creation contract.

The fresh focused review approved the correction against the manager's actual admission limit and snapshot behavior. Neither review found introduced comments, suppressions, or unenforced comment constraints. No review findings remain unresolved.

The first reviewer also audited the local decision trails and all 28 original evidence hashes. The abandoned worker-identity choice has an explicit superseding entry. The recorded provider requests, saved document, and historical run views agree. The focused reviewer checked the new regression's recorded failure against the parent's trail.

The parent independently replayed type checks, formatting, the production frontend build, 65 frontend tests, native all-target Clippy with warnings denied, and the real-manager creation contract. One unrelated external-fixture frontend test remained skipped. The native contract verifies source validation, creation conflicts, exact reconciliation without replay, manager and desktop restart, new-run provider context, and unchanged historical snapshots. See the [integration record](integration/results.json).

The parent visually inspected the before, selected-source, and saved-Memory images. The interaction video remains an explicitly labeled component replay. Packaged window and operating-system keychain acceptance remain unverified. Source messages must already exist. Full P01 remains open.

Model the Domain keeps creation provenance distinct from an existing edit base. Type System Discipline brands selected message identifiers at the strict boundary. Prove It Works requires actual-manager restart and captured provider input. Sequence Work into Verifiable Units keeps the independently reviewed correction separate from the feature implementation.
