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
const temporary = await fs.mkdtemp(path.join(os.tmpdir(), 'botstral-p05-proof-'));
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
      rollupOptions: { input: path.join(source, 'desktop/fixtures/p05-proof.html') },
    },
  });
  const filename = path.join(outDir, 'fixtures/p05-proof.html');
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
  await page.getByRole('button', { name: 'Saved results', exact: true }).click();
  const library = page.getByRole('dialog', { name: 'Saved results', exact: true });
  await library.getByText('00-old-result.txt', { exact: true }).waitFor();
  await show();
  await page.screenshot({ path: path.join(evidence, 'component-after.png') });
  await library.getByRole('button', { name: 'Next results page' }).click();
  assert.equal(await library.locator('.saved-result-list > li').count(), 3);
  await show();
  await library.getByRole('button', { name: 'Refresh results' }).click();
  await library.getByLabel('Result bot').selectOption({ label: 'Other result bot' });
  await library.getByText('literal %_*.txt', { exact: true }).waitFor();
  await show();
  await library.getByLabel('Result bot').selectOption('');
  await library.getByLabel('File name contains').fill('%_*');
  await library.getByText('literal %_*.txt', { exact: true }).waitFor();
  await show();
  await library.getByLabel('File name contains').fill('00-old');
  await library.getByRole('button', { name: 'Source task', exact: true }).click();
  const sourceTask = library.getByRole('region', { name: 'Retained source task' });
  await sourceTask.waitFor();
  await sourceTask.getByText('Exact action and target', { exact: true }).click();
  await sourceTask.scrollIntoViewIfNeeded();
  await show();
  assert.match(await sourceTask.innerText(), /Exact ordinary action contents must remain visible/);
  await sourceTask.getByRole('button', { name: 'Save verified file' }).click();
  await page.getByRole('dialog', { name: 'Replay save chooser' }).waitFor();
  await show();
  await page.getByRole('button', { name: 'Cancel replay save' }).click();
  await sourceTask.getByRole('button', { name: 'Save verified file' }).click();
  await page.getByRole('button', { name: 'Save replay file' }).click();
  await show();
  await page.screenshot({ path: path.join(evidence, 'source-task.png') });
  await page.getByText('The verified file was saved.', { exact: true }).waitFor();
  await library.getByRole('button', { name: 'Close saved results' }).click();
  await show();
  const video = page.video();
  await context.close();
  await video.saveAs(path.join(evidence, 'saved-results.webm'));
  assert.deepEqual(errors, []);
  const report = {
    browser: await browser.version(),
    native_views_sha256: createHash('sha256').update(await fs.readFile(path.join(evidence, 'native-views.json'))).digest('hex'),
    viewport: { width: 1440, height: 1080 },
    source: 'Actual PairedWorkspace, SavedResults, TaskCard, and PairedController components using captured real manager/native reads.',
    limitations: 'Replay only. Synthetic worker. Simulated save chooser and notices. Real byte verification, restart, restore, and private disk saves are separately tested. Packaged native UI was not exercised.',
    actions: ['open library', '32 plus 3 result paging', 'bot filter', 'literal wildcard fragment', 'retained source task', 'exact action', 'cancel simulated chooser', 'complete simulated chooser'],
    errors,
    video: 'docs/evidence/p05/saved-results.webm',
    sha256: createHash('sha256').update(await fs.readFile(path.join(evidence, 'saved-results.webm'))).digest('hex'),
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
