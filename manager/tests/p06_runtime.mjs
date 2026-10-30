import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';
import net from 'node:net';
import http from 'node:http';
import crypto from 'node:crypto';
import { spawn } from 'node:child_process';

export async function verifyRuntime(root, profile, send, privateBody) {
  const fields = [...new URLSearchParams(privateBody).values()];
  const secret = fields[fields.length - 1];
  const html = '<body><div id="safe">Safe account status</div><div id="echo"></div><canvas></canvas><script>const v=' + JSON.stringify(secret).replaceAll('<', '\\u003c') + ';document.querySelector("#echo").textContent=v;localStorage.setItem("echo",v);document.querySelector("canvas").getContext("2d").fillText(v,10,10)</script></body>';
  const server = http.createServer((_, response) => response.end(html));
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const origin = `http://127.0.0.1:${server.address().port}`;
  const execution = path.join(root, 'capture-execution');
  await fs.mkdir(execution);
  const gateway = net.createServer({ allowHalfOpen: true }, socket => {
    const chunks = [];
    socket.on('data', chunk => chunks.push(chunk));
    socket.on('end', async () => {
      const bytes = Buffer.concat(chunks);
      try { socket.end(JSON.stringify(await send(JSON.parse(bytes.toString())))); }
      catch { socket.end('{"type":"refused"}'); }
      finally { bytes.fill(0); for (const chunk of chunks) chunk.fill(0); }
    });
  });
  await new Promise(resolve => gateway.listen(path.join(execution, 'protected.sock'), resolve));
  const results = [];
  const initialHistory = await fs.readFile(path.join(root, 'automation-protection.json'), 'utf8');
  const markerPath = path.join(profile, '.botstral-profile.json');
  const initialMarker = await fs.readFile(markerPath, 'utf8');
  try {
    for (const test of ['safe_text', 'echo_text', 'screenshot', 'profile_file', 'alias_export', 'missing_inventory', 'corrupt_provenance', 'metadata_rollback', 'partial_publication', 'legacy_marker']) {
      await fs.writeFile(path.join(root, 'automation-protection.json'), initialHistory);
      await fs.writeFile(markerPath, initialMarker);
      const outputRoot = path.join(execution, test);
      await fs.mkdir(outputRoot);
      const output = { slot: 'result', name: 'result', media_type: test === 'screenshot' ? 'png' : 'text_plain', max_bytes: 8192 };
      let operation = test === 'screenshot' ? { type: 'screenshot', output: 'result' } : { type: 'capture_text', selector: test === 'echo_text' ? '#echo' : '#safe', output: 'result' };
      if (test === 'profile_file') operation = { type: 'read_file', path: path.join(profile, '.botstral-profile.json'), output: 'result' };
      if (test === 'alias_export') {
        const alias = path.join(execution, 'alias');
        await fs.symlink(profile, alias);
        operation = { type: 'export_file', path: path.join(alias, '.botstral-profile.json'), output: 'result' };
      }
      if (test === 'missing_inventory') {
        const history = JSON.parse(initialHistory); history.protection.releases.push(crypto.randomUUID());
        await fs.writeFile(path.join(root, 'automation-protection.json'), JSON.stringify(history));
        await fs.writeFile(markerPath, JSON.stringify({ profile: history.protection.profile, releases: history.protection.releases }));
      }
      if (test === 'corrupt_provenance') await fs.writeFile(path.join(root, 'automation-protection.json'), '{}');
      if (test === 'metadata_rollback') {
        const history = JSON.parse(initialHistory); history.protection.releases = [];
        await fs.writeFile(path.join(root, 'automation-protection.json'), JSON.stringify(history));
        operation = { type: 'screenshot', output: 'result' };
      }
      if (test === 'partial_publication') {
        const marker = JSON.parse(initialMarker); marker.releases.push(crypto.randomUUID());
        await fs.writeFile(markerPath, JSON.stringify(marker));
      }
      if (test === 'legacy_marker') await fs.writeFile(markerPath, JSON.stringify(JSON.parse(initialMarker).profile));
      if (operation.type === 'screenshot') output.media_type = 'png';
      const spec = path.join(execution, 'spec.json');
      await fs.writeFile(spec, JSON.stringify({ viewport: { width: 800, height: 600 }, operations: [{ type: 'navigate', url: origin }, operation], outputs: [output] }));
      const child = spawn(process.execPath, ['agent/runtime/run.mjs', 'run', spec, profile, outputRoot], { stdio: ['ignore', 'pipe', 'pipe'] });
      const chunks = [];
      child.stdout.on('data', chunk => chunks.push(chunk)); child.stderr.on('data', chunk => chunks.push(chunk));
      const exit = await new Promise(resolve => child.on('exit', resolve));
      const log = Buffer.concat(chunks).toString();
      assert.equal(exit, test === 'safe_text' ? 0 : 1, `${test} runtime exit`);
      assert.equal(log, test === 'safe_text' ? '' : 'computer operation failed\n');
      assert.deepEqual(await fs.readdir(outputRoot), test === 'safe_text' ? ['result.out'] : []);
      if (test === 'safe_text') assert.equal(await fs.readFile(path.join(outputRoot, 'result.out'), 'utf8'), 'Safe account status');
      results.push({ scenario: test, exit, passed: true });
    }
    async function scan(directory) {
      for (const entry of await fs.readdir(directory, { withFileTypes: true })) {
        if (entry.isDirectory()) await scan(path.join(directory, entry.name));
        else if (entry.isFile()) {
          const bytes = await fs.readFile(path.join(directory, entry.name));
          for (const value of fields) for (const encoded of [value, JSON.stringify(value).slice(1, -1), JSON.stringify(JSON.stringify(value)).slice(1, -1)]) assert.equal(bytes.includes(Buffer.from(encoded)), false, 'private value in operational file');
        }
      }
    }
    await scan(execution);
    return { results, scan: 'Actual runtime specifications and output files, raw and escaped synthetic inventory. Browser profile is the protected private state, not a public artifact.' };
  } finally {
    await fs.writeFile(path.join(root, 'automation-protection.json'), initialHistory);
    await fs.writeFile(markerPath, initialMarker);
    await new Promise(resolve => gateway.close(resolve));
    await new Promise(resolve => server.close(resolve));
  }
}
