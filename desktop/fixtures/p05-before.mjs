import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
assert.equal(execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim(), '271f9cd72dabd2d0749027b84d355ca046de5f7e', 'Capture this baseline only from the original source identity.');
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { createHash } from 'node:crypto';
import { build } from 'vite';
import react from '@vitejs/plugin-react';

const module = process.argv[2];
assert(module, 'Pass an installed Playwright module path.');
const { chromium } = await import(pathToFileURL(path.resolve(module)).href);
const source = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const temporary = await fs.mkdtemp(path.join(os.tmpdir(), 'botstral-p03-proof-'));
const evidence = path.join(source, 'docs/evidence/p05');
await fs.mkdir(evidence, { recursive: true });
let browser;
try {
  const outDir = path.join(temporary, 'dist');
  await build({
    configFile: false,
    root: path.join(source, 'desktop'),
    base: './',
    plugins: [react()],
    build: {
      outDir,
      emptyOutDir: true,
      rollupOptions: { input: path.join(source, 'desktop/fixtures/p03-proof.html') },
    },
  });
  const filename = path.join(outDir, 'fixtures/p03-proof.html');
  let html = await fs.readFile(filename, 'utf8');
  for (const match of [...html.matchAll(/<script\b[^>]*src="([^"]+)"[^>]*><\/script>/g)]) {
    const script = await fs.readFile(path.resolve(path.dirname(filename), match[1]), 'utf8');
    html = html.replace(
      match[0],
      () => '<script type="module">' + script.replaceAll('</script', '<\\/script') + '</script>',
    );
  }
  for (const match of [...html.matchAll(/<link\b[^>]*href="([^"]+\.css)"[^>]*>/g)]) {
    const css = await fs.readFile(path.resolve(path.dirname(filename), match[1]), 'utf8');
    html = html.replace(match[0], () => '<style>' + css + '</style>');
  }
  await fs.writeFile(filename, html);
  browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({
    viewport: { width: 1440, height: 1080 },
    timezoneId: 'Europe/Stockholm',
    recordVideo: { dir: temporary, size: { width: 1440, height: 1080 } },
  });
  const page = await context.newPage();
  const errors = [];
  const show = () => page.waitForTimeout(1500);
  page.on('pageerror', (e) => errors.push(e.message));
  await page.goto(pathToFileURL(filename).href);
  await page.getByRole('heading', { name: 'Work with Inference bot' }).waitFor();
  await page.screenshot({ path: path.join(evidence, 'component-before.png') });
  await context.close();
} finally {
  await browser?.close();
  await fs.rm(temporary, { recursive: true, force: true });
}
