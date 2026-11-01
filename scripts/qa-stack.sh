#!/bin/sh
# Bring up a disposable Botstral QA stack on a cloud VM.
# Commands: up | down | status | chat | send
# Do not print MISTRAL_API_KEY. State dirs are mode 0700.
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_ROOT=${QA_STACK_REPO:-}
if [ -z "$REPO_ROOT" ]; then
    if REPO_ROOT=$(git -C "$SCRIPT_DIR" rev-parse --show-toplevel 2>/dev/null); then
        :
    elif [ -d /workspace/manager ] && [ -d /workspace/desktop ]; then
        REPO_ROOT=/workspace
    else
        REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
    fi
fi
STACK=${QA_STACK_HOME:-/tmp/botstral-qa-stack}
LISTEN=${QA_STACK_LISTEN:-127.0.0.1:54120}
SETUP=${QA_STACK_SETUP:-127.0.0.1:54121}
WORKER=${QA_STACK_WORKER:-127.0.0.1:54122}
OWNER_NAME=${QA_STACK_OWNER:-QA owner}
BOT_NAME=${QA_STACK_BOT:-Botstral}
MODEL=${QA_STACK_MODEL:-mistral-small-2603}
ENDPOINT=${QA_STACK_ENDPOINT:-eu}
INPUT_PRICE=${QA_STACK_INPUT_PRICE:-150000}
OUTPUT_PRICE=${QA_STACK_OUTPUT_PRICE:-600000}
CONTEXT=${QA_STACK_CONTEXT:-}
DEVICE_LABEL=${QA_STACK_DEVICE_LABEL:-QA desktop}
export PATH="${HOME:-/home/ubuntu}/.cargo/bin:/usr/local/cargo/bin:$PATH"

log() { printf '%s\n' "$*" >&2; }
die() { log "qa-stack: $*"; exit 1; }

need_key() {
    if [ -z "${MISTRAL_API_KEY:-}" ]; then
        die "MISTRAL_API_KEY is unset"
    fi
}

mkdir_priv() {
    mkdir -p "$1"
    chmod 700 "$1"
}

write_priv() {
    umask 077
    cat > "$1"
    chmod 600 "$1"
}

load_secrets() {
    if [ ! -f "$STACK/secrets.env" ]; then
        umask 077
        mkdir_priv "$STACK"
        {
            printf 'QA_STACK_PASSWORD=%s\n' "QA-stack-$(python3 -c 'import secrets; print(secrets.token_urlsafe(18))')"
            printf 'QA_STACK_VAULT=%s\n' "QA-vault-$(python3 -c 'import secrets; print(secrets.token_urlsafe(18))')"
        } > "$STACK/secrets.env"
        chmod 600 "$STACK/secrets.env"
    fi
    # shellcheck disable=SC1091
    . "$STACK/secrets.env"
    export QA_STACK_PASSWORD QA_STACK_VAULT STACK
}

manager_bin() {
    if [ -x "$STACK/bin/botstral-manager" ]; then
        printf '%s\n' "$STACK/bin/botstral-manager"
    elif [ -x "$REPO_ROOT/target/debug/botstral-manager" ]; then
        printf '%s\n' "$REPO_ROOT/target/debug/botstral-manager"
    else
        die "manager binary missing. Run up so the stack can build it."
    fi
}

agent_bin() {
    if [ -x "$STACK/bin/botstral-agent" ]; then
        printf '%s\n' "$STACK/bin/botstral-agent"
    elif [ -x "$REPO_ROOT/target/debug/botstral-agent" ]; then
        printf '%s\n' "$REPO_ROOT/target/debug/botstral-agent"
    else
        die "agent binary missing"
    fi
}

desktop_bin() {
    if [ -x "$STACK/bin/botstral-desktop" ]; then
        printf '%s\n' "$STACK/bin/botstral-desktop"
        return
    fi
    for path in \
        "$REPO_ROOT/desktop/src-tauri/target/debug/botstral-desktop" \
        "$REPO_ROOT/desktop/src-tauri/target/debug/bundle/appimage/botstral-desktop" \
        "$REPO_ROOT/target/debug/botstral-desktop"
    do
        if [ -x "$path" ]; then
            printf '%s\n' "$path"
            return
        fi
    done
}

