# M02 two-VM acceptance

M02 transport and synthetic session ownership passed on two Proxmox virtual machines on October 26, 2026. This closes the deployment condition in [the M02 plan](../PLAN.md#m02-connect-the-shared-computer-and-track-bot-sessions).

The application binaries came from commit `e4d10c023ccd511c444faebff6b0b8fdf7fc5a06`. The acceptance tools add no production behavior. Source hashes identify the runner files used for each recorded run. The raw results, shutdown records, Linux verification log, binary hashes, and decision trail are retained under `agent/tests/evidence/raw/`. [The recorded results](../agent/tests/evidence/m02-two-vm.json) contain individual outcomes, host observations, and binary hashes.

## Acceptance environment

| Component | Manager | Computer |
| --- | --- | --- |
| Proxmox guest | 100, `botstral-manager` | 101, `botstral-computer` |
| Address | `192.168.1.40` | `192.168.1.235` |
| Operating system | Ubuntu 24.04.5 LTS | Ubuntu 24.04.5 LTS |
| Kernel | `6.8.0-147-generic` | `6.8.0-147-generic` |
| Virtual processors | 4 | 12 |
| Memory | 8 GiB | 32 GiB |
| Disk | 128 GiB | 512 GiB |

The hosts share the `192.168.1.0/24` network through Proxmox's `vmbr0` bridge. The daemon connects directly to the manager's mutual-TLS listener at `192.168.1.40:43122`. SSH carries test orchestration and protected inputs. It does not tunnel worker traffic. Owner and setup HTTP listeners remain on manager loopback addresses.

Rust and Cargo 1.93.0 built the application. The worker probe uses Ubuntu's `python3-websocket` package. The direct daemon probe uses `python3-websockets` on the manager. Separate machine identities and interface addresses are checked before each fixture starts.

## Verified behavior

The release manager and release daemon passed these checks across the two VMs.

- Certificate-bound enrollment rejected mismatched, expired, and consumed invitations.
- Invalid server trust, hostname, client trust, and expired client certificates failed explicitly.
- Worker TLS routes excluded owner, vault, approval, device, and security operations. Invitations did not authenticate owner requests.
- Unsupported versions, channels, capabilities, first frames, and privileged message variants failed before work admission.
- The actual daemon reached ready state. Restart preserved its identity and advanced its generation. Revocation closed its current connection and rejected another connection.
- Production work, approved-operation execution, and owner input reached `unsupported_execution` receipts.

The feature-enabled debug manager and labelled test daemon passed the session checks. Both sessions first applied synthetic effects. Confirmed takeover suspended session A with a fixed applied count while session B continued under unchanged ownership. Current owner input reached `applied`. Old owner revisions were rejected after both control changes.

Cancellation produced a confirmed `cancelled` event. Its applied count matched the later result. A new three-step task completed exactly three effects. Identical command retries reused their command identity. Changed command content conflicted. Reconnection advanced generations and revisions, rejected old commands, and reported old in-memory task outcomes as unknown.

The separate mutual-TLS daemon probe sent frames directly from the manager VM to the real test daemon on the Computer VM. It first confirmed that a current input receipt applied an effect. It then verified stale revision and generation rejection. A delayed receipt for bot input sent before takeover produced no effect after confirmed takeover. Returning control to the bot did not replay the old producer.

Every recorded fixture daemon and manager exited successfully after SIGTERM. Private file modes and synthetic-secret scans passed. Fixture units stopped and the test listeners closed. Isolated state remains private for inspection. The certificate authority private key stayed on the operator computer.

## Coverage boundaries

`make verify` also passed on the Linux Computer VM. It covers schema drift, migration, local actual-binary transport, queue limits, malformed frames, invitation races, lost enrollment responses, input fencing, cancellation, and production separation. Fresh release builds excluded the test binary. Both release builds with a test feature failed as required.

The cross-VM runner explicitly marks five extensions as `not_run`: socket permit exhaustion, silent and malformed frames, concurrent invitation redemption, lost enrollment response recovery, and additional local stress. Their Linux suite results do not become cross-VM claims. The separate daemon probe supplies direct cross-VM fence evidence.

M03 still owns durable task journals, replay, approval consumption, and recovery after crashes. W01 owns production execution. W02 owns independent desktops. Synthetic inputs do not prove operating-system input, browser work, artifact transfer, or process-group cancellation. The acceptance fixtures do not configure a permanent owner installation.

The aggregate JSON record is generated by `agent/tests/m02_record.py` from those raw artifacts. Its negative controls reject missing checks, changed hashes, failed shutdown, unfinished Linux verification, incorrect VM resources, and remaining fixture units or listeners.

See [the acceptance instructions](worker-deployment.md#repeat-the-two-vm-acceptance) for the exact build and run commands.
