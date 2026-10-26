#!/usr/bin/env python3
import argparse
import asyncio
import hashlib
import json
from pathlib import Path
import ssl
import uuid

import websockets


def identifier():
    return str(uuid.uuid4())


async def send(socket, message, request_id=None):
    await socket.send(json.dumps({
        "version": 1,
        "request_id": request_id or identifier(),
        "message": message,
    }))


async def receive(socket):
    while True:
        envelope = json.loads(await asyncio.wait_for(socket.recv(), timeout=5))
        assert envelope["version"] == 1, envelope
        if envelope["message"]["type"] == "heartbeat":
            await send(socket, {"type": "pong"})
            continue
        return envelope


async def matching(socket, predicate):
    deadline = asyncio.get_running_loop().time() + 5
    while asyncio.get_running_loop().time() < deadline:
        envelope = await receive(socket)
        assert envelope["message"]["type"] != "error", envelope
        if predicate(envelope["message"]):
            return envelope
    raise TimeoutError("Expected daemon report was not received.")


def fence(session, generation):
    return {
        "generation": generation,
        "session_id": session["id"],
        "revision": session["revision"],
        "owner": session["owner"],
    }


async def run_probe(socket, settings, checks):
    peer = socket.remote_address[0]
    assert peer == settings.worker_address, peer
    certificate = socket.transport.get_extra_info("ssl_object").getpeercert(binary_form=True)
    expected = ssl.PEM_cert_to_DER_cert(Path(settings.client_cert).read_text())
    assert hashlib.sha256(certificate).digest() == hashlib.sha256(expected).digest()
    hello = (await receive(socket))["message"]
    assert hello["type"] == "hello", hello
    assert "test_input" in hello["capabilities"], hello
    checks.append({"name": "actual_worker_identity_across_vms", "status": "passed", "peer": peer})
    generation = 2
    session = {
        "id": identifier(), "bot_id": identifier(), "worker_id": identifier(),
        "owner": "bot", "revision": 1, "status": "pending",
    }
    await send(socket, {
        "type": "welcome", "worker_id": session["worker_id"],
        "generation": generation, "sessions": [session],
    })
    snapshot = (await receive(socket))["message"]
    assert snapshot == {"type": "snapshot_applied", "generation": generation}, snapshot
    task_id, start_id = identifier(), identifier()
    await send(socket, {
        "type": "command", "id": start_id, "task_id": task_id,
        "fence": fence(session, generation),
        "action": {"type": "start", "work": {"type": "test_input", "steps": 10}},
    })
    queued = await matching(socket, lambda message: message["type"] == "bot_input")
    assert queued["message"]["command_id"] == start_id, queued
    owned = dict(session, owner="owner", revision=2)
    await send(socket, {"type": "control", "generation": generation, "session": owned})
    control = (await receive(socket))["message"]
    assert control == {"type": "control_applied", "fence": fence(owned, generation)}, control
    await send(socket, {
        "type": "receipt", "command_id": start_id, "outcome": {"type": "accepted"},
    }, request_id=queued["request_id"])

    async def suspended(current):
        command_id = identifier()
        await send(socket, {
            "type": "command", "id": command_id, "task_id": task_id,
            "fence": fence(current, generation), "action": {"type": "status"},
        })
        report = (await matching(socket, lambda message:
            message["type"] == "event" and message["command_id"] == command_id
        ))["message"]
        assert report["event"] == {"type": "status", "status": "suspended", "applied": 0}, report

    await suspended(owned)
    checks.append({"name": "delayed_receipt_after_confirmed_takeover", "status": "passed", "applied": 0})
    for stale, code in [
        (fence(session, generation), "stale_revision"),
        (fence(owned, generation - 1), "stale_generation"),
    ]:
        command_id = identifier()
        await send(socket, {"type": "input", "command_id": command_id, "fence": stale, "value": 1})
        report = (await matching(socket, lambda message:
            message["type"] == "acknowledgment" and message["command_id"] == command_id
        ))["message"]
        assert report["outcome"] == {"type": "rejected", "code": code}, report
        checks.append({"name": "daemon_rejects_" + code, "status": "passed"})
    command_id = identifier()
    await send(socket, {
        "type": "input", "command_id": command_id, "fence": fence(owned, generation), "value": 7,
    })
    applied = (await matching(socket, lambda message:
        message["type"] == "acknowledgment" and message["command_id"] == command_id
        and message["outcome"]["type"] != "accepted"
    ))["message"]
    assert applied["outcome"] == {"type": "applied"}, applied
    checks.append({"name": "current_owner_input_applied", "status": "passed"})
    returned = dict(owned, owner="bot", revision=3)
    await send(socket, {"type": "control", "generation": generation, "session": returned})
    control = (await receive(socket))["message"]
    assert control == {"type": "control_applied", "fence": fence(returned, generation)}, control
    await suspended(returned)
    checks.append({"name": "return_to_bot_does_not_replay_old_producer", "status": "passed", "applied": 0})


async def main(settings):
    output = Path(settings.output_dir)
    output.mkdir(mode=0o700, parents=True, exist_ok=True)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.minimum_version = ssl.TLSVersion.TLSv1_2
    context.load_cert_chain(settings.server_cert, settings.server_key)
    context.load_verify_locations(settings.ca)
    context.verify_mode = ssl.CERT_REQUIRED
    finished = asyncio.Event()
    checks = []
    result = {"status": "failed", "checks": checks}

    async def connected(socket, _path):
        try:
            await run_probe(socket, settings, checks)
            result["status"] = "passed"
        except Exception as error:
            result["error"] = type(error).__name__ + ": " + str(error)
        finally:
            finished.set()

    try:
        async with websockets.serve(
            connected, settings.listen, settings.port, ssl=context,
            max_size=65536, compression=None, ping_interval=None,
        ):
            (output / "ready").write_text("ready\n")
            await asyncio.wait_for(finished.wait(), timeout=30)
    except Exception as error:
        result["error"] = type(error).__name__ + ": " + str(error)
    (output / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result), flush=True)
    return 0 if result["status"] == "passed" else 1


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--listen", required=True)
    parser.add_argument("--port", type=int, default=43123)
    parser.add_argument("--worker-address", required=True)
    parser.add_argument("--server-cert", required=True)
    parser.add_argument("--server-key", required=True)
    parser.add_argument("--ca", required=True)
    parser.add_argument("--client-cert", required=True)
    parser.add_argument("--output-dir", required=True)
    raise SystemExit(asyncio.run(main(parser.parse_args())))
