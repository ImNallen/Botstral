import assert from 'node:assert/strict';
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
const temporary = await fs.mkdtemp(path.join(os.tmpdir(), 'botstral-p01-memory-proof-'));
const evidence = path.join(source, 'docs/evidence/p01-desktop-memory');
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
      rollupOptions: { input: path.join(source, 'desktop/fixtures/p01-memory-proof.html') },
    },
  });
  const filename = path.join(outDir, 'fixtures/p01-memory-proof.html');
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
  await page.getByText('Memory', { exact: true }).click();
  await page.getByText('Saved revision 1', { exact: true }).waitFor();
  await show();
  await page.getByRole('button', { name: 'Edit memory', exact: true }).click();
  await page
    .getByLabel('Memory draft', { exact: true })
    .fill('Corrected saved preference.\nUse concise answers.');
  await show();
  await page.getByRole('button', { name: 'Save memory', exact: true }).click();
  await page.getByText('Saved revision 2', { exact: true }).waitFor();
  await page.getByText('Current memory matches this edit', { exact: true }).waitFor();
  await show();
  await page.getByRole('button', { name: 'Clear current memory', exact: true }).click();
  await page.getByLabel('Memory draft', { exact: true }).fill('');
  await show();
  await page.getByRole('button', { name: 'Save memory', exact: true }).click();
  await page.getByText('Saved Memory is blank.', { exact: true }).waitFor();
  await show();
  await page.getByRole('button', { name: 'Edit memory', exact: true }).click();
  await page.getByLabel('Memory draft', { exact: true }).fill('Reviewed draft after conflict.');
  await page.getByRole('button', { name: 'Refresh workspace', exact: true }).click();
  await page.getByText('Saved revision 4', { exact: true }).waitFor();
  await page.getByText(/Memory changed since this draft began/).waitFor();
  assert.equal(
    await page.getByRole('button', { name: 'Save memory', exact: true }).isDisabled(),
    true,
  );
  await page.getByRole('button', { name: 'Review draft against current', exact: true }).waitFor();
  await page.screenshot({ path: path.join(evidence, 'conflict.png') });
  await show();
  await page.getByRole('button', { name: 'Review draft against current', exact: true }).click();
  await show();
  await page.getByRole('button', { name: 'Save memory', exact: true }).click();
  await page.getByText('Saved revision 5', { exact: true }).waitFor();
  await page.getByText('Current memory matches this edit', { exact: true }).waitFor();
  await page.screenshot({ path: path.join(evidence, 'after.png') });
  await show();
  const video = page.video();
  await context.close();
  await video.saveAs(path.join(evidence, 'memory-controls.webm'));
  assert.deepEqual(errors, []);
  const report = {
    browser: await browser.version(),
    viewport: { width: 1440, height: 1080 },
    source:
      'Real PairedWorkspace and Memory components consuming snapshots captured from the actual-manager native journal test.',
    normalization:
      'Renderer generation and revision counters are normalized across native service restarts. Durable documents remain unchanged.',
    limitations:
      'Snapshot replay proves UI behavior. The native journal test proves manager writes and recovery. The competing-current snapshot is injected by Refresh. No external inference or execution occurs in this video.',
    actions: [
      'inspect and provenance',
      'multiline edit',
      'explicit save',
      'prepare blank draft',
      'explicit clear save',
      'preserve draft on conflicting refresh',
      'explicit review against current',
      'deliberate save',
    ],
    errors,
    video: 'docs/evidence/p01-desktop-memory/memory-controls.webm',
    sha256: createHash('sha256')
      .update(await fs.readFile(path.join(evidence, 'memory-controls.webm')))
      .digest('hex'),
  };
  await fs.writeFile(
    path.join(evidence, 'component-proof.json'),
    JSON.stringify(report, null, 2) + '\n',
  );
  console.log(JSON.stringify(report));
} finally {
  await browser?.close();
  await fs.rm(temporary, { recursive: true, force: true });
}
