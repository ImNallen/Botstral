# Independent production review

Verdict. Changes requested for one reproduced P2 defect in cf01259..f091f09.

## P2. Escape workspace paths before constructing tool permission globs.

`agent/vibe/runtime.py:58` inserts the literal workspace into each Vibe tool allowlist as `str(workspace) + "/*"`. `link/src/vibe.rs:136` accepts canonical paths containing glob metacharacters. Pinned Vibe 2.24.2 uses `fnmatch.fnmatch` on the resolved target path. An approved literal workspace `/srv/work[12]` therefore grants automatic permission to `/srv/work1/outside.txt`. Files inside the actual workspace do not match this allowlist. The same problem affects `*` and `?` in valid directory names.

This changes the explicit file-edit authorization and bypasses the adapter's intended approval denial for outside-workspace edits. It does not depend on treating the guest account as a sandbox. Escape the literal prefix with `glob.escape`, leaving only the final child wildcard active, and cover both the literal workspace and a matching sibling with the actual pinned permission evaluator.

Reproduction used the installed package at `/Users/nallen/.local/share/uv/tools/mistral-vibe`, whose metadata reported version 2.24.2. Calling `vibe.core.tools.utils.resolve_path_permission` with `cwd=Path("/srv/work[12]")`, `allowlist=["/srv/work[12]/*"]`, and no denylist returned `None` for `/srv/work[12]/owned.txt` and `ToolPermission.ALWAYS` for `/srv/work1/outside.txt`. No provider request or file modification was required.

## Coverage and verification.

Read the implementation contract, all changed production Rust and Python paths, the coding and vision tests, the Linux production test, shared provider parsing, process supervision, parent run transitions, and the existing token-detail and Linux CLI probe logs. Reviewed typed policy validation, mTLS identity and lease admission, per-call reservations, cumulative parent accounting, unknown-call recovery, exact resume identity, output collection, PNG authorization and decoding, and runtime shutdown registration.

The shutdown fix registers coding jobs before durable admission and retains them independently of the HTTP caller. Shutdown signals the registered jobs and drains settlement while the state queue remains available. The Linux test explicitly drops the worker listener during a sent provider request, then asserts bounded manager shutdown and a durable unknown hold. I found no further concrete defect in these paths. The Linux test was inspected but not rerun here.

Independent local checks passed. `cargo test -p botstral-manager coding -- --nocapture` passed six tests and ignored the Linux-only test. `cargo test -p botstral-manager vision -- --nocapture` passed three tests. These results do not establish actual two-VM or paid-provider acceptance.

The suspected unsupported `enable_update_checks` setting is not a finding against the installed 2.24.2 source. Its schema defines the field and `cli.py` checks it before update discovery. No unsupported-field failure was observed.

Prove It Works shaped the review by checking the pinned package's actual permission evaluator and running the focused tests rather than accepting fixture reports alone. No production edits, paid calls, secret-file reads, remote operations, or child agents were used. This report is the only authored file.