api() {
    # $1 method $2 path $3 optional body file $4 optional token
    method=$1
    path=$2
    body=${3:-}
    token=${4:-}
    url="http://$LISTEN$path"
    if [ "$path" = "/v1/owner" ] && [ "$method" = "POST" ]; then
        url="http://$SETUP$path"
        host=$SETUP
    else
        host=$LISTEN
    fi
    set -- curl -sS --http1.1 --max-time 180 -D "$STACK/http.headers" -o "$STACK/http.body" \
        -H "Host: $host"
    if [ -n "$token" ]; then
        set -- "$@" -H "Authorization: Bearer $token"
    fi
    if [ "$method" != "GET" ]; then
        set -- "$@" -H "Content-Type: application/json"
        if [ -n "$body" ]; then
            set -- "$@" --data-binary @"$body"
        else
            set -- "$@" --data-binary '{}'
        fi
    fi
    set -- "$@" -X "$method" "$url"
    "$@" || return 1
    python3 - "$STACK/http.headers" <<'PY'
import sys
path=sys.argv[1]
status=0
with open(path, errors="replace") as fh:
    line=fh.readline()
    parts=line.split()
    if len(parts)>=2 and parts[1].isdigit():
        status=int(parts[1])
print(status)
PY
}

api_json() {
    method=$1
    path=$2
    token=${3:-}
    python3 -c 'import json,sys; json.dump(json.loads(sys.stdin.read()), open(sys.argv[1],"w"))' "$STACK/http.json"
    status=$(api "$method" "$path" "$STACK/http.json" "$token") || return 1
    printf '%s\n' "$status"
}

save_token() {
    python3 - "$STACK/http.body" "$STACK/session.token" <<'PY'
import json,sys
body=json.load(open(sys.argv[1]))
token=body.get("token")
if not token:
    raise SystemExit("sign-in returned no token")
open(sys.argv[2],"w").write(token)
PY
    chmod 600 "$STACK/session.token"
}

token() {
    cat "$STACK/session.token"
}

wait_health() {
    i=0
    while [ "$i" -lt 50 ]; do
        if curl -sS --http1.1 --max-time 2 -H "Host: $LISTEN" "http://$LISTEN/health" >"$STACK/health.json" 2>/dev/null; then
            if python3 -c 'import json,sys; sys.exit(0 if json.load(open(sys.argv[1])).get("status")=="ok" else 1)' "$STACK/health.json"; then
                return 0
            fi
        fi
        i=$((i + 1))
        sleep 0.2
    done
    die "manager health failed on $LISTEN"
}

pipe_admin() {
    # stdin lines are prompt answers. Avoid a PTY so TCSAFLUSH cannot drop them.
    "$@"
}

build_bins() {
    mkdir_priv "$STACK/bin"
    if [ ! -x "$REPO_ROOT/target/debug/botstral-manager" ] || [ ! -x "$REPO_ROOT/target/debug/botstral-agent" ]; then
        log "building manager and agent"
        (cd "$REPO_ROOT" && cargo build --locked -p botstral-manager -p botstral-agent)
    fi
    cp -f "$REPO_ROOT/target/debug/botstral-manager" "$STACK/bin/botstral-manager"
    cp -f "$REPO_ROOT/target/debug/botstral-agent" "$STACK/bin/botstral-agent"
    chmod 700 "$STACK/bin/botstral-manager" "$STACK/bin/botstral-agent"
}

build_client() {
    src=$SCRIPT_DIR/qa-stack-harness
    if [ ! -d "$src" ]; then
        src=$SCRIPT_DIR/harness
    fi
    [ -d "$src" ] || die "qa-stack-harness sources missing next to the script"
    mkdir_priv "$STACK/client-src"
    mkdir_priv "$STACK/client-src/src"
    cp -f "$src/src/main.rs" "$STACK/client-src/src/main.rs"
    write_priv "$STACK/client-src/Cargo.toml" <<EOF
[package]
name = "qa-stack-client"
version = "0.1.0"
edition = "2024"
publish = false

[dependencies]
botstral-desktop = { path = "$REPO_ROOT/desktop/src-tauri", default-features = false }
botstral-link = { path = "$REPO_ROOT/link", features = ["tls"] }
botstral-owner-client = { path = "$REPO_ROOT/link/client" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["rt-multi-thread", "macros", "time", "io-util", "io-std"] }
uuid = { version = "1", features = ["v4"] }
zeroize = "1"
EOF
    if [ -f "$REPO_ROOT/rust-toolchain.toml" ]; then
        cp -f "$REPO_ROOT/rust-toolchain.toml" "$STACK/client-src/rust-toolchain.toml"
    fi
    log "building paired desktop client harness"
    (cd "$STACK/client-src" && CARGO_TARGET_DIR="$STACK/client-target" cargo build)
    cp -f "$STACK/client-target/debug/qa-stack-client" "$STACK/bin/qa-stack-client"
    chmod 700 "$STACK/bin/qa-stack-client"
}

