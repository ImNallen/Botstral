import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { build } from 'vite';
import react from '@vitejs/plugin-react';

const source = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const playwright = process.argv[2];
assert(playwright, 'Pass the installed Playwright module path.');
const { chromium } = await import(pathToFileURL(path.resolve(playwright)).href);
const temporary = await fs.mkdtemp(path.join(os.tmpdir(), 'botstral-m06-approval-'));
const fixture = JSON.parse(
  await fs.readFile(
    path.join(source, 'docs/evidence/m06-parent/approval-projections.json'),
    'utf8',
  ),
);
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
      rollupOptions: { input: path.join(source, 'desktop/fixtures/m06-approval-proof.html') },
    },
  });
  const filename = path.join(outDir, 'fixtures/m06-approval-proof.html');
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
  const page = await browser.newPage({
    viewport: { width: 1440, height: 1100 },
    deviceScaleFactor: 1,
  });
  const errors = [];
  page.on('pageerror', (error) => errors.push(error.message));
  const results = [];
  await fs.mkdir(path.join(source, 'docs/assets'), { recursive: true });
  for (const stage of ['before', 'after']) {
    await page.goto(pathToFileURL(filename).href + '?stage=' + stage);
    const dialog = page.getByRole('dialog', { name: 'Review exact action' });
    await dialog.waitFor();
    await page.evaluate(() => document.fonts.ready);
    assert.equal(await dialog.locator('pre').textContent(), fixture[stage].review.action);
    await page.getByRole('button', { name: 'Approve exact action' }).waitFor();
    const target = path.join(source, 'docs/assets/m06-approval-' + stage + '.png');
    await page.screenshot({ path: target });
    results.push({
      stage,
      path: path.relative(source, target),
      sha256: createHash('sha256')
        .update(await fs.readFile(target))
        .digest('hex'),
    });
  }
  assert.deepEqual(errors, []);
  const report = {
    viewport: { width: 1440, height: 1100 },
    browser: await browser.version(),
    source:
      'Actual native RemoteSnapshot review serialization and unmodified React approval component.',
    before: 'Existing process action supported before M06.',
    after: 'New M06 connector action.',
    authority: 'Read-only fixture. Every mutation is rejected.',
    errors,
    screenshots: results,
  };
  await fs.writeFile(
    path.join(source, 'docs/evidence/m06-parent/approval-renderer.json'),
    JSON.stringify(report, null, 2) + '\n',
  );
  console.log(JSON.stringify(report));
} finally {
  await browser?.close();
  await fs.rm(temporary, { recursive: true, force: true });
}
