# M06 connector implementation result

The assigned connector implementation is complete. Full M06 acceptance remains open for parent integration and the gates below. No installed service, real owner state, paid inference, remote branch, or pull request was changed.

## Commits and paths

- `cef580b802e36ee3208e94da3eeefd771a0898e4` implements the contracts, owner APIs, durable authority/effect ledger, transports, task/run/Vibe integration, recovery, generated bindings, compatibility changes, and Rust tests.
- `cdc1542cb96d19cf367bf70f042c033ce1ad8339` adds actual-binary and stdio fixtures, captured evidence, and the operator/API documentation.
- The report commit contains this report, its unique decision trail, the complete changed-path manifest, and one documentation clarification. Its exact head is provided in the handoff.

The base is `4483b183671aec5bc36a65fa11c25308fb127423`. The complete implementation path list is [m06-connectors-paths-d1feb4e5.txt](m06-connectors-paths-d1feb4e5.txt). The decision trail is [m06-connectors-d1feb4e5.tsv](m06-connectors-d1feb4e5.tsv). The integration API and configuration instructions are in [m06-connectors.md](../docs/m06-connectors.md).

## Delivered behavior

Owner connections have immutable connection, catalog, credential, and per-bot grant revisions. Catalogs, grants, protected bearer or named local environment secret entry, reconnect, rotation, terminal revocation, summaries, and redacted audit are available through local routes and owner WSS. SQLite triggers protect historical revisions and invocation identity. Secrets are encrypted with identity/transport/epoch binding.

Ordinary connector tasks retain exact task approvals and Process supervision. Vibe local stdio remains inside its occupied scope. Worker communication remains outbound. Remote bearer tokens never enter worker launch material. Both transports explicitly negotiate MCP 2025-06-18. Remote JSON/SSE, local framing, JSON schemas, output, deadlines, and concurrency are bounded.

The sole State writer validates current authority and commits a possible-effect marker before release. Local permits are single-use even after a lost response. Saved outcomes can be settled again without execution. Unknown effects stop the parent run or Vibe; restart does not replay them. Late results update evidence without automatic resumption. Dynamic model definitions and task/Vibe approvals capture grant revisions, so revoke-and-regrant cannot revive cached work.

## Checks

All commands ran from this checkout. Logs are retained in `/tmp` under the names shown. The actual-binary JSON evidence is committed under `agent/tests/evidence/m06-connectors` and `manager/tests/evidence/m06-connectors`.

| Command | Result |
| --- | --- |
| `cargo test --workspace --all-features --locked` | 124 passed, 11 platform/fixture tests ignored. Final committed-source run: `/tmp/m06-committed-workspace.log`. |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | Passed. `/tmp/m06-clippy-final.log`. |
| `cargo fmt --all --check` | Passed. |
| `cargo fmt --manifest-path desktop/src-tauri/Cargo.toml --check` | Passed. |
| `cargo test --manifest-path desktop/src-tauri/Cargo.toml --locked` | 14 passed, 8 paired-manager/keychain tests ignored. `/tmp/m06-desktop-tests.log`. |
| `npm run check` | Generated binding drift checks and both TypeScript projects passed. `/tmp/m06-ts-final.log`. |
| `npm test` | 26 passed, 1 existing graphical test skipped. `/tmp/m06-ui-tests.log`. |
| `python3 agent/tests/m06_connector_contract.py` | Six tests passed. Real stdio, shared digest vectors, clean environment, redaction, bounds, deadlines, live revocation, fresh pre-call authority, lost permits and settlement responses. `/tmp/m06-python-final.log`. |
| `python3 agent/tests/m06_vibe_contract.py --vibe /Users/nallen/.local/bin/vibe --report /tmp/botstral-m06-vibe-contract.json` | Actual Vibe 2.24.2 passed dynamic tools and real MCP/file denial callbacks without paid inference. `/tmp/m06-vibe-contract-final.log`. |
| `python3 agent/tests/m06_vibe_local.py --vibe /Users/nallen/.local/bin/vibe --report /tmp/m06-vibe-local.json` | Actual Vibe, production relay, real stdio child, and synthetic mutual-TLS manager passed. One observed effect per invocation; unknown stopped further inference. `/tmp/m06-vibe-local-final.log`. |
| `python3 agent/tests/m06_official_stdio.py --node "$(command -v node)" --server /tmp/botstral-m06-official-mcp/node_modules/@modelcontextprotocol/server-filesystem/dist/index.js --report /tmp/m06-official-stdio.json` | Real official filesystem server 2.0.1 passed exact negotiation, discovery of 14 tools, and a temporary file effect. `/tmp/m06-official-stdio-final.log`. |
| `cargo build -p botstral-manager --locked` | Passed. `/tmp/m06-build-final.log`. |
| `python3 manager/tests/m06_owner_connector.py --manager target/debug/botstral-manager --report /tmp/m06-owner-binary.json` | Actual manager passed protected local administration, encrypted storage, locked restart, reconnect, rotation, revocation, and audit redaction. `/tmp/m06-owner-binary-final.log`. |
| `python3 manager/tests/m06_owner_wss.py --manager target/debug/botstral-manager --report /tmp/m06-owner-wss.json` | Actual manager passed TLS enrollment, owner WSS operations, local parity, and snapshot summaries. `/tmp/m06-owner-wss-final.log`. |
| `git diff --check` | Passed before each implementation commit. |

