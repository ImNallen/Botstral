import fs from 'node:fs/promises';
import { constants } from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { chromium } from 'playwright';
import { protection, isProtected, channel, safePath, login } from './protected.mjs';
import { fileURLToPath } from 'node:url';

const [mode, specPath, profile, outputRoot] = process.argv.slice(2);
const probe = mode === 'probe';
const spec = probe ? { viewport: { width: 800, height: 600 }, operations: [], outputs: [] } : JSON.parse(await fs.readFile(specPath, 'utf8'));
const declarations = new Map(spec.outputs.map(o => [o.slot, o]));
async function atomicFile(destination, write) {
  const temp = destination + '.' + crypto.randomUUID() + '.tmp';
  const file = await fs.open(temp, constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | constants.O_NOFOLLOW, 0o600);
  try { await write(file); await file.sync(); } finally { await file.close(); }
  await fs.rename(temp, destination);
  const directory = await fs.open(path.dirname(destination), constants.O_RDONLY);
  try { await directory.sync(); } finally { await directory.close(); }
}
async function bytes(slot, value) {
  if (value.length > declarations.get(slot).max_bytes) throw Error('output exceeds declaration');
  await atomicFile(path.join(outputRoot, slot + '.out'), f => f.writeFile(value));
}
async function exportFile(source, slot) {
  const file = await fs.open(source, constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK);
  try {
    const stat = await file.stat();
    if (!stat.isFile() || stat.size > declarations.get(slot).max_bytes) throw Error('invalid output file');
    await atomicFile(path.join(outputRoot, slot + '.out'), async target => {
      let offset = 0;
      const buffer = Buffer.alloc(16384);
      while (true) {
        const { bytesRead } = await file.read(buffer, 0, buffer.length, offset);
        if (!bytesRead) break;
        offset += bytesRead;
        if (offset > declarations.get(slot).max_bytes) throw Error('growing output file');
        await target.writeFile(buffer.subarray(0, bytesRead));
      }
    });
  } finally { await file.close(); }
}
const send = channel(specPath);
const runtime = path.dirname(fileURLToPath(import.meta.url));
let context;
try {
  if (probe || spec.operations.some(op => ['navigate', 'click', 'fill', 'capture_text', 'screenshot', 'protected_login'].includes(op.type))) {
    context = await chromium.launchPersistentContext(profile, { headless: true, viewport: spec.viewport, acceptDownloads: false, serviceWorkers: 'block', timeout: 15000 });
    context.setDefaultTimeout(10000);
    const page = context.pages()[0] ?? await context.newPage();
    if (probe) {
      await page.setContent('<body>botstral-runtime-probe</body>');
      if (await page.locator('body').innerText() !== 'botstral-runtime-probe') throw Error('probe failed');
      if (!(await page.screenshot()).length) throw Error('probe screenshot failed');
    }
  }
  const page = context?.pages()[0];
  for (const op of spec.operations) {
    const p = await protection(profile);
    switch (op.type) {
      case 'protected_login': await login(page, op, send, profile); break;
      case 'navigate': await page.goto(op.url, { waitUntil: 'domcontentloaded', timeout: 15000 }); break;
      case 'click': await page.locator(op.selector).click(); break;
      case 'fill': await page.locator(op.selector).fill(op.text); break;
      case 'capture_text': {
        let text = await page.locator(op.selector).innerText();
        if (isProtected(p)) {
          const reply = await send({ type: 'screen', protection: p, text });
          text = '';
          if (reply.type !== 'screened') throw Error('protected observation withheld');
          text = reply.text;
        }
        await bytes(op.output, Buffer.from(text));
        text = '';
        break;
      }
      case 'screenshot': if (isProtected(p)) throw Error('protected capture refused'); await bytes(op.output, await page.screenshot({ fullPage: false, type: 'png' })); break;
      case 'write_file': await atomicFile(await safePath(op.path, profile, runtime), f => f.writeFile(op.text)); break;
      case 'read_file':
      case 'export_file': if (isProtected(p)) throw Error('protected export refused'); await exportFile(await safePath(op.path, profile, runtime), op.output); break;
      default: throw Error('unsupported operation');
    }
  }
} catch {
  process.exitCode = 1;
  process.stderr.write('computer operation failed\n');
} finally {
  if (context) await context.close();
}
