import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { build } from 'vite';
import react from '@vitejs/plugin-react';
import { chromium } from '../../agent/runtime/node_modules/playwright/index.mjs';

const temporary = await fs.mkdtemp(path.join(os.tmpdir(), 'botstral-p06-component-'));
const evidence = process.env.BOTSTRAL_P06_EVIDENCE ?? 'docs/evidence/p06/storage-interaction';
let browser;
try {
  await fs.mkdir(evidence, { recursive: true });
  await build({ configFile: false, root: 'desktop', base: './', plugins: [react()],
    build: { outDir: temporary, emptyOutDir: true, rollupOptions: { input: 'desktop/fixtures/p06-entry.html' } } });
  const filename = path.join(temporary, 'fixtures/p06-entry.html');
  let html = await fs.readFile(filename, 'utf8');
  for (const match of [...html.matchAll(/<script\b[^>]*src="([^"]+)"[^>]*><\/script>/g)]) {
    const script = await fs.readFile(path.resolve(path.dirname(filename), match[1]), 'utf8');
    html = html.replace(match[0], () => '<script type="module">' + script.replaceAll('</script', '<\\/script') + '</script>');
  }
  for (const match of [...html.matchAll(/<link\b[^>]*href="([^"]+\.css)"[^>]*>/g)]) {
    const css = await fs.readFile(path.resolve(path.dirname(filename), match[1]), 'utf8');
    html = html.replace(match[0], () => '<style>' + css + '</style>');
  }
  await fs.writeFile(filename, html);
  browser = await chromium.launch({ executablePath: process.env.BOTSTRAL_CHROME ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true });
  const context = await browser.newContext({ viewport: { width: 1100, height: 700 }, recordVideo: { dir: temporary } });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', e => errors.push(e.message));
  await page.goto(pathToFileURL(filename).href);
  await page.getByRole('article', { name: 'Vault handoff' }).waitFor();
  await page.getByRole('button', { name: 'Protected login entry', exact: true }).click();
  await page.getByLabel('Login label', { exact: true }).fill('Example account');
  await page.getByLabel('Exact allowed origin', { exact: true }).fill('https://example.test');
  await page.getByLabel('Permit remembered password rules for this exact origin').check();
  assert.equal(await page.locator('input[type=password]').count(), 0);
  assert.equal(await page.getByRole('button', { name: 'Enter private login' }).isEnabled(), true);
  await page.screenshot({ path: path.join(evidence, 'handoff.png') });
  const video = page.video();
  await context.close();
  await video.saveAs(path.join(evidence, 'handoff.webm'));
  assert.deepEqual(errors, []);
  await fs.writeFile(path.join(evidence, 'component.json'), JSON.stringify({ source: 'Real MemoryNotice component. Synthetic value-free handoff. Component replay, not packaged native execution.', browser: await browser.version(), errors }, null, 2) + '\n');
} finally {
  await browser?.close();
  await fs.rm(temporary, { recursive: true, force: true });
}
