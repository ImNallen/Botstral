# Run Computer work

Computer work requires Linux x86_64, systemd 255, the configured non-root guest account, and a sealed runtime. Shell work continues to use `WorkSpec::Process`. Computer work accepts typed navigation, click, fill, text capture, screenshot, file write, file read, and file export operations. It does not accept page evaluation or executable arguments.

## Install the runtime

Run this command from the checkout on the Computer VM with a fresh absolute destination:

```sh
agent/runtime/setup.sh /home/botstral/botstral-runtime-v1
```

The command installs pinned Node 24.22.1, locked Playwright 1.64.1, matching Chromium, and browser system dependencies. Setup requires the guest's existing administrative rights. Keep the resulting runtime directory unchanged. The last line prints its runtime identity. To recompute that identity without changing files, run:

```sh
sha256sum /home/botstral/botstral-runtime-v1/runtime.json
```

The runtime is checked against [Playwright's current requirements](https://playwright.dev/docs/intro#system-requirements), its [matching-browser installation contract](https://playwright.dev/docs/browsers), and the [Node LTS policy](https://nodejs.org/en/about/previous-releases). The package lock and Node archive digest are tracked under `agent/runtime/`.

## Enable the capability

Keep the existing private `process.json` configuration. Add a private `computer.json` beside it in the worker state directory. Replace the runtime digest with the setup result. Use the guest's actual user and group identifiers.

```json
{
  "account": { "uid": 1000, "gid": 1000 },
  "runtime": { "sha256": "REPLACE_WITH_64_CHARACTER_RUNTIME_DIGEST" },
  "directory": "/home/botstral/botstral-runtime-v1",
  "ceilings": {
    "runtime_seconds": 60,
    "output_bytes": 1024,
    "processes": 128,
    "memory_bytes": 1073741824
  }
}
```

Restart the worker. It advertises `computer_v1` only after storage, supervision, runtime integrity, and actual browser probes pass. It refuses a corrupt shared profile without resetting it. Completed browser state is stored in the dedicated `automation-profile` under worker state. All bots share this profile and execute serially.

## Submit work and retrieve outputs

Submit a task to `POST /v1/tasks` with `work.type` set to `computer`. Its `spec` contains `profile: linux_playwright_v1`, the configured account and runtime identity, the same deadline as task settings, limits, a viewport, typed operations, and output declarations. [The generated protocol](../link/typescript/protocol.d.ts) defines the exact shapes. [The actual two-VM fixture](../agent/tests/w01_computer_two_vm.py) contains runnable owner API examples.

Approve the exact task through the existing approval route. Occupied execution or artifact capacity leaves approval unconsumed. Each declaration has a unique slot, a safe attachment name, a media type, and a maximum byte count. Capture and export operations refer to that slot. Only declared outputs become artifacts. File operations accept approved absolute paths outside coding workspaces.

Poll the task's `outputs` separately from its execution `state`. Completion does not imply available artifacts. An available output includes an opaque artifact identifier, length, and SHA-256 digest. Download it with the existing owner bearer session:

```text
GET /v1/tasks/{task}/artifacts/{artifact}
```

The attachment route survives manager restart. Sign in again after restart. Missing or corrupt outputs remain explicitly unavailable and do not repeat guest effects.

## Limits and preserved state

A Computer specification is at most 2048 serialized bytes. It has at most twelve operations and four outputs. Each artifact is at most 8 MiB. One execution declares at most 16 MiB. The worker spool reserves at most 64 MiB. The manager reserves at most 256 MiB and retains at most 512 artifact records. Task retention remains bounded to 128 tasks. No automatic retention deletion is implemented.

The tested Computer ceiling is 1 GiB and 128 tasks. The Process ceiling remains 256 MiB and 32 tasks. Work is serial globally and per bot. These limits do not promise support for every website or a throughput level.

Clean context closure preserves cookies, local storage, and tested IndexedDB across tasks and ordinary idle restarts. Open tabs, session storage, and unflushed crash-time changes are unsupported. Active crash recovery remains uncertain and does not replay browser effects. Guest setup privileges and ordinary same-account filesystem access remain unchanged. The worker does not claim hostile isolation from other processes using that account.