lookup_context() {
    if [ -n "$CONTEXT" ]; then
        printf '%s\n' "$CONTEXT"
        return
    fi
    host=api.mistral.ai
    if [ "$ENDPOINT" = "eu" ]; then
        host=api.eu.mistral.ai
    elif [ "$ENDPOINT" = "us" ]; then
        host=api.us.mistral.ai
    fi
    curl -sS --max-time 20 -H "Authorization: Bearer ${MISTRAL_API_KEY}" \
        "https://$host/v1/models/$MODEL" >"$STACK/model.json" || true
    python3 - "$STACK/model.json" <<'PY' || printf '262144\n'
import json,sys
try:
    d=json.load(open(sys.argv[1]))
    print(int(d["max_context_length"]))
except Exception:
    raise SystemExit(1)
PY
}

cmd_down() {
    if [ -d "$STACK" ]; then
        if [ -f "$STACK/harness.pid" ]; then
            pid=$(cat "$STACK/harness.pid")
            kill "$pid" 2>/dev/null || true
        fi
        if [ -f "$STACK/manager.pid" ]; then
            pid=$(cat "$STACK/manager.pid")
            kill "$pid" 2>/dev/null || true
        fi
        if [ -f "$STACK/worker.pid" ]; then
            pid=$(cat "$STACK/worker.pid")
            kill "$pid" 2>/dev/null || true
        fi
        if [ -f "$STACK/desktop.pid" ]; then
            pid=$(cat "$STACK/desktop.pid")
            kill "$pid" 2>/dev/null || true
        fi
        sleep 0.4
        # Only kill processes that still advertise this stack dir.
        pkill -f "$STACK/state" 2>/dev/null || true
        pkill -f "$STACK/worker" 2>/dev/null || true
        pkill -f "$STACK/owner" 2>/dev/null || true
        pkill -f "$STACK/desktop-profile" 2>/dev/null || true
    fi
    if [ -d "$STACK" ]; then
        rm -rf "$STACK/state" "$STACK/worker" "$STACK/owner" "$STACK/desktop-profile" \
            "$STACK/session.token" "$STACK/http.body" "$STACK/http.headers" \
            "$STACK/harness.cmd" "$STACK/harness.reply" "$STACK/pids" \
            "$STACK/manager.log" "$STACK/worker.log" "$STACK/harness.log" \
            "$STACK/desktop.log" "$STACK/endpoints.txt" "$STACK/ready.json"
    fi
    log "qa-stack down"
}

start_manager() {
    mkdir_priv "$STACK/state"
    : >"$STACK/manager.log"
    chmod 600 "$STACK/manager.log"
    nohup "$(manager_bin)" \
        --state-dir "$STACK/state" \
        --listen "$LISTEN" \
        --setup-listen "$SETUP" \
        --worker-listen "$WORKER" \
        >"$STACK/manager.log" 2>&1 &
    printf '%s\n' "$!" >"$STACK/manager.pid"
    chmod 600 "$STACK/manager.pid"
    wait_health
    mode=$(python3 -c 'import os,sys; print(oct(os.stat(sys.argv[1]).st_mode & 0o777))' "$STACK/state")
    [ "$mode" = "0o700" ] || die "state dir mode is $mode, want 0700"
}

