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

Worker creation requires Linux procfs, cgroup v2, an accessible systemd user manager, no active `botstral-*.service` units, and stopped journal process lifecycles. It scans same-account processes for browser, Vibe, and worker names and for open selected paths before and after copying. The invoking ancestry and the systemd user-manager PID are excluded. Its PAM helper is excluded only when its parent and cgroup match that manager. Every other inaccessible process check refuses backup. This is an offline maintenance boundary. It cannot prevent an operator from launching a new writer outside the service lock.

Retire the original worker before restore. `--original-instance-retired` records the operator's explicit retained-identity cutover decision. It cannot prove that a disconnected old VM is powered off. Never start both copies. External roots restore to their original absolute paths and must be absent. The new state root may differ. Multi-root publication is not atomic across filesystems. If publication partially succeeds, maintenance reports the published paths. Keep services stopped and resolve those paths before retrying. VM replacement changes host and boot identity, so the existing journal recovery remains authoritative and does not replay old process effects.

## Run verification

```sh
python3 -m unittest discover -s manager/packaging -p test_recovery.py -v
cargo test --locked -p botstral-manager offline_restore_reopens -- --nocapture
```

The Rust test creates a real configured owner state, history, authoritative memory, encrypted provider credentials, and a ledger-backed artifact. It invokes the maintenance CLI, reopens through manager state initialization, proves locked startup and expired old sessions, unlocks with the original passphrase, decrypts the saved credential, and retrieves the artifact through the manager download path. Python tests cover WAL content, service-lock refusal, corruption, unsafe links, malicious manifests, selected guest roots, executable tool restoration, retained worker incarnation, and unproved process refusal.

Worker root tests isolate filesystem recovery with a substituted quiescence check. They do not prove a live systemd host. Actual connector credential recovery, independent review, remote installation, and supported container supervisor evidence remain separate acceptance gates.

## Install separate native services

Use Ubuntu 24.04 with systemd 255 and cgroup v2. Run the manager and Computer on separate VMs. The worker requires its own nonroot account, a running systemd user manager, and delegated memory and process controllers. Its configured Process account must match that account. Computer browser automation currently requires Linux x86_64. Linux arm64 acceptance covers Process supervision and recovery.

Build release binaries from the same source revision with Rust 1.93.0 and the checked-in `Cargo.lock`.

```sh
cargo build --locked --release -p botstral-manager --bin botstral-manager
cargo build --locked --release -p botstral-agent --bin botstral-agent --bin botstral-agent-supervisor-check
```

On the manager VM, stage the manager package. `--root` also accepts a fresh disposable directory for layout inspection. Installation refuses existing component files and enables no services.

```sh
sudo python3 manager/packaging/install.py --binary "$PWD/target/release/botstral-manager" --root /
sudo useradd --system --user-group --home-dir /var/lib/botstral-manager --shell /usr/sbin/nologin botstral-manager
sudo install -d -m 0700 -o botstral-manager -g botstral-manager /var/lib/botstral-manager
sudo chown -R botstral-manager:botstral-manager /etc/botstral-manager
```

Prepare `/etc/botstral-manager/service.json` using the installed `service.example.json`. Place the server certificate, private key, and worker CA at its configured paths. Make the configuration directory mode `0700`. Make all configuration and certificate files mode `0600`, owned by `botstral-manager`. The launcher accepts paths only for TLS material, rejects extra configuration keys, and refuses non-loopback local owner listeners. Keep the owner listener behind an authenticated tunnel or use the existing public owner TLS protocol.

```sh
sudo systemctl daemon-reload
sudo systemctl enable --now botstral-manager.service
```

On the Computer VM, install the separate worker package. Create its account only if it does not already exist.

```sh
sudo python3 agent/packaging/install.py --binary "$PWD/target/release/botstral-agent" --root /
sudo useradd --create-home --user-group --shell /bin/bash botstral-agent
sudo chmod 0700 /home/botstral-agent
sudo loginctl enable-linger botstral-agent
worker_uid=$(id -u botstral-agent)
sudo systemctl start "user@$worker_uid.service"
sudo -u botstral-agent env XDG_RUNTIME_DIR="/run/user/$worker_uid" DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$worker_uid/bus" systemctl --user daemon-reload
```

Enroll through the existing file-based worker CLI before activation. Use private certificate and invitation files. Configure `process.json`, Computer, and Vibe with their existing verified runtime manifests. The Computer runtime installer is `agent/runtime/setup.sh`. It pins Node 24.22.1 with a published SHA256 and Playwright through `package-lock.json`. Vibe remains pinned to 2.24.2 and its sealed distribution manifest. These runtime directories are instance installation inputs, not secrets baked into an image. A service or image does not imply that optional runtimes are installed. Use diagnostics to validate their hashes and backup coverage.

```sh
sudo -u botstral-agent env XDG_RUNTIME_DIR="/run/user/$worker_uid" DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$worker_uid/bus" systemctl --user enable --now botstral-agent.service
```

