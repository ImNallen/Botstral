# Connect a worker

Use Linux or macOS with Rust 1.93.0. Build both production binaries with `make build`. The manager keeps its ordinary and setup listeners on loopback. Expose only the separate worker TLS port to the Computer VM. The worker needs outbound access to that port and no inbound application port. Reserve SSH for bootstrap and recovery.

The manager creates its own Ed25519 key and self-signed certificate on first start. The worker creates its own key when it joins. Neither machine needs a CA, a copied certificate, a fingerprint, or an invitation file. Private keys stay on the machine that generated them. State directories require mode `0700`. Generated key files are mode `0600`.

## Start the manager

```sh
./target/release/botstral-manager --state-dir manager-state \
  --listen 127.0.0.1:43120 --setup-listen 127.0.0.1:43121 \
  --worker-listen 192.168.1.40:43122
```

Bind the address workers should dial. That address is copied into the join code. Complete [local owner setup](local-api.md) on the manager host. The worker listener is optional. When it is set, the manager serves public TLS with the key in `manager-state/manager-identity.key` and the certificate in `manager-state/manager-identity.pem`. Private `listeners.json` records the worker address in `worker`, or `null` when the listener is disabled.

## Invite and join the worker

On the manager host, invite a worker. The command signs in on the loopback owner API, prints one join code, and signs out. It reads the owner password from the terminal, or from standard input when there is no terminal. Do not put the password on the command line.

```sh
./target/release/botstral-manager invite worker --state-dir manager-state
```

The code looks like `bj1/worker/HOST:PORT/PIN/SECRET`. `PIN` is the manager's raw Ed25519 public key as 64 lowercase hex characters. `SECRET` is single-use and expires. Carry the code to the Computer VM through a channel you trust, then join before it expires:

```sh
./target/release/botstral-agent join --state-dir worker-state 'bj1/worker/...'
./target/release/botstral-agent check --state-dir worker-state
./target/release/botstral-agent run --state-dir worker-state
```

`join` generates the worker key, pins the manager, redeems the secret, and prints `worker enrolled`. `check` validates the authenticated connection, negotiation, and snapshot, then exits. `run` stays connected and retries after disconnection. `worker-state/config.json` stores the manager origin, the manager pin, and the generated certificate paths. It does not store a CA or the join secret.

The same secret redeemed again by the same worker key returns the existing worker. A different key is rejected. A worker that presents a key the manager never enrolled cannot connect. A manager certificate that does not match the pin is refused before enrollment.

`identity` prints the worker's public-key hex:

```sh
./target/release/botstral-agent identity --client-cert worker-state/worker-identity.pem
```

The owner API equivalent is `POST /v1/worker-invitations` with `{"lifetime_seconds":600}`. The response includes `code`.

## Create sessions and take control

Use the authenticated owner API in [the worker protocol reference](worker-protocol.md). Create one logical session per existing bot, specifying the enrolled worker ID. Wait until its status is `confirmed`. Send the current generation and revision with every command or input request.

Changing a session's owner returns `pending`. Poll until the same revision becomes `confirmed` before treating takeover as complete. A disconnected or unresponsive worker cannot confirm that it stopped input. Other sessions retain their own revisions and continue independently.

Production rejects the legacy M02 command route. Use [the approved process guide](w01-process-acceptance.md) to enable the first W01 guest-command runner on supported Linux hosts. Execution stays disabled without its private configuration.

For local protocol testing only, build the separate debug worker:

```sh
cargo build --workspace --features botstral-agent/nonproduction-test-worker,botstral-manager/nonproduction-test-worker --locked
./target/debug/botstral-test-agent run --state-dir worker-state
```

This worker manipulates deterministic in-memory inputs. It provides no operating-system desktop, browser, shell, or model execution. Release compilation rejects the feature.

## Replace a worker

Revoke the old worker through `DELETE /v1/workers/{id}`. Revocation takes effect immediately: the live connection stops and the revoked key cannot connect again. The worker row remains so existing session references stay valid. Invite a new code and run `join` on the replacement machine. Repeat `POST /v1/computer-sessions` with each existing bot ID and the new worker ID. Rebinding preserves the session ID, advances its revision, and requires the old worker to be revoked. Stop the old daemon so it does not continue retrying.

The manager key is the pin inside every later join code. Replacing that key means every worker and device must join again. There is no CA to rotate.

Two-VM transport and synthetic session acceptance passed on October 26, 2026. [The acceptance record](m02-two-vm-acceptance.md) describes the hosts, observed behavior, and coverage boundaries. The fixtures do not configure a permanent owner installation.

## Repeat the two-VM acceptance

Use existing Linux VMs with distinct machine identities, working key-based SSH aliases, and strict host-key checking. Install Rust 1.93.0 and a C compiler on the build host. Install `python3-websocket` on the Computer VM and `python3-websockets` on the manager. Enable the test user's systemd user manager to survive SSH disconnects.

Build both application variants from the commit under acceptance.

```sh
cargo build --workspace --release --locked --target-dir target/production
cargo build --workspace --locked \
	--features botstral-agent/nonproduction-test-worker,botstral-manager/nonproduction-test-worker \
	--target-dir target/m02-debug
make verify
```

Install the release manager on the manager VM at `/home/botstral/m02-acceptance/bin/production/botstral-manager`. Install the release daemon on the Computer VM at `/home/botstral/m02-acceptance/bin/production/botstral-agent`. Install the debug manager and test daemon under the corresponding `bin/test/` directories. Use private parent directories and executable binary files. These paths are the runner's fixture convention, not a release packaging contract.

Generate short-lived fixture certificates in an empty local directory. Keep its `private/` directory on the operator computer. The certificate generator includes deliberately expired and untrusted client fixtures.

```sh
bash agent/tests/m02_certificates.sh .audit/m02-certificates 192.168.1.40
python3 agent/tests/m02_two_vm.py \
	--manager-ssh botstral-manager \
	--worker-ssh botstral-computer \
	--manager-address 192.168.1.40 \
	--cert-dir .audit/m02-certificates/certificates \
	--output-dir .audit/m02-results
bash agent/tests/m02_daemon_probe.sh \
	.audit/m02-certificates/certificates .audit/m02-daemon-results \
	botstral-manager botstral-computer 192.168.1.40 192.168.1.235
```

Use addresses belonging to the two selected hosts. The main runner binds port 43122. The daemon probe binds port 43123. Keep those ports available for the test. Every invocation uses new private remote roots and unique systemd unit names. The tools stop their own fixture processes and preserve result files. The certificate generator and daemon probe refuse nonempty output directories.

The runner sends synthetic owner credentials, bearer tokens, and invitations over SSH standard input. It writes invitation input and certificate files with private permissions. Credential contents do not enter command arguments or environment variables. Log copies are redacted. Secret scans cover manager and worker state and service logs, excluding intentional certificate and invitation input files.

Verify the saved evidence and its negative controls from the matching runner checkout. Regenerate the aggregate record with the same command without `--check-controls`.

```sh
python3 -m unittest discover -s agent/tests -p 'test_m02_*.py'
python3 agent/tests/m02_record.py \
	--evidence-dir agent/tests/evidence \
	--application-commit e4d10c023ccd511c444faebff6b0b8fdf7fc5a06 \
	--check-controls
```

## Enable Computer work

Follow [Run Computer work](w01-computer.md) to install the pinned Linux runtime and opt into typed browser and file operations. The existing process profile and ceilings remain unchanged.