admin_cli() {
    load_secrets
    mgr=$(manager_bin)
    log "setup"
    printf '%s\n' yes "$OWNER_NAME" "$QA_STACK_PASSWORD" "$QA_STACK_PASSWORD" "$QA_STACK_VAULT" "$QA_STACK_VAULT" \
        | pipe_admin "$mgr" setup --state-dir "$STACK/state" >"$STACK/setup.out" 2>"$STACK/setup.err"
    log "unlock"
    printf '%s\n' "$QA_STACK_PASSWORD" "$QA_STACK_VAULT" \
        | pipe_admin "$mgr" unlock --state-dir "$STACK/state" >"$STACK/unlock.out" 2>"$STACK/unlock.err"
    log "provider-key"
    printf '%s\n' "$QA_STACK_PASSWORD" "$MISTRAL_API_KEY" \
        | pipe_admin "$mgr" provider-key --state-dir "$STACK/state" >"$STACK/provider-key.out" 2>"$STACK/provider-key.err"
    if grep -F -q "${MISTRAL_API_KEY}" "$STACK/provider-key.out" "$STACK/provider-key.err" 2>/dev/null; then
        die "provider-key leaked the API key into logs"
    fi
    context=$(lookup_context)
    printf '%s\n' "$context" >"$STACK/context.txt"
    log "configure $BOT_NAME context=$context"
    printf '%s\n' "$QA_STACK_PASSWORD" \
        | pipe_admin "$mgr" configure "$BOT_NAME" \
            --model "$MODEL" \
            --endpoint "$ENDPOINT" \
            --input-price "$INPUT_PRICE" \
            --output-price "$OUTPUT_PRICE" \
            --context "$context" \
            --state-dir "$STACK/state" \
            >"$STACK/configure.out" 2>"$STACK/configure.err"
}

signin_http() {
    load_secrets
    export QA_STACK_PASSWORD STACK
    python3 -c 'import json,os; json.dump({"password":os.environ["QA_STACK_PASSWORD"]}, open(os.environ["STACK"]+"/login.json","w"))'
    chmod 600 "$STACK/login.json"
    status=$(api POST /v1/sessions "$STACK/login.json") || die "sign-in request failed"
    if [ "$status" = "429" ]; then
        log "sign-in rate limited. Sleeping 61s."
        sleep 61
        status=$(api POST /v1/sessions "$STACK/login.json") || die "sign-in retry failed"
    fi
    [ "$status" = "201" ] || die "sign-in HTTP $status"
    save_token
}

enroll_worker() {
    python3 -c 'import json; json.dump({"lifetime_seconds":600}, open("'"$STACK"'/invite-worker.json","w"))'
    status=$(api POST /v1/worker-invitations "$STACK/invite-worker.json" "$(token)")
    [ "$status" = "201" ] || die "worker invite HTTP $status"
    code=$(python3 -c 'import json; print(json.load(open("'"$STACK"'/http.body"))["code"])')
    printf '%s\n' "$code" | write_priv "$STACK/worker.code"
    mkdir_priv "$STACK/worker"
    log "joining worker"
    "$(agent_bin)" join --state-dir "$STACK/worker" "$code" >"$STACK/worker-join.out" 2>"$STACK/worker-join.err"
    grep -q "worker enrolled" "$STACK/worker-join.out" || die "worker join did not print worker enrolled"
    : >"$STACK/worker.log"
    chmod 600 "$STACK/worker.log"
    nohup "$(agent_bin)" run --state-dir "$STACK/worker" >"$STACK/worker.log" 2>&1 &
    printf '%s\n' "$!" >"$STACK/worker.pid"
    i=0
    while [ "$i" -lt 60 ]; do
        status=$(api GET /v1/workers "" "$(token)") || true
        if [ "$status" = "200" ]; then
            if python3 - "$STACK/http.body" <<'PY'; then
import json,sys
rows=json.load(open(sys.argv[1]))
sys.exit(0 if any(w.get("phase")=="ready" and not w.get("revoked") for w in rows) else 1)
PY
                cp -f "$STACK/http.body" "$STACK/workers.json"
                return 0
            fi
        fi
        i=$((i + 1))
        sleep 0.5
    done
    die "worker never reached phase ready"
}

client_call() {
    python3 - "$STACK" "$@" <<'PY'
import json, os, sys, time
stack = sys.argv[1]
payload = json.loads(sys.argv[2])
cmd = os.path.join(stack, "harness.cmd")
out = os.path.join(stack, "harness.out")
before = 0
if os.path.exists(out):
    before = sum(1 for _ in open(out, encoding="utf-8", errors="replace"))
fd = os.open(cmd, os.O_WRONLY)
os.write(fd, (json.dumps(payload) + "\n").encode())
os.close(fd)
deadline = time.time() + 180
line = ""
while time.time() < deadline:
    if os.path.exists(out):
        lines = open(out, encoding="utf-8", errors="replace").read().splitlines()
        if len(lines) > before:
            line = lines[-1]
            break
    time.sleep(0.05)
if not line:
    raise SystemExit("paired client produced no reply")
print(line)
PY
}