The official server was installed only in `/tmp/botstral-m06-official-mcp` using `npm install --prefix /tmp/botstral-m06-official-mcp --ignore-scripts --no-audit --no-fund @modelcontextprotocol/server-filesystem@2.0.1`. It used Node 26.12.1 and no external secrets. The fixture records its entry-file digest. The actual Vibe binary digest is `1ffaf2dd0540ce5d42faa3c8713b166b9943524c411863153b3d7b32b2e925a2`. The actual manager tested by both owner fixtures has digest `8411fa9717f93d9507a4d80fbb62f322926feedb4aabfd005c057161f8525deb`.

The first Vibe proof preceded implementation. The final proof was rerun after adapter integration. Rust tests cover two bots with different grants, stale model/Vibe calls, exact approval, fresh release authority, immutable epochs, credential rotation and redaction, atomic markers, lost replies, SQLite reopening, worker-generation changes, unknown effects, late evidence, dynamic provider responses, call limits, and artifact-to-ledger result identity. The full workspace suite also runs the actual agent transport binaries with temporary state.

## Integration notes and remaining gates

- `manager/src/lib.rs` was not changed. Merge the parent's `#[cfg(test)] mod backup_tests` addition in `manager/src/manager/state.rs` and `pub mod maintenance` addition in `agent/src/lib.rs`; the connector additions are independent. No parent recovery commits were cherry-picked.
- Worker protocol 4, owner protocol 2, and migration 9 are deliberate. Generated JSON Schema and TypeScript are committed. Existing M01 migration assertions and M05 owner protocol fixtures were adapted. Backup interfaces were not edited or coupled to a hardcoded version.
- `ConnectorToolPolicy.runner` is the minimal addition needed to lower an ordinary approved connector task. Remote discovery uses an ordinary worker task and requires a connected worker/session, as permitted by the design verdict. Run connector results reserve 4096 bytes for payload within the existing 8192-byte result envelope. Connector and Vibe task specifications retain the existing 2048-byte serialized bound.
- The only desktop behavior change is the exhaustive Connector action description in the existing exact approval review. The parent should capture the required approval-screen evidence if the integration pull request includes this visible text. No approval is granted automatically.
- The parent still must prove real authenticated remote connector use and credential use after integrated restore. The official local server proof does not satisfy those gates.
- Linux Process scope termination and Vibe single-slot/cgroup integration remain parent-run acceptance. The macOS fixture proves a common wrapper parent and correctly reports that it cannot prove systemd scope membership. The existing Linux, paired-manager, and keychain tests remain ignored until their required fixtures are supplied.
- Parent packaging, Proxmox, independent installation, restore acceptance, and independent review remain open. This child did not run the full release packaging script or alter live services. The implementation received a self-review and deslop pass; it did not receive a separate child review because delegation was prohibited.
