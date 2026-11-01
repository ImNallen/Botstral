# QA stack

Dev and Tester bring up one disposable Botstral install from the repo root. The install is a manager with a worker listener, an enrolled worker, a paired desktop client, and a real Mistral key.

## Bring up the stack

Set `MISTRAL_API_KEY`. From the repo root, run:

```sh
scripts/qa-stack.sh down
scripts/qa-stack.sh up
scripts/qa-stack.sh status
scripts/qa-stack.sh chat 'Reply with the single word pong.'
scripts/qa-stack.sh send --tokens 200000 --text 'Reply with the single word pong.'
scripts/qa-stack.sh down
```

`up` prints the existing endpoints when the stack is already healthy. `down` then `up` is a from-scratch bring-up.

The default home is `/tmp/botstral-qa-stack`. Override it with `QA_STACK_HOME`. Keep `$QA_STACK_HOME` outside the clone.

## What you need

Rust 1.93 and `cargo` on `PATH`, plus `curl`, `python3`, and `git`.

On Linux, install `pkg-config` and `libdbus-1-dev` before `up`. The client build compiles `botstral-desktop` with `default-features = false`. That library still depends on `keyring`, and the Linux build of `keyring` looks up `dbus-1` through `pkg-config`.

The Mistral key stays in the environment. The script writes the owner password and the vault passphrase into `$QA_STACK_HOME/secrets.env` at mode 0600.

A Tauri window needs `libwebkit2gtk-4.1-dev`, `Xvfb`, `xdotool`, and a Secret Service such as `gnome-keyring-daemon`. Install those packages with `DEBIAN_FRONTEND=noninteractive` and `Dpkg::Options::=--force-confold` so `fuse3` does not stop on a conffile prompt. Pairing completes through the Rust client when the window is missing.

## What up does

The script builds `botstral-manager` and `botstral-agent` when the debug binaries are missing. It always compiles the paired client from `scripts/qa-stack-harness/src/main.rs` into the home directory.

The manager starts with `--listen`, `--setup-listen`, and `--worker-listen`. Invite codes need the worker listener.

`setup`, `unlock`, `provider-key`, and `configure` take their answers on stdin. `read_password` never sees a terminal.

The script enrolls a worker and waits until `GET /v1/workers` reports `phase=ready`. It then pairs a device through the desktop `RemoteService` client.

## Chat and send

`chat` reads `context_tokens` for the paired bot and sends that value plus 1024. Output is 1024 and cost is 200000. That is the pull request 93 default. The command prints admission, then `finish_reason` and `provider_request_id` from `GET /v1/runs/{id}`.

`send --tokens 200000` is the older default. Expect admission to return `budget` for Mistral Small 4, whose context is 262144 tokens.

## Gotchas

State directories are mode 0700. The script creates them that way.

Sign-in allows 5 attempts per 60 seconds. `up` uses three CLI sign-ins plus one kept HTTP session. On HTTP 429 the script sleeps 61 seconds and retries.

`read_password` flushes a real terminal and can drop bytes that arrived early. Keep answers on stdin. `pexpect.spawn` on an argument list drops the binary path. Leave that driver unused.

`invite worker` and `invite device` need `--worker-listen`. The join code embeds that address.

Leave `MISTRAL_API_KEY` out of logs. The script stops when the key appears in the provider-key log.

The paired client stays running on a fifo. `down` stops it. Closing the fifo makes the client exit.

`up` prints `listen`, `setup`, `worker_listen`, `state_dir`, `worker_dir`, `owner_dir`, bot ids, the worker phase, and whether a Tauri window opened. The same record is in `$QA_STACK_HOME/ready.json` and `endpoints.txt`.