start_client() {
    rm -f "$STACK/harness.cmd" "$STACK/harness.out"
    mkfifo "$STACK/harness.cmd"
    chmod 600 "$STACK/harness.cmd"
    mkdir_priv "$STACK/owner"
    : >"$STACK/harness.log"
    : >"$STACK/harness.out"
    chmod 600 "$STACK/harness.log" "$STACK/harness.out"
    # Keep a writer open so the client stdin fifo does not see EOF.
    # Open the fifo read/write first so the client stdin open does not block.
    exec 8<>"$STACK/harness.cmd"
    nohup "$STACK/bin/qa-stack-client" "$STACK/owner" \
        <"$STACK/harness.cmd" >>"$STACK/harness.out" 2>"$STACK/harness.log" &
    printf '%s\n' "$!" >"$STACK/harness.pid"
    sleep 0.2
    began=$(client_call '{"op":"begin","label":"'"$DEVICE_LABEL"'"}')
    printf '%s\n' "$began" >"$STACK/begin.json"
    python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); sys.exit(0 if d.get("ok") else 1)' "$STACK/begin.json" \
        || die "desktop begin pairing failed"
    python3 -c 'import json; json.dump({"label":"'"$DEVICE_LABEL"'","lifetime_seconds":600}, open("'"$STACK"'/invite-device.json","w"))'
    status=$(api POST /v1/device-invitations "$STACK/invite-device.json" "$(token)")
    [ "$status" = "201" ] || die "device invite HTTP $status"
    code=$(python3 -c 'import json; print(json.load(open("'"$STACK"'/http.body"))["code"])')
    printf '%s\n' "$code" | write_priv "$STACK/device.code"
    joined=$(python3 -c 'import json,os; print(json.dumps({"op":"join","code":open(os.environ["STACK"]+"/device.code").read().strip()}))')
    printf '%s\n' "$(client_call "$joined")" >"$STACK/join.json"
    python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); sys.exit(0 if d.get("ok") else 1)' "$STACK/join.json" \
        || die "device join failed"
    printf '%s\n' "$(client_call '{"op":"confirm"}')" >"$STACK/confirm.json"
    python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); sys.exit(0 if d.get("status")=="online" else 1)' "$STACK/confirm.json" \
        || die "device confirm was not online"
    printf '%s\n' "$(client_call '{"op":"refresh"}')" >"$STACK/workspace.json"
}

try_tauri() {
    bin=$(desktop_bin || true)
    if [ -z "${bin:-}" ]; then
        printf '%s\n' '{"path":"gui","ok":false,"reason":"desktop binary not built"}' >"$STACK/gui.json"
        return 0
    fi
    mkdir_priv "$STACK/desktop-profile"
    export BOTSTRAL_DESKTOP_PROFILE="$STACK/desktop-profile"
    export DISPLAY=${DISPLAY:-:1}
    if [ -z "${DISPLAY:-}" ] || [ ! -S "/tmp/.X11-unix/X${DISPLAY#:}" ]; then
        if command -v Xvfb >/dev/null 2>&1; then
            Xvfb :93 -screen 0 1400x900x24 >"$STACK/xvfb.log" 2>&1 &
            printf '%s\n' "$!" >"$STACK/xvfb.pid"
            export DISPLAY=:93
            sleep 0.4
        fi
    fi
    if command -v gnome-keyring-daemon >/dev/null 2>&1; then
        gnome-keyring-daemon --start --components=secrets >/dev/null 2>&1 || true
    fi
    nohup "$bin" >"$STACK/desktop.log" 2>&1 &
    printf '%s\n' "$!" >"$STACK/desktop.pid"
    sleep 2
    if command -v xdotool >/dev/null 2>&1; then
        if xdotool search --name Botstral >/dev/null 2>&1; then
            printf '%s\n' '{"path":"gui","ok":true,"window":"Botstral","note":"window opened. Pairing still uses the desktop Rust client fifo for a deterministic first chat."}' >"$STACK/gui.json"
            return 0
        fi
    fi
    printf '%s\n' '{"path":"gui","ok":false,"reason":"window not found or xdotool missing","binary":"'"$bin"'"}' >"$STACK/gui.json"
}