Run offline diagnostics after stopping the relevant service. The manager check verifies migration range, vault identity, state safety, and artifacts. It reports locked startup and does not attempt vault decryption. The worker check validates the recovery plan, configured runtime hashes, systemd version, and user bus. `--connect` also validates TLS files and performs a mutually authenticated TLS connection. `--probe-supervisor` invokes the production backend against a new scratch directory and proves a stopped empty scope. The standalone command offers the same proof without a configured installation.

```sh
botstral-manager-diagnose --state-dir /var/lib/botstral-manager
botstral-agent-diagnose --state-dir /home/botstral-agent/.local/share/botstral-agent --plan /home/botstral-agent/recovery-plan.json --connect --probe-supervisor
botstral-agent-supervisor-check --scratch /home/botstral-agent/new-supervisor-check
```

## Build and verify OCI images

Build each image independently from the repository root. The Dockerfiles pin Rust and Ubuntu by registry digest. Ubuntu packages resolve from the immutable `20261020T000000Z` snapshot. Each image records resolved package versions in `/usr/share/botstral-image-packages.txt` and its binary digest in the installed component's `install.json`. Rust dependencies resolve from `Cargo.lock`. Maintenance requires Python 3.9 or newer and SQLite 3.37 or newer. The pinned Ubuntu images provide Python 3.12.

```sh
docker build -f manager/packaging/Dockerfile -t botstral-manager:m06 .
docker build -f agent/packaging/Dockerfile -t botstral-agent:m06 .
python3 manager/packaging/container-acceptance.py --engine docker --image botstral-manager:m06
python3 agent/packaging/container-acceptance.py --engine docker --image botstral-agent:m06 --evidence /private/evidence/docker-worker.json
```

The manager image runs as UID and GID `10001`, with private persistent state and read-only configuration mounts. `manager/packaging/run-container.sh` drops capabilities and uses a read-only root filesystem. Configuration contains secret file paths, never secret values. Certificates and keys are private mounted files. The image exposes the public TLS port only. Local setup can run inside the container through its loopback interface.

The worker image uses systemd 255 as PID 1 and starts the UID `10001` user manager. It uses the existing systemd Process backend. `agent/packaging/run-container.sh` supplies the required rootful Docker or rootful Podman profile. Both require a dedicated Linux Computer host, cgroup v2 with memory and process controllers, `--privileged`, `--cgroupns=host`, a writable bind mount of `/sys/fs/cgroup`, temporary `/run`, `/run/lock`, and `/tmp`, and `SIGRTMIN+3` shutdown. Podman also requires `--systemd=always`. Rootless engines and ordinary unprivileged containers are refused. These permissions expose the dedicated host's cgroup hierarchy. Do not run this profile on the manager host or mount manager state into it.

Provide a private worker home owned by UID and GID `10001` when using the launch script. Enrollment configuration, certificates, selected workspaces, profiles, and runtimes live in that volume. The image's service activation is outside that home, so replacing the home mount does not hide its unit. The manager and worker scripts are separate. No combined deployment is supplied.

Run the same container acceptance with `--engine podman` and an equivalent locally built image before claiming that environment is supported. Docker Linux arm64 has observed production scope-stop and recovery evidence in this implementation run. Podman remains unverified. Docker Linux x86_64 and complete Computer and Vibe runtime installation remain separate acceptance gates. Diagnostics refuse an absent user bus, an unsupported systemd version, missing controllers, bad runtime digests, or uncovered external references.

## Capture installation acceptance on each host

Run these commands from the same checkout on the two Proxmox guests. Use the same commands on an additional compatible Linux installation. Each command requires a fresh absolute evidence directory. The agent command runs as the nonroot worker account with its systemd user bus available.

```sh
scripts/m06-accept-linux.sh manager /private/evidence/m06-manager
scripts/m06-accept-linux.sh agent /home/botstral-agent/evidence/m06-computer
```

The script stages a new native layout, records the source revision and kernel, and drives the relevant recovery or production-supervisor check. It does not replace or activate existing services. Full acceptance also requires authenticated worker enrollment, actual Computer and Vibe runtime configuration, connector credential recovery after connector integration, and end-to-end connector execution. The coordinator owns those live checks.

For an intentionally stopped production pair, `scripts/m06-backup-pair.py` coordinates installed maintenance commands through SSH aliases. It uses `sudo -n`, freezes new admission by stopping the manager, then stops the worker. It creates and verifies both bundles and records both backup identities and the stop timestamp in a private receipt. Stopping the manager first replaces a separate admission-freeze endpoint, which the baseline does not provide. Shutdown may retain uncertain external effects. Neither bundle snapshots remote services. Failure leaves services stopped. Restart occurs only after both bundles verify and only with `--restart-after-success`.

```sh
python3 scripts/m06-backup-pair.py --manager-host botstral-manager --worker-host botstral-computer --worker-plan /home/botstral-agent/recovery-plan.json --manager-destination /private/backups/manager-01 --worker-destination /home/botstral-agent/backups/worker-01 --receipt /private/backups/pair-01.json
```

Manager TLS and service configuration outside its state root are deployment inputs. Retain or reprovision those private mounted files separately. Worker external roots restore to their original paths. Stop the old worker permanently before retained-identity cutover. If the original worker state is present locally, restore also acquires its service lock and rechecks stopped processes before publication.
