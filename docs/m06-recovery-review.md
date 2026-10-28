# M06 recovery and Linux packaging review

Reviewed `4483b18..2b82b7d` plus the working-tree Python docstring deletions. This is an independent read-only review of recovery and Linux packaging. Connector work and untracked coordinator documents are outside this review. No repository source was changed. No remote service was stopped or started.

## Findings

### P2. Worker relocation leaves absolute configuration and execution references behind.

Location: `manager/packaging/recovery.py:448` and `manager/packaging/recovery.py:471`.

`references()` permits certificate and runtime paths inside the private state root. `restore_validated()` then allows that root to move to any new destination, but copies configuration and persisted process evidence without relocating their absolute paths. The actual worker opens certificate paths from `config.json` directly in `agent/src/transport.rs:48-51`. `docs/m06-recovery.md` explicitly promises that the new state root may differ.

Reproduced with an isolated private state directory, a schema-2 worker journal, a certificate inside that directory, and a configuration whose three certificate paths reference it. Created the bundle while substituting only the Linux quiescence check, renamed the original root, then restored into a different directory. Restore returned success. The certificate existed inside the restored root, while `Path(config['client_cert']).exists()` was false. No network, production credentials, or active worker was involved.

The smallest safe correction is to require the original absolute worker state path for restore, before staging or publication, and update documentation and tests. Retire and rename the source before restoring. If relocation remains supported instead, configuration and persisted execution references need an explicit domain-aware migration. Manager relocation can remain supported.

The coordinator has acknowledged this finding and owns the correction.

### P2. Diagnostics reject every valid configured Vibe runtime.

Location: `manager/packaging/diagnose.py:46`.

`runtime_hashes()` calls `value['files'].items()`. Vibe's existing manifest stores `files` as a list of `[path, sha256]` pairs, produced by `agent/vibe/seal.py:19-20` and consumed as `Vec<(String, CertificateFingerprint)>` by `agent/src/vibe.rs:42`. A valid Vibe runtime therefore raises `AttributeError`, which is also outside the CLI's handled exception set. This blocks the documented worker diagnostics and any installation acceptance depending on it.

Reproduced by creating a private runtime with one file, a correctly hashed manifest using the production list shape, and a correctly hashed `vibe.json` configuration. Calling `diagnose.runtime_hashes(state)` printed `AttributeError: 'list' object has no attribute 'items'`.

Iterate the list directly and exercise a valid Vibe manifest plus a tampered runtime file in the diagnostics tests.

The coordinator has received this finding and owns the correction.

## Rejected hypotheses

- Manager SQLite backup does not omit committed WAL content. It uses SQLite's backup API under the same flock as the service, and the recorded Rust and Python tests cover committed WAL and actual reopened manager state.
- A restored manager does not silently keep old local sign-in sessions or start with an unlocked vault. The actual State integration test asserts expiration, locked startup, original-passphrase unlock, recovered provider-secret decryption, history, Markdown, and artifact download.
- Selected guest executables do not lose executable permission merely because payload files are private. The manifest records original modes and restore reapplies them.
- The source service lock is not an unrelated advisory lock. The test drives the real manager lock and maintenance refuses it.
- Ordinary manifest path traversal and static symlink/hardlink payloads are rejected. Relative paths, ancestors, file types, ownership, private payload modes, unexpected content, sizes, and hashes are validated before publication.
- Multi-root partial publication is not concealed. It produces a failure naming published destinations; documentation explicitly requires services to remain stopped. It is not an atomic transaction across roots.
- Worker containers do not substitute a weaker process supervisor. The added check invokes the production `Backend::probe`, including scope start and stopped/empty-scope proof. The recorded Docker arm64 evidence covers that behavior.
- The manager launcher does not expose secret values in arguments. It passes private TLS file paths and requires loopback owner/setup listeners.
- The docstring deletions do not change executable behavior.

## Unresolved acceptance gates

The recovery/package report is appropriately narrower than full M06 completion. Its evidence does not establish native installation on the existing Proxmox x86_64 pair, Docker x86_64 behavior, Podman support, complete Computer and Vibe runtime installation, actual connector credential use after restore, or successful coordinated backup against the intended remote pair. The report lists these limits and does not claim them passed.

The two concrete defects above prevent an unqualified correctness verdict on this revision. After correction, the supported acceptance matrix must still be reported accurately. Passing isolated recovery tests and Docker arm64 process supervision does not close the outstanding live-installation and runtime gates. The retained-identity cutover remains an explicit operator assertion and cannot establish that an old disconnected VM is off. This limitation is documented and is not a newly discovered code defect.

No additional demonstrated data-loss, authority-escalation, or startup blocker was found within the reviewed subset.
