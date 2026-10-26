# Connect a worker

Use Linux or macOS with Rust 1.93.0. Build both production binaries with `make build`. The manager keeps its ordinary and setup listeners on loopback. Expose only the separate worker TLS port to the Computer VM. The worker needs outbound access to that port and no inbound application port. Reserve SSH for bootstrap and recovery.

## Provision certificates

Use your installation CA to issue a manager server certificate and a worker client certificate. The manager certificate must have server-auth usage and a DNS subject alternative name matching the daemon's configured hostname. The worker certificate must have client-auth usage. Supply each service with its peer's trusted CA certificate. Certificate PEM files may include intermediates after the leaf certificate.

Keep the CA private key outside both services. Install service private keys as regular files owned by the service account with mode `0600`. State directories require mode `0700`. Do not place keys, passwords, tokens, or invitation contents in command arguments or environment variables. The examples below pass file paths only.

## Start the manager

```sh
./target/release/botstral-manager --state-dir manager-state \
  --worker-listen 0.0.0.0:43122 \
  --worker-tls-cert manager.pem --worker-tls-key manager-key.pem \
  --worker-client-ca worker-ca.pem
```

Complete [local owner setup](local-api.md) on the manager host. The worker listener is optional. Configuring it requires all four worker options. Private `listeners.json` records the worker address in `worker`, or `null` when disabled.

## Invite and enroll the worker

On the Computer VM, print the public certificate fingerprint:

```sh
./target/release/botstral-agent identity --client-cert worker.pem
```

On the manager host, use the `call` function from the [protected local API example](local-api.md#protected-input-example). Sign in with protected password entry, then run this in the same interactive Python session before signing out:

```python
import os
fingerprint = input("Worker certificate fingerprint: ")
invitation = call(address, "POST", "/v1/worker-invitations", {
	"fingerprint": fingerprint,
	"lifetime_seconds": 600,
}, token)
fd = os.open("worker-invitation.json", os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
with os.fdopen(fd, "w") as file:
	json.dump(invitation, file)
del invitation
```

Transfer that private file through your bootstrap channel. Redeem it on the Computer VM before expiry:

```sh
./target/release/botstral-agent enroll --state-dir worker-state \
  --manager wss://manager.internal:43122 --manager-ca manager-ca.pem \
  --client-cert worker.pem --client-key worker-key.pem \
  --invitation-file worker-invitation.json
./target/release/botstral-agent check --state-dir worker-state
./target/release/botstral-agent run --state-dir worker-state
```

Enrollment prints `worker enrolled`. `check` validates the authenticated connection, negotiation, and snapshot, then exits. `run` stays connected and retries after disconnection. Its configuration contains the manager origin and certificate paths, never the invitation or private-key contents. Remove your invitation-file copies after successful redemption. Replaying a consumed invitation fails.

The configuration is saved before redemption. If the enrollment response is lost, try `check` with the same certificate. A committed enrollment remains discoverable without redeeming the invitation again. If enrollment never committed, retry the original invitation before expiry.

## Create sessions and take control

Use the authenticated owner API in [the worker protocol reference](worker-protocol.md). Create one logical session per existing bot, specifying the enrolled worker ID. Wait until its status is `confirmed`. Send the current generation and revision with every command or input request.

Changing a session's owner returns `pending`. Poll until the same revision becomes `confirmed` before treating takeover as complete. A disconnected or unresponsive worker cannot confirm that it stopped input. Other sessions retain their own revisions and continue independently.

Production rejects execution in M02. For local protocol testing only, build the separate debug worker:

```sh
cargo build --workspace --features botstral-agent/nonproduction-test-worker,botstral-manager/nonproduction-test-worker --locked
./target/debug/botstral-test-agent run --state-dir worker-state
```

This worker manipulates deterministic in-memory inputs. It provides no operating-system desktop, browser, shell, or model execution. Release compilation rejects the feature.

## Renew or recover certificates

Revoke the old worker through `DELETE /v1/workers/{id}`. Provision the replacement certificate and key. Issue an invitation for its new certificate fingerprint and enroll it. Repeat `POST /v1/computer-sessions` with each existing bot ID and the new worker ID. Rebinding preserves the session ID, advances its revision, and requires the old worker to be revoked. Stop the old daemon so it does not continue retrying.

To rotate a CA or manager certificate, stop the affected services, provision the replacement trust and certificate files, and restart them. Update the manager hostname when the server certificate's DNS name changes. There is no automatic CA rotation, certificate renewal, or insecure trust fallback.

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

Use addresses belonging to the two selected hosts. The main runner binds port 43122. The daemon probe binds port 43123. Keep those ports available for the test. Every invocation uses new private remote roots and unique systemd unit names. The tools stop their own fixture processes and preserve result files. A repeated certificate-generation command refuses a nonempty output directory.

The runner sends synthetic owner credentials, bearer tokens, and invitations over SSH standard input. It writes invitation input and certificate files with private permissions. Credential contents do not enter command arguments or environment variables. Log copies are redacted. Secret scans cover manager and worker state and service logs, excluding intentional certificate and invitation input files.
