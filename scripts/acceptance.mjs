import assert from "node:assert/strict";
import { copyFile, chmod, mkdir, mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve, join } from "node:path";
import { spawnSync } from "node:child_process";
import { startManager } from "./manager-process.mjs";

const root = await mkdtemp(join(tmpdir(), "botstral-acceptance-"));
let manager;
try {
  const release = join(root, "release");
  await mkdir(release);
  const binary = join(release, "botstral");
  await copyFile(resolve("bin/botstral"), binary);
  await chmod(binary, 0o700);
  const dataDir = join(root, "persistent ?# data");
  const ordinary = spawnSync(binary, ["--data-dir", dataDir, "--port", "0"], {
    cwd: release,
    encoding: "utf8",
    timeout: 10000,
  });
  assert.equal(ordinary.status, 1, "Uninitialized ordinary startup must fail.");
  assert.match(
    ordinary.stderr,
    /--setup/,
    "Startup must give the setup command.",
  );
  assert.doesNotMatch(
    ordinary.stderr,
    /Manager:/,
    "Uninitialized startup must not listen.",
  );

  manager = await startManager({ binary, dataDir, setup: true, cwd: release });
  assert.ok(
    manager.setupURL,
    "Explicit setup must open its separate listener.",
  );
  const setup = new URL(manager.setupURL);
  const setupOrigin = setup.origin;
  const capability = setup.hash.slice(1);
  assert.notEqual(
    setupOrigin,
    manager.mainURL,
    "Setup and main need separate sockets.",
  );
  const credentials = { username: "owner", password: "synthetic passphrase" };
  const setupBody = { ...credentials, confirmPassword: credentials.password };
  for (const origin of ["", "null", "http://foreign.invalid"]) {
    const response = await fetch(`${setupOrigin}/owner`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Origin: origin,
        "X-Setup-Token": capability,
      },
      body: JSON.stringify(setupBody),
    });
    assert.equal(response.status, 403, "Setup must reject hostile origins.");
  }
  const initialized = await fetch(`${setupOrigin}/owner`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Origin: setupOrigin,
      "X-Setup-Token": capability,
    },
    body: JSON.stringify(setupBody),
  });
  assert.equal(initialized.status, 201, "Owner setup must succeed.");
  assert.equal(
    initialized.headers.get("set-cookie"),
    null,
    "Setup must never create cookies.",
  );
  await initialized.arrayBuffer();
  for (let attempt = 0; attempt < 40; attempt++) {
    try {
      await fetch(setupOrigin, { signal: AbortSignal.timeout(500) });
    } catch {
      break;
    }
    assert.ok(attempt < 39, "Setup listener must close after success.");
    await new Promise((resolve) => setTimeout(resolve, 50));
  }

  async function request(
    path,
    { method = "GET", body, session, origin = manager.mainURL } = {},
  ) {
    const headers = new Headers();
    if (session) headers.set("Cookie", session.cookie);
    if (method !== "GET") {
      headers.set("Content-Type", "application/json");
      headers.set("Origin", origin);
      headers.set("X-Botstral-Request", "1");
      if (session) headers.set("X-CSRF-Token", session.csrf);
    }
    return fetch(`${manager.mainURL}${path}`, {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
      signal: AbortSignal.timeout(5000),
    });
  }
  async function signIn() {
    const response = await request("/api/v1/sign-in", {
      method: "POST",
      body: credentials,
    });
    assert.equal(response.status, 200, "Owner sign-in must succeed.");
    const cookieHeader = response.headers.get("set-cookie");
    assert.ok(
      cookieHeader?.includes("HttpOnly") &&
        cookieHeader.includes("SameSite=Strict") &&
        cookieHeader.includes("Path=/api/v1"),
      "Cookie attributes must restrict browser access.",
    );
    const value = await response.json();
    return {
      cookie: cookieHeader.split(";")[0],
      csrf: value.csrfToken,
      id: value.session.id,
    };
  }
  for (const path of ["/api/v1/bots", "/api/v1/session", "/api/v1/sessions"]) {
    const response = await request(path);
    assert.equal(response.status, 401, "Anonymous access must fail.");
    assert.doesNotMatch(
      await response.text(),
      /Research|"bots"|"owner"/,
      "Anonymous errors must contain no private fields.",
    );
  }
  assert.equal(
    (
      await request("/api/v1/bots", {
        method: "POST",
        body: { name: "anonymous" },
      })
    ).status,
    401,
  );
  for (const path of ["/setup", "/setup.js", "/owner", "/api/v1/setup"]) {
    for (const method of ["GET", "POST"]) {
      const response = await fetch(`${manager.mainURL}${path}`, {
        method,
        headers: {
          "X-Forwarded-For": "127.0.0.1",
          "X-Forwarded-Host": setup.host,
          Forwarded: `for=127.0.0.1;host=${setup.host}`,
        },
      });
      assert.equal(response.status, 404, "Setup must remain absent from main.");
    }
  }
  const page = await fetch(manager.mainURL);
  const html = await page.text();
  assert.equal(page.status, 200);
  assert.match(
    html,
    /<div id="root">/,
    "Release must serve the real application shell.",
  );
  const assets = Array.from(
    html.matchAll(/(?:src|href)="(\/assets\/[^" ]+)"/g),
    (match) => match[1],
  );
  assert.ok(
    assets.length >= 2,
    "Release must reference built JavaScript and CSS.",
  );
  for (const asset of assets)
    assert.equal(
      (await fetch(`${manager.mainURL}${asset}`)).status,
      200,
      "Copied binary must serve its embedded assets.",
    );
  for (const path of ["/assets/missing.js", "/missing", "/api/v1/missing"]) {
    const response = await request(path);
    assert.equal(response.status, 404);
    assert.doesNotMatch(
      await response.text(),
      /<div id="root">/,
      "Misses must not return the application shell.",
    );
  }
  let first = await signIn();
  for (const path of ["/api/v1/sign-in", "/api/v1/bots", "/api/v1/sign-out"]) {
    assert.equal(
      (
        await request(path, {
          method: "POST",
          body: credentials,
          session: first,
          origin: "http://foreign.invalid",
        })
      ).status,
      403,
      "Mutations must reject hostile origins.",
    );
  }
  const created = await request("/api/v1/bots", {
    method: "POST",
    body: { name: "Research" },
    session: first,
  });
  assert.equal(created.status, 201);
  const bot = await created.json();
  assert.match(bot.id, /^[a-f0-9]{32}$/);
  await manager.stop();
  manager = await startManager({ binary, dataDir, cwd: release });
  const durable = await request("/api/v1/session", { session: first });
  assert.equal(durable.status, 200, "Original cookie must survive restart.");
  assert.equal(
    (await durable.json()).session.id,
    first.id,
    "Restart must preserve the session identity.",
  );
  first = await signIn();
  const listing = await request("/api/v1/bots", { session: first });
  const bots = (await listing.json()).bots;
  assert.equal(bots.length, 1);
  assert.equal(
    bots[0].id,
    bot.id,
    "Explicit sign-in after restart must see the same bot ID.",
  );
  assert.equal(bots[0].name, "Research");
  const second = await signIn();
  assert.equal(
    (
      await request(`/api/v1/sessions/${second.id}`, {
        method: "DELETE",
        body: {},
        session: first,
        origin: "http://foreign.invalid",
      })
    ).status,
    403,
  );
  assert.equal(
    (
      await request(`/api/v1/sessions/${second.id}`, {
        method: "DELETE",
        body: {},
        session: first,
      })
    ).status,
    204,
  );
  assert.equal(
    (await request("/api/v1/bots", { session: second })).status,
    401,
  );
  await manager.stop();
  manager = await startManager({ binary, dataDir, setup: true, cwd: release });
  assert.equal(
    manager.setupURL,
    undefined,
    "An initialized installation must not reopen setup.",
  );
  assert.equal(
    (await request("/api/v1/bots", { session: second })).status,
    401,
    "Revocation must survive restart.",
  );
  assert.equal(
    (
      await request("/api/v1/sign-out", {
        method: "POST",
        body: {},
        session: first,
      })
    ).status,
    204,
  );
  assert.equal(
    (await request("/api/v1/bots", { session: first })).status,
    401,
    "Signed-out cookie must lose access.",
  );
  const current = await signIn();
  assert.equal(
    (
      await request(`/api/v1/sessions/${current.id}`, {
        method: "DELETE",
        body: {},
        session: current,
      })
    ).status,
    204,
  );
  assert.equal(
    (await request("/api/v1/bots", { session: current })).status,
    401,
    "Current-session revocation must lose access.",
  );
  console.log(
    `Built-binary acceptance passed. Restart preserved bot ${bot.id}.`,
  );
} finally {
  try {
    await manager?.stop();
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}