write_endpoints() {
    python3 - "$STACK" "$LISTEN" "$SETUP" "$WORKER" "$REPO_ROOT" <<'PY'
import json, os, sys
stack, listen, setup, worker, repo = sys.argv[1:]
ws = {}
try:
    ws = json.load(open(os.path.join(stack, "workspace.json")))
except Exception:
    pass
workspace = ws.get("workspace") or {}
bots = {b.get("name"): b.get("id") for b in workspace.get("bots", [])}
configs = workspace.get("configurations") or []
workers = []
try:
    workers = json.load(open(os.path.join(stack, "workers.json")))
except Exception:
    pass
gui = {}
try:
    gui = json.load(open(os.path.join(stack, "gui.json")))
except Exception:
    pass
context = open(os.path.join(stack, "context.txt")).read().strip() if os.path.exists(os.path.join(stack, "context.txt")) else ""
ready = {
    "listen": listen,
    "setup": setup,
    "worker_listen": worker,
    "state_dir": os.path.join(stack, "state"),
    "worker_dir": os.path.join(stack, "worker"),
    "owner_dir": os.path.join(stack, "owner"),
    "desktop_profile": os.path.join(stack, "desktop-profile"),
    "stack": stack,
    "repo": repo,
    "bots": bots,
    "configurations": configs,
    "workers": workers,
    "context_tokens": int(context) if context.isdigit() else context,
    "client_path": "desktop-rust-RemoteService",
    "gui": gui,
    "state_mode": oct(os.stat(os.path.join(stack, "state")).st_mode & 0o777),
}
json.dump(ready, open(os.path.join(stack, "ready.json"), "w"), indent=2)
text = [
    f"listen={listen}",
    f"setup={setup}",
    f"worker_listen={worker}",
    f"state_dir={ready['state_dir']}",
    f"worker_dir={ready['worker_dir']}",
    f"owner_dir={ready['owner_dir']}",
    f"stack={stack}",
    f"bots={bots}",
    f"workers={[(w.get('id'), w.get('phase')) for w in workers]}",
    f"client=desktop RemoteService fifo",
    f"gui={gui}",
]
open(os.path.join(stack, "endpoints.txt"), "w").write("\n".join(text) + "\n")
print("\n".join(text))
PY
}

cmd_up() {
    need_key
    export STACK
    if [ -f "$STACK/ready.json" ] && [ -f "$STACK/manager.pid" ]; then
        if curl -sS --http1.1 --max-time 2 -H "Host: $LISTEN" "http://$LISTEN/health" >/dev/null 2>&1; then
            log "stack already up"
            cat "$STACK/endpoints.txt"
            return 0
        fi
    fi
    cmd_down
    mkdir_priv "$STACK"
    load_secrets
    build_bins
    build_client
    start_manager
    admin_cli
    signin_http
    enroll_worker
    start_client
    try_tauri || true
    write_endpoints
    log "qa-stack up"
}

cmd_status() {
    export STACK
    if [ ! -f "$STACK/ready.json" ]; then
        die "stack is down"
    fi
    wait_health
    status=$(api GET /v1/workers "" "$(token)") || true
    printf 'health=ok listen=%s worker_listen=%s workers_http=%s\n' "$LISTEN" "$WORKER" "$status"
    if [ "$status" = "200" ]; then
        python3 -c 'import json; print(json.dumps(json.load(open("'"$STACK"'/http.body")), indent=2))'
    fi
    if [ -f "$STACK/harness.pid" ]; then
        printf '%s\n' "$(client_call '{"op":"inspect"}')"
    fi
    cat "$STACK/endpoints.txt"
}

