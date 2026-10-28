# M06 integrated Linux acceptance

The integrated acceptance harness proves authenticated connector use, retained credentials after manager restoration, and Vibe stdio supervision on the existing separate Proxmox guests. It does not close M06 or replace the coordinator's independent reviews.

## Throughput checkpoint

One native build precedes three sequential live gates. Each fixture has one controlling driver. Native restoration and authority checks use independent temporary state and run alongside acceptance.

## Observed gates

The [report](evidence/m06-integrated-84b6d2fb/report.json) contains assertions, task approvals, durable call records, host identities, live process observations, and cleanup receipts.

| Gate | Verdict | Evidence |
| --- | --- | --- |
| Authenticated HTTPS through production manager authority and actual worker | VERIFIED | A pinned official MCP SDK server rejects absent and wrong bearer values. Approved work writes an exact file. A second bot has no grant. Revocation rejects cached task approval and submission. |
| Retained credential use after real manager backup and restore | VERIFIED | Both fixture services stop. Maintenance creates, verifies, and restores the bundle. Startup is locked. The original passphrase unlocks the vault. A new authenticated effect succeeds without credential entry. |
| Actual Vibe 2.24.2 local stdio within its existing task scope | VERIFIED | Vibe completes a call. A held call's child, wrapper, and descendant are independently observed in the same cgroup. The journal gains one Vibe execution. Cancellation empties the scope and removes every observed process. |
| Stale calls after live grant revocation | VERIFIED | The authenticated worker gateway rejects the cached binding. No stale file appears. The interrupted call retains an unknown external effect and revocation evidence. |

The manager restore also retains the original owner, worker identity, exact message text, authoritative memory, call history, and downloaded artifact bytes. Effect counts show no replay during restore or reconnection. The [native restoration test](evidence/m06-integrated-84b6d2fb/linux-restoration.log) separately exercises the existing actual-state restoration test.

The Vibe policy evidence comes from its actual generated configuration. Blanket permission bypass is false. File tools still require permission. Only the approved connector name has the exact connector permission. Live `/proc` reads and `cgroup.procs` establish membership independently of the connector's receipt. Cancellation checks both the emptied cgroup and absence of every observed process identifier. The journal comparison proves that the local connector did not create a nested worker job.

## Source and runtime identities

The starting commit is `6b4dfe9392b85af1c3c14c466faf059158b7a5bd`. The [deployment record](evidence/m06-integrated-84b6d2fb/deployment.json) identifies the transferred archive and SHA256 of the actual Linux manager fixture, production worker, Node binary, and dependency lock. The [source manifest](evidence/m06-integrated-84b6d2fb/source-files.json) identifies every file transferred for the final build. Documentation and evidence added after that build do not change executable inputs.

The Computer VM compiles the current source privately with Rust 1.93.0. The manager fixture binary is copied to the manager VM and verified by digest before startup. The worker uses the default production binary. Both guests are Ubuntu 24.04.5 x86_64 with systemd 255 and distinct hashed machine identities. Node is 24.22.1. The existing Vibe runtime is sealed as 2.24.2 without changing its installation.

The manager process uses the real `Manager`, owner HTTP router, public TLS listener, connector authority, connector transport, and persistent state. Its only substitute is recorded inference at the existing test provider boundary. The worker, connector HTTP client, gateway, relay, and Process supervisor are production implementations. No inference reaches a paid provider.

The external connector is a self-hosted server built with the official `@modelcontextprotocol/sdk` 1.32.2. Its bounded filesystem tool writes and reads actual UTF-8 file bytes. HTTPS uses explicit bearer authentication and the SDK Streamable HTTP transport. The local form uses the SDK stdio transport and the same filesystem backend. This is not the separately tested official `server-filesystem` package and does not claim acceptance of an OAuth-only service. Dependencies are locked, and installation scripts are disabled. Installation uses `npm ci --ignore-scripts` in a fresh private directory.

## Private connector trust

The sole production integration change enables reqwest's `rustls-tls-native-roots` alongside `rustls-tls`. The manager consequently trusts OS roots as well as bundled WebPKI roots. This applies to its reqwest clients, including self-hosted private connectors. Certificate and hostname verification remain enabled. There is no connection-level trust override.

The primary local sources are reqwest 0.12.28 `src/async_impl/client.rs`, which adds both root collections, and rustls-native-certs 0.8.5 `src/lib.rs`, which documents and implements `SSL_CERT_FILE`. The temporary manager receives only the public fixture CA path through that variable before startup. No global trust store changes. Operators can use OS trust or a process-scoped public CA file for private self-hosted connectors. Bundled public WebPKI roots remain available when the native loader uses a file override.

## Repeat the checks

Run from a clean checkout containing these changes. The SSH aliases must already have trusted host keys. Both guests need the documented existing runtimes and worker build tools. All credentials are generated synthetic fixture values. Owner passwords and secrets travel in private files or SSH standard input, never command arguments or process environment.

```sh
python3 manager/tests/m06_integrated.py --prepare --output /private/new-m06-evidence
python3 manager/tests/m06_integrated.py --output /private/new-m06-evidence
python3 manager/tests/m06_integrated.py --cleanup --output /private/new-m06-evidence
```

Preparation records a tracked-file source archive and builds Linux binaries under a unique private root. Stage new harness files before preparation when running from an uncommitted worktree. Each acceptance run uses new state beneath that root. Failures retain their verdict and stop fixture units. Cleanup refuses deletion while another same-account process holds a selected path. It removes only the roots named in the deployment record.

Applicable local checks are retained in [workspace tests](evidence/m06-integrated-84b6d2fb/workspace.log), [Clippy](evidence/m06-integrated-84b6d2fb/clippy.log), [packaging checks](evidence/m06-integrated-84b6d2fb/packaging.log), and [connector contract checks](evidence/m06-integrated-84b6d2fb/connector-contract.log). The [Linux authority tests](evidence/m06-integrated-84b6d2fb/linux-authority.log) run against the compiled final fixture binary.

## Limits and deviations

This run restores the manager bundle. It stops and restarts the retained original worker journal. It does not claim a second worker restoration or a full replacement-machine cutover. Worker restoration, packaging, Docker, and unsupported Podman modes retain the separate coordinator evidence and limits in `m06-recovery-acceptance.md`. The sealed shared runtimes are neither modified nor included in a new worker bundle.

The ordinary connector task and Vibe calls are exercised live. Durable run orchestration is covered by the Linux authority tests and workspace tests. This run does not add a separate model-created durable run on the two VMs. The remote HTTPS server stays external to the manager backup, so restoring manager state cannot undo its effects.

A pre-existing admission test used `now()+121` as an invalid deadline. Under a loaded run it became valid before the manager checked it. The test now uses `now()+3600` to preserve the intended refusal without the one-second race. The original failing workspace log was overwritten by the passing rerun. Other failed setup attempts corrected module placement, npm's macOS symlink-path handling, fixture resource ceilings, approval expiry, snapshot pagination, workspace identity reuse, and the differing owner and gateway authorization status codes. These were acceptance-driver errors. Their [retained attempt records](evidence/m06-integrated-84b6d2fb/attempts.json) remain failures or partial observations.

Installed services, real owner state, global trust stores, shared runtimes, and system packages were unchanged. The report records stopped fixture units, absent fixture processes, and removal of both fixture roots. Parent-owned desktop proof, independent final reviews, the pull request, and milestone checkbox closure remain open. Delegation was prohibited for this test owner, so this report and its [decision trail](../.audit/m06-integrated-84b6d2fb.tsv) have no independent review in this run.
