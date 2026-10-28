# C02 acceptance record

Implementation is in progress. C02 is not yet accepted as a complete packaged remote workflow.

The extracted client passed `python3 manager/tests/m05_client_regressions.py --output /tmp/c02-m05-client`. Both exclusive ownership and uncertain committed mutation checks passed, together with transient TLS retry.

The actual manager and probe passed `python3 manager/tests/m05_acceptance.py --manager-bin target/debug/botstral-manager --probe-bin target/debug/owner-probe --agent-bin target/debug/botstral-agent --output /tmp/c02-m05-acceptance`. All 15 M05 contracts passed. The fixture removed its state and processes. These tests use isolated synthetic identities and loopback TLS. They do not prove the packaged desktop or a production Linux worker.

`cargo test --workspace --locked --lib --test m05_bounds` passed. The first full workspace run found a compatibility regression caused by imposing a new bot-creation limit. That limit was removed. Workspace checks its own image admission without changing old bot-creation behavior. The complete final verification remains required.

The parent owns independent security and correctness review, final native graphical acceptance, the configured remote route, screenshots and interaction video, integration, and the pull request. Linux Secret Service behavior, Windows builds, real Linux process cancellation, and production browser execution are not established by macOS service tests.

`python3 manager/tests/c02_workspace.py --output /tmp/c02-workspace-contract-2` passed. It checked the complete roster and session image, unchanged Resume fields, rejected changed normalized action arguments, rejected a stale permission revision, and confirmed the exact execution binding. A pre-extraction probe binary was unavailable in the parent checkout, so the separate old-binary gate remains unverified.
