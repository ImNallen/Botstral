import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { build } from 'vite';
import react from '@vitejs/plugin-react';
import { chromium } from '../../agent/runtime/node_modules/playwright/index.mjs';

export async function verifyReviewUi(root, review, decide, evidence) {
  const output = path.join(root, 'review-ui');
  await build({ configFile: false, root: 'desktop', base: './', plugins: [react()], logLevel: 'error', build: { outDir: output, emptyOutDir: true, rollupOptions: { input: 'desktop/fixtures/p06-review.html' } } });
  const filename = path.join(output, 'fixtures/p06-review.html');
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
  const browser = await chromium.launch({ headless: true });
  let result;
  try {
    const context = await browser.newContext({ viewport: { width: 1100, height: 700 }, recordVideo: { dir: output } });
    const page = await context.newPage();
    const errors = [];
    page.on('pageerror', () => errors.push('component error'));
    await page.exposeFunction('fixtureInvoke', async (command, args) => {
      if (command === 'credential_uses') return result ? [] : [review];
      if (command === 'decide_credential_use') {
        assert.deepEqual(args.review, review);
        assert.equal(args.decision, 'allow_once');
        result = await decide(args.review, args.decision);
        assert.equal(result.type, 'review');
        return;
      }
      throw Error('unexpected fixture command');
    });
    await page.addInitScript(() => { window.__TAURI_INTERNALS__ = { invoke: (command, args) => window.fixtureInvoke(command, args) }; });
    await page.goto(pathToFileURL(filename).href);
    await page.getByRole('button', { name: 'Allow once', exact: true }).waitFor();
    await page.screenshot({ path: path.join(evidence, 'live-review.png') });
    await page.getByRole('button', { name: 'Allow once', exact: true }).click();
    await page.getByRole('status').filter({ hasText: 'Decision recorded' }).waitFor();
    await page.screenshot({ path: path.join(evidence, 'live-decision.png') });
    const video = page.video();
    await context.close();
    await video.saveAs(path.join(evidence, 'live-review.webm'));
    assert.deepEqual(errors, []);
    return result;
  } finally { await browser.close(); }
}
