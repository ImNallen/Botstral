# Desktop attachment acceptance

The packaged macOS app selects and sends one local attachment, receives a recorded assistant response, approves a file task, and exports its result. Manager backups now preserve submitted and staged attachment bytes. This closes the scoped attachment check. Full P05 and C03 remain open.

## Corrections

The original package rejected `choose_run_file` before opening a dialog. Its command was absent from the application manifest and the main window capability. `clear_run_file`, `saved_results`, and `source_task` had the same configuration gap. All four commands now have explicit registration and permissions. The permission regression checks every native application command against both files. It failed before the correction with `choose_run_file is missing from the application manifest`.

The original backup tool rejected `run-files` with `unsupported manager state layout`. It now copies that directory and checks each submitted or staged attachment against its database length and SHA256 before backup and restoration. The existing backup format and older schemas remain supported.

## Observed native workflow

The [evidence record](attachments.json) preserves source hashes, the packaged binary hash, run identities, file digests, and platform versions. The graphical observations come from native computer-use controls in the task transcript. This record contains no video.

The unsigned debug package ran on macOS 27.0.1, build 26A434, on Apple silicon. Rust was 1.93.0, Node was 26.12.1, and Python was 3.9.6.

1. A disposable owner signed in and paired through the real macOS Keychain and native trust chooser.
2. The attachment chooser selected a 58-byte text file. The composer showed its filename. Removing a selected attachment cleared the composer.
3. A 16,385-byte file was rejected with the documented 16 KiB limit.
4. Sending the note completed a run with a recorded summary. Captured provider requests contained the exact bytes under `Untrusted attached file`. The stored owner prompt remained unchanged.
5. A second attached run proposed a file task. The graphical approval showed the note's exact text. Approval completed the task.
6. The native save chooser exported 65 verified bytes with mode `0600`. The bytes explicitly identify their synthetic worker origin.
7. **Saved results** listed the output and opened its retained source task.
8. A manager restart and a same-binary app relaunch each restored the completed runs. The app signed out and exited afterward.
9. An offline backup of that actual manager state passed verification. The original attachment directory was moved outside the state before restoration. Both restored blobs matched the uploaded bytes. Restored turn inputs retained the attachments.

Rebuilding the unsigned app changed its keychain identity and blocked access to the earlier disposable credential. That attempt was stopped. A fresh disposable profile completed acceptance. Both fixture credentials were removed. This does not establish signed update behavior.

## Repeatable checks

The focused native contract uses the actual manager and native transport. It verifies untrusted provider input, exact approval, export, restart, reconnection, and a second export.

```sh
python3 -B desktop/fixtures/paired_manager.py --verify-journal --journal-case p05_attachments_restart_and_export
cargo test --manifest-path desktop/src-tauri/Cargo.toml --no-default-features --locked --test permissions
cargo test -p botstral-manager --lib file_tests --locked
python3 -B -m unittest discover -s manager/packaging -p test_recovery.py -v
```

The manager restoration test also proves that a staged attachment can be submitted after restoration. Python tests reject missing, corrupt, symbolic-linked, and hard-linked blobs in both attachment tables. A rewritten manifest digest cannot override the database digest.

The [final check output](attachment-checks.log) records the passing results. The manager library had 192 passing tests and six ignored environment-specific tests. Desktop tests had 20 passing tests and 20 ignored fixture tests. The focused attachment fixture passed separately. Frontend tests had 49 passes and one skip. All 19 packaging tests, both component Clippy checks, formatting, and the packaged build passed.

## Limits

Provider replies are recorded locally. Worker execution and exported content are synthetic. These checks prove attachment transport, persistence, approval, and export. They do not prove new live Mistral reasoning or production Linux file execution.

The automated native contract substitutes an in-memory credential store. The graphical app uses the real macOS Keychain and native choosers. Linux and Windows desktop integrations, release signing, and updates remain unverified.

Attachments remain limited to one regular file per run, up to 16 KiB. Binary content, known secrets, and text beyond the transcript budget remain on disk without inline model access. Previews, OCR, editing, larger uploads, tray restoration, and delivered notifications remain open.
