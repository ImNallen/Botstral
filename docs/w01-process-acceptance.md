# Verify the first production process unit

Run the production fixture on an ordinary Linux guest account with a running systemd 255 user manager. Execution stays disabled unless the private agent state contains `process.json`.

Use this configuration for UID and GID 1000. The file must have mode 0600 in the existing mode 0700 agent state directory.

```json
{
  "profile": "linux_systemd_v1",
  "account": {"uid": 1000, "gid": 1000},
  "ceilings": {
    "runtime_seconds": 300,
    "output_bytes": 1024,
    "processes": 32,
    "memory_bytes": 268435456
  }
}
```

Restart the daemon after changing configuration. The worker probes actual prerequisites before advertising the capability. An approved process request remains unclaimed when execution is disabled. Use the authenticated `POST /v1/tasks` endpoint with `work.type` set to `process` and `work.spec` matching the fields described in [Approved production processes](w01-process.md). Inspect that exact task, then grant its existing operation approval.

## Run the Linux fixture

Run these commands from the repository root.

```sh
make verify
./scripts/verify-w01.sh /tmp/botstral-w01-evidence
```

The script builds default release binaries and runs the real manager and agent. It then builds the existing debug-only fault feature and tests crashes before start submission, after service startup, and before importing the helper's result. It does not enable fault injection in a release binary.

The production fixture verifies literal arguments and environment, exact byte output and file effects, approval and capability refusal, immutable submission conflict, one active execution, revoked and expired approvals, concurrent large output, nonzero exit, runtime limits, normal-exit descendant cleanup, cancellation escalation, heartbeats, control confirmation, manager restart, daemon crash, supported shutdown, independent supervision after an uncatchable crash, and replacement-journal refusal while the original descendants remain live. It compares an independent invocation ledger against the original execution identity.

## Run across two existing machines

Build default Linux release binaries. Put them in an isolated directory on the corresponding hosts. The fixture expects `BASE/bin/production/botstral-manager` and `BASE/bin/production/botstral-agent`. Keep the base separate from permanent owner state.

Generate short-lived fixture certificates locally. Keep the resulting private files outside the repository's tracked files.

```sh
bash agent/tests/m02_certificates.sh .audit/w01-tls 192.168.1.40
python3 agent/tests/w01_two_vm.py \
  --base /home/botstral/w01-570e76c4 \
  --manager-ssh botstral-manager \
  --worker-ssh botstral-computer \
  --manager-address 192.168.1.40:43123 \
  --cert-dir .audit/w01-tls/certificates \
  --output-dir .audit/two-vm
```

The fixture uses strict SSH host checking. Owner and setup listeners stay on manager loopback. Only the certificate-authenticated worker listener is reachable across machines. Credentials pass through protected files or standard input. Each run records machine fingerprints, binary hashes, process liveness, task evidence, shutdown results, private-file permissions, and secret scans. Its cleanup targets only recorded fixture units.

The two-machine fixture covers exact approved effects, duplicate submission, descendant cancellation, responsiveness across a heartbeat, manager restart, daemon crash, and preservation of evidence. The broader fault and limit matrix runs on the Computer VM. Reboots, hostile same-account escape, executable-byte pinning, and atomic exactly-once effects are not claimed.

## Review recorded results

The retained evidence is in `agent/tests/evidence/w01/`. The baseline process submission failed with HTTP 400 `invalid_json`, where the fixture requires HTTP 200. This demonstrates the missing production capability before implementation.

The final verification record and decision trail identify exact commands and outcomes. Independent review remains with the parent. This evidence does not mark full W01 complete.
