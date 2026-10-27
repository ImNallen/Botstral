# M05 implementation acceptance

M05 implementation and protocol acceptance passed. The parent retains independent security, correctness, and comment review, PR creation, and integration. This implementation agent created no PR and pushed no branch.

## Source and commits

The assigned checkout is `/Users/nallen/.t3/worktrees/Botstral/t3-7f98da24` on `t3/m05-owner-device-authority`. It started clean at reviewed design commit `e2acd167a40acf5b6058ef50e4e819ca069b03fe`. Production code is frozen at `e2547e132732f8b7ed9774e4f1c54c4e3f6da5fa`. The following acceptance and documentation commit changes no production objects. `.audit/m05-source-manifest.json` records their identities, source archive hash, and binary hashes. The handback records the final exact head.

```
1d2ce30f4d5459c23de32c2b3ad62683b30a40be Add durable owner device authority and bound invitations
f8f1a1401b196473b111a3177d4f4fd53e2b11e9 Serve paired owners and workers through one pinned TLS protocol
c70a6909c49a0a6ed9d1922e60fb54312133d84c Preserve the existing public worker connection budget
909af8d6098e5602e46eba1f92e92a3ae38fab7d Preserve owner domain errors and inference verification semantics
e2547e132732f8b7ed9774e4f1c54c4e3f6da5fa Bound cancelled owner work and preserve admission fences
```

The final source archive SHA-256 is `db37badcbb22ff32a768d85ef9931b2f5beb6c2f2ac5c3309ceefaa0d2166a4a`.

| Linux artifact | SHA-256 |
| --- | --- |
| Manager | `c0aba18eddd8e9ac45f978fe6b28da49ddce738374acd74445ce4e9dbed9928f` |
| Native owner probe | `41022bce77ba935d5fa115ceecf146a1b6768dbb868edfd64bbd06fbc25bde50` |
| Retained M04 worker | `a1dffc665e7eb6176a53fe5f325a0874f20f4ea526e9eb23b7a24ba412365a00` |
| bore v0.6.0 | `60548b7a145ba334981b19fda7cd0210d24a108a3d0dc113919b33fd2eaa90ab` |

The retained worker came from the previous M04 acceptance bundle. Its associated source archive identifies `2c9a4356289538216ec45e5dcf9283d1f9785391`. The actual retained executable passed enrollment, reconnection, and the production coding proxy against the new Manager. Current worker builds also passed. No Agent production source changed in M05.

## Changed modules

- `link/src/owner.rs` owns typed identities, roles, operations, errors, reply transfers, and recovery bounds. `link/src/tls.rs` enforces exact pins alongside normal TLS validation. Existing schema export generated the JSON schema and TypeScript bindings.
- `manager/migrations/0008.sql` and `manager/src/manager/state/devices.rs` own durable Manager identity, bound invitations, device hashes, attempt limits, revocation epochs, and connection registration. Shared State methods authorize the private local-session or device-lease enum.
- `manager/src/public_http.rs` owns the single public listener. `worker_http.rs` retains worker routes and compatible entry points. `owner_http.rs` owns bounded owner operations, transfers, heartbeats, and committed-event hints. Local administration remains in `http/devices.rs`.
- `manager/src/owner_client.rs` and `manager/src/bin/owner-probe.rs` own protected reference storage, pairing, trust import, transfer verification, atomic application state, reconnect, heartbeat, dead-peer detection, and unknown mutation outcomes.
- The M05 acceptance scripts and bounds test are under `manager/tests/`. M01's schema assertions now expect version 8 and reject version 9. Architecture, connectivity, operator documentation, PLAN.md, and the append-only decision trail describe the implementation.

## Verification

| Command | Observed result | Retained log |
| --- | --- | --- |
| `scripts/verify.sh` | Passed formatting, clippy, default and test-feature builds, workspace tests, release build, and both forbidden test-feature release checks. Manager library had 60 passed and 2 environment-specific ignored tests. | `.audit/verify-final.log` |
| `python3 manager/tests/m05_acceptance.py --manager-bin target/debug/botstral-manager --probe-bin target/debug/owner-probe --output .audit/m05-local-source-final` | All 15 checks passed against actual binaries. | `.audit/m05-local-source-final.log` |
| `python3 manager/tests/m05_two_vm.py --binaries .audit/linux-bin --legacy-agent-bin .audit/linux-bin/m04-botstral-agent --bore-bin .audit/bore-linux --output .audit/m05-two-vm` | All 7 checks passed. Actual LAN, public relay, retained worker, reconnect, revocation, restart, and cleanup were observed. | `.audit/m05-two-vm.log` and `.audit/m05-two-vm/summary.json` |
| `cargo test -p botstral-manager --locked --lib m04_coding_production_worker_pinned_vibe -- --ignored --nocapture` | Passed on the Computer VM with the retained M04 executable, pinned Vibe 2.24.2, and recorded provider fixture. | `.audit/m05-coding-legacy.log` |
| `npm run check`, `npm test`, `npm run build` | Bindings and TypeScript checks passed. All 17 desktop tests passed. Desktop build passed. | `.audit/desktop-final-check.log`, `.audit/desktop-final-tests.log`, `.audit/desktop-final-build.log` |
| `cargo test --manifest-path desktop/src-tauri/Cargo.toml --no-default-features --locked` | Native protocol tests passed. | `.audit/desktop-native-protocol.log` |
| `cargo test --manifest-path desktop/src-tauri/Cargo.toml --no-default-features --locked --test local actual_manager_owner_bot_workers_and_sign_out -- --ignored --test-threads=1 --nocapture` | Existing native desktop service passed against the actual Manager. | `.audit/desktop-actual-manager.log` |
| `python3 -m py_compile manager/tests/m05_acceptance.py manager/tests/m05_remote.py manager/tests/m05_two_vm.py` | Passed. | Command exit 0. |