chat_or_send() {
    export STACK
    need_running
    text=${1:-Reply with the single word pong.}
    tokens=${2:-}
    output=${3:-1024}
    cost=${4:-200000}
    turns=${5:-1}
    inspect=$(client_call '{"op":"refresh"}')
    printf '%s\n' "$inspect" >"$STACK/last-refresh.json"
    QA_STACK_CHAT_TEXT=$text QA_STACK_TOKENS=${tokens:-default} QA_STACK_OUTPUT=$output QA_STACK_COST=$cost QA_STACK_TURNS=$turns \
    python3 - <<'PY'
import json, os
stack = os.environ["STACK"]
view = json.load(open(os.path.join(stack, "last-refresh.json")))
workspace = view.get("workspace") or {}
bots = workspace.get("bots") or []
if not bots:
    raise SystemExit("no bots in paired workspace")
bot = bots[0]["id"]
configs = {c["bot"]: c for c in workspace.get("configurations", [])}
context = configs.get(bot, {}).get("context_tokens")
output = int(os.environ["QA_STACK_OUTPUT"])
tokens = os.environ["QA_STACK_TOKENS"]
if tokens in ("", "default", "derived"):
    if not context:
        raise SystemExit("paired workspace has no context_tokens")
    tokens = int(context) + output
else:
    tokens = int(tokens)
req = {
    "op": "send",
    "bot": bot,
    "tokens": tokens,
    "output": output,
    "cost": int(os.environ["QA_STACK_COST"]),
    "turns": int(os.environ["QA_STACK_TURNS"]),
    "text": os.environ["QA_STACK_CHAT_TEXT"],
    "derived_from_context": context,
}
json.dump(req, open(os.path.join(stack, "last-send-req.json"), "w"), indent=2)
print(json.dumps(req))
PY
    sent=$(client_call "$(cat "$STACK/last-send-req.json")")
    printf '%s\n' "$sent" >"$STACK/last-send.json"
    python3 - "$STACK/last-send.json" <<'PY'
import json,sys
d=json.load(open(sys.argv[1]))
print(json.dumps({k:d.get(k) for k in ("ok","error","tokens_sent","output_sent","cost_sent","issue")}, indent=2))
if d.get("ok"):
    sys.exit(0)
sys.exit(2)
PY
    send_status=$?
    if [ "$send_status" -eq 0 ]; then
        waited=$(client_call '{"op":"wait"}')
        printf '%s\n' "$waited" >"$STACK/last-wait.json"
        run_id=$(python3 -c 'import json; d=json.load(open("'"$STACK"'/last-wait.json")); runs=(d.get("workspace") or {}).get("runs") or []; print(runs[-1]["id"] if runs else "")')
        if [ -n "$run_id" ]; then
            status=$(api GET "/v1/runs/$run_id" "" "$(token)") || true
            cp -f "$STACK/http.body" "$STACK/last-run.json"
            python3 - "$STACK/last-run.json" "$status" <<'PY'
import json,sys
status=sys.argv[2]
try:
    d=json.load(open(sys.argv[1]))
except Exception:
    print("run_http", status)
    raise SystemExit(0)
turns=d.get("turns") or []
out=[]
for t in turns:
    out.append({
        "ordinal": t.get("ordinal"),
        "finish_reason": t.get("finish_reason"),
        "provider_request_id": t.get("provider_request_id"),
        "text": (t.get("response") or {}).get("content") if isinstance(t.get("response"), dict) else None,
    })
print(json.dumps({
    "http": status,
    "phase": (d.get("summary") or {}).get("phase"),
    "limits": (d.get("request") or {}).get("limits"),
    "turns": out,
}, indent=2))
PY
        fi
        return 0
    fi
    return "$send_status"
}

need_running() {
    [ -f "$STACK/ready.json" ] || die "stack is down. Run qa-stack.sh up"
    [ -f "$STACK/harness.pid" ] || die "paired client is not running"
    wait_health
}

usage() {
    cat <<'EOF'
Usage: qa-stack.sh up|down|status|chat [text]|send [--tokens N] [--text MSG]

up       Build binaries if needed, start manager with --listen/--setup-listen/--worker-listen,
         run setup/unlock/provider-key/configure, enroll a worker, pair a desktop client.
down     Stop processes and delete stack state.
status   Print health, workers, paired workspace, and endpoints.
chat     Send a first chat using the derived default budget (context + 1024).
send     Send with an explicit --tokens value. Used to prove the old 200k default is refused.

Environment:
  MISTRAL_API_KEY   required for up/chat/send
  QA_STACK_HOME     default /tmp/botstral-qa-stack
  QA_STACK_REPO     repository root if git cannot see it
EOF
}

cmd=$1
shift || true
export STACK
case "${cmd:-}" in
    up) cmd_up ;;
    down) cmd_down ;;
    status) cmd_status ;;
    chat)
        text=${1:-Reply with the single word pong.}
        chat_or_send "$text" default
        ;;
    send)
        tokens=
        text="Reply with the single word pong."
        while [ $# -gt 0 ]; do
            case $1 in
                --tokens) tokens=$2; shift 2 ;;
                --text) text=$2; shift 2 ;;
                --output) output=$2; shift 2 ;;
                *) die "unknown send option $1" ;;
            esac
        done
        chat_or_send "$text" "${tokens:-default}" "${output:-1024}"
        ;;
    -h|--help|help) usage ;;
    *) usage; exit 2 ;;
esac
