# M06 approval display proof

The screenshots show the existing process approval and the new connector approval. Both use the unmodified React workspace and its production stylesheet. Their action text comes from the native `RemoteSnapshot` review serializer, including the exact execution details. The fixture rejects every mutation and cannot approve work.

The new review displays the connection identity, connection revision, credential epoch, catalog revision, grant revision, tool name, normalized arguments, worker, and session. Chromium's rendered text is compared exactly with the native serializer output before capture. [The capture record](evidence/m06-parent/approval-renderer.json) includes dimensions, browser version, image hashes, and the empty browser-error list.

- [Before, existing process approval](assets/m06-approval-before.png).
- [After, connector approval](assets/m06-approval-after.png).

These are renderer captures with synthetic review data. They do not claim a new packaged graphical workflow. Live production connector execution and exact manager approvals are verified separately in [integrated Linux acceptance](m06-integrated-acceptance.md).

## Repeat

Use the checked-in npm lockfile and Rust toolchain. The native generator refuses an existing output file.

```sh
npm ci --ignore-scripts
proof_root=$(mktemp -d)
cargo run --manifest-path desktop/src-tauri/Cargo.toml --no-default-features --locked --example m06-approval-proof -- "$proof_root/projections.json"
cp "$proof_root/projections.json" docs/evidence/m06-parent/approval-projections.json
npm install --prefix "$proof_root" --ignore-scripts --no-audit --no-fund --save-exact playwright@1.64.1
PLAYWRIGHT_BROWSERS_PATH="$proof_root/browsers" node "$proof_root/node_modules/playwright/cli.js" install chromium
PLAYWRIGHT_BROWSERS_PATH="$proof_root/browsers" node desktop/fixtures/m06-approval-proof.mjs "$proof_root/node_modules/playwright/index.mjs"
node_modules/.bin/tsc -p desktop/fixtures/tsconfig.m06.json
```

The driver builds a temporary local HTML artifact, inlines its assets, and captures it without a server. It closes its browser and removes its temporary build directory. The before action follows the process formatter already present before M06. The after action uses the new connector formatter. These represent two supported action types.