The Linux coding command set `BOTSTRAL_VIBE_RUNTIME=/home/botstral/m04-vibe-2.24.2` and `BOTSTRAL_RUN_AGENT_BIN=/home/botstral/m04-final-acceptance.GycLnu/bin/production/botstral-agent`. It used the isolated build source under `/home/botstral/m05-9660621780d3/source`, now removed. The recorded provider fixture made no paid model calls and used no real provider credentials.

Local acceptance covers enrollment races and lost responses, exact pin refusal before any application bytes, same-CA leaf replacement, trusted local rotation, public setup exclusion, cross-role refusal, oversized snapshots and ordinary replies, interrupted snapshots, retention rollover, subscription ordering, slow-viewer isolation with real worker control confirmation, blocked-send revocation, persistent revocation, capacity retry, native reconnect, silent-peer detection, and notification semantics. The heartbeat check observed 60.592 idle seconds with an unchanged durable authentication counter. State tests additionally cover wrong Manager, role, digest, expiry, rollback, persistent attempt limits, stale leases, and model shutdown fences.

## Snapshot and resource calculation

The complete eight-collection M03 image and its head come from one State operation, including filesystem document reconciliation. Its conservative serialized bound is 64,317,440 bytes. The decoded allocation allowance is 67,272,704 bytes. Both calculations include every admitted record count and string bound. JSON escaping uses a sixfold expansion allowance. [The detailed calculation](m05-snapshot-bound.md) includes layout checks, vector spare capacity, B-tree storage, serializer buffers, and bounded document reads.

One materialization permit, eight bounded viewers, and at most nine maximum transfer buffers including cancellation allowance keep additional owner recovery allocation below 1 GiB. The client needs at most 161 MiB of temporary and installed storage. Snapshots and ordinary replies share the 80 MiB transfer limit, 24 KiB chunks, ordered identities, byte counts, digest, completion marker, and 120-second deadline. Interrupted recovery cannot advance the application cursor. Blocking artifact work retains its permit after cancellation.

## Observed remote topology and cleanup

The Computer VM's native client connected directly to `192.168.1.40:43727` and through `bore.pub:8119`. The Manager VM's bore v0.6.0 process connected outward to bore.pub and forwarded opaque TCP to its unique fixture Manager. SSH bootstrapped fixtures and collected evidence.

The observed leaf SHA-256 was `82faef29f9908bdeeff72a8d6aa4ac7afd1a37aed94048d4cb9f2a530724f6ed`. It matched direct LAN observation, Computer VM relay observation, and operator relay observation. The native client reconnected after a Manager restart and atomically recovered 160 messages at cursor 161. Revocation terminated the active client and rejected later access, including after another Manager restart. The worker continued independently.

All recorded fixture PIDs were absent after shutdown. Every recorded transient unit ended inactive, including the expected authentication-failure unit. Both LAN and relay endpoints refused TCP after shutdown. Both private VM fixture roots and all temporary credentials were removed. `.audit/m05-build-cleanup.jsonl` also confirms removal of the separate build fixture root. Per-process sanitized logs remain under `.audit/m05-two-vm/` and `.audit/m05-local-source-final/`.

## Reconciliation and remaining scope

The implementation retains the reviewed authority, pairing, TLS, and recovery contracts. The native client uses a complete authoritative M03 image when events reference changed entities. This favors a simple durable projection. Its filesystem storage is the native protocol reference implementation. C02 retains graphical pairing, native keychain integration, and platform notification display. The durable deployment endpoint remains configurable, with a self-hosted rathole configuration documented in [remote owner access](remote-owner.md). The public bore route was temporary synthetic acceptance.

A stolen bearer credential can clone its device until revocation. Invitation binding prevents theft of the invitation alone from enrolling another credential. Short-code interfaces, an owner certificate authority, custom signature handshakes, and online pin rotation are excluded as reviewed.

No implementation blocker remains. Independent review belongs to the parent and has not been performed by this implementation agent. Successful self-tests are not an independent verdict.

Model the domain and type-system discipline produced separate authority and identity types. Boundary discipline kept setup and local administration out of public routing. Separate-before-serializing preserved the single State writer and independent viewer buffers. Prove-it-works required the real binaries and actual external data path. Attack-the-premise and fix-root-causes separated the deliberately throttled interruption fixture from silent-peer verification after byte and connection measurements identified the fixture delay.
