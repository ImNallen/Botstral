# Recover manager and Computer state

Stop services before backup. Run maintenance as the same account that owns the state. Use canonical absolute paths without symlink ancestors. Create backup and restore destinations under an account-owned directory with mode `0700`.

## Create and restore manager backups

Run the separate maintenance command. It does not start the manager or unlock the vault.

```sh
botstral-manager-backup create --state-dir /var/lib/botstral-manager --destination /private/backups/manager-01
botstral-manager-backup verify --backup /private/backups/manager-01
botstral-manager-backup restore --backup /private/backups/manager-01 --destination /private/recovered-manager
```

The source must contain an initialized owner. Backup acquires `manager.lock`, uses SQLite's online backup API while the service is offline, and validates SQLite integrity. The snapshot includes committed WAL transactions. Markdown sources, encrypted identity and credential records, owner history, and artifact bytes travel in one bundle. Every available artifact must match its durable ledger length and SHA256. Transfer `.part` files, SQLite WAL and shared-memory sidecars, and service locks are excluded.

Restore requires a new destination. It validates format, schema, component identity, root labels, paths, modes, sizes, file digests, SQLite integrity, and ledger artifact digests before publication. It rejects unexpected files, duplicate paths, symbolic links, hardlinks, special files, and populated destinations. It publishes through a private sibling directory. Manager startup preserves the original encrypted identity, starts locked, drops old local sign-in sessions, and uses existing recovery and reconciliation.

The bundle is private, not encrypted. Existing encrypted vault and credential fields remain encrypted. History, Markdown, and artifact contents remain plaintext. Copy bundles through an encrypted channel and keep them on protected storage. SHA256 detects corruption. The manifest is not a signature against an attacker who can rewrite the entire bundle.

## Select Computer recovery roots

Stop the worker and all guest browser, coding, and supervised task processes. Keep the systemd user manager available for stop verification. Select directory roots in a private JSON plan. Use `agent/packaging/recovery-plan.example.json` as the shape.

```sh
botstral-agent-backup create --state-dir /home/botstral-agent/.local/share/botstral-agent --plan /home/botstral-agent/recovery-plan.json --destination /home/botstral-agent/backups/worker-01
botstral-agent-backup verify --backup /home/botstral-agent/backups/worker-01
botstral-agent-backup restore --backup /home/botstral-agent/backups/worker-01 --destination /home/botstral-agent/.local/share/recovered-agent --original-instance-retired
```

The private worker root is always included. It preserves journal incarnation, execution records, process evidence, configuration, and internal coding homes together. Select external workspaces, browser profiles, coding homes, certificate directories, runtime directories, and tools explicitly. The certificate and runtime paths referenced by configuration must be covered. Overlapping roots are refused. All selected files must belong to the maintenance account. Symlinks, hardlinks, and special files are refused even inside selected tools or runtimes. Stop browsers cleanly so their transient profile links are removed. A runtime with retained links requires a different deployment layout or a separate reproducible installation before recovery. This tool does not silently dereference it.

Selected tool binaries are copied and restored as bytes with their executable modes. An inventory does not count as restoration. Unselected tools, operating-system packages, external services, Proxmox configuration, and unselected guest roots are excluded. Reinstall the documented operating system and package dependencies before starting a recovered worker.

Worker creation requires Linux procfs, cgroup v2, an accessible systemd user manager, no active `botstral-*.service` units, and stopped journal process lifecycles. It scans same-account processes for browser, Vibe, and worker names and for open selected paths before and after copying. An inaccessible process check refuses backup. This is an offline maintenance boundary. It cannot prevent an operator from launching a new writer outside the service lock.

Retire the original worker before restore. `--original-instance-retired` records the operator's explicit retained-identity cutover decision. It cannot prove that a disconnected old VM is powered off. Never start both copies. External roots restore to their original absolute paths and must be absent. The new state root may differ. Multi-root publication is not atomic across filesystems. If publication partially succeeds, maintenance reports the published paths. Keep services stopped and resolve those paths before retrying. VM replacement changes host and boot identity, so the existing journal recovery remains authoritative and does not replay old process effects.

## Run verification

```sh
python3 -m unittest discover -s manager/packaging -p test_recovery.py -v
cargo test --locked -p botstral-manager offline_restore_reopens -- --nocapture
```

The Rust test creates a real configured owner state, history, authoritative memory, encrypted provider credentials, and a ledger-backed artifact. It invokes the maintenance CLI, reopens through manager state initialization, proves locked startup and expired old sessions, unlocks with the original passphrase, decrypts the saved credential, and retrieves the artifact through the manager download path. Python tests cover WAL content, service-lock refusal, corruption, unsafe links, malicious manifests, selected guest roots, executable tool restoration, retained worker incarnation, and unproved process refusal.

Worker root tests isolate filesystem recovery with a substituted quiescence check. They do not prove a live systemd host. Actual connector credential recovery, independent review, remote installation, and supported container supervisor evidence remain separate acceptance gates.
