import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { createHash } from 'node:crypto';
import { build } from '/Users/nallen/.t3/worktrees/Botstral/t3-6ce83c69/node_modules/vite/dist/node/index.js';
import react from '/Users/nallen/.t3/worktrees/Botstral/t3-6ce83c69/node_modules/@vitejs/plugin-react/dist/index.js';

const module = process.argv[2];
assert(module, 'Pass an installed Playwright module path.');
const { chromium } = await import(pathToFileURL(path.resolve(module)).href);
const source = '/Users/nallen/.t3/worktrees/Botstral/t3-6ce83c69';
const temporary = await fs.mkdtemp(path.join(os.tmpdir(), 'botstral-p05-proof-'));
const evidence = '/Users/nallen/.t3/worktrees/Botstral/t3-9cf8cc22/.audit/p05/reviewer-probes';
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
  await library.getByLabel('File name contains').fill('00-old');
  await library.getByRole('button', { name: 'Save verified file' }).click();
  await page.getByRole('button', { name: 'Save replay file' }).click();
  await page.getByText('The verified file was saved.', { exact: true }).waitFor();
  const notice = page.getByText('The verified file was saved.', { exact: true });
  const success = {
    textInDom: await notice.count(),
    noticeInLibrary: await library.getByText('The verified file was saved.', { exact: true }).count(),
    accessibleStatuses: await page.getByRole('status').allTextContents(),
    accessibilityTree: await page.locator('body').ariaSnapshot(),
    modalOpen: await library.evaluate((el) => el.matches(':modal')),
  };
  await page.screenshot({ path: path.join(evidence, 'save-feedback.png') });
  await library.getByRole('button', { name: 'Close saved results' }).click();
  success.noticeAfterClose = await notice.count();
  assert.equal(success.textInDom, 1);
  await fs.writeFile(path.join(evidence, 'save-feedback.json'), JSON.stringify(success, null, 2) + '\n');
  assert.equal(success.noticeInLibrary, 1, 'Save completion must render inside the open Saved results dialog.');
  assert.equal(success.modalOpen, true);

  assert.equal(success.noticeAfterClose, 0);
  await fs.writeFile(path.join(evidence, 'save-feedback.json'), JSON.stringify(success, null, 2) + '\n');
  console.log(JSON.stringify(success));
  const video = page.video();
  await context.close();
  await video.saveAs(path.join(evidence, 'saved-results.webm'));
  assert.deepEqual(errors, []);
  console.log(JSON.stringify({ browser: await browser.version(), errors }));
} finally {
  await browser?.close();
  await fs.rm(temporary, { recursive: true, force: true });
}
