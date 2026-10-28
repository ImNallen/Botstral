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
const temporary = await fs.mkdtemp(path.join(os.tmpdir(), 'botstral-p03-proof-'));
const evidence = path.join(source, 'docs/evidence/p03');
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
  await page.getByText('Routines and follow-ups', { exact: true }).click();
  await page.getByText('Cumulative owner budget', { exact: true }).click();
  await page.getByRole('button', { name: 'Save owner budget' }).click();
  await page.getByRole('button', { name: 'Create routine', exact: true }).click();
  await page.getByLabel('Routine name', { exact: true }).fill('Daily briefing');
  await page.getByLabel('Routine instruction', { exact: true }).fill('Prepare the daily briefing.');
  await page.getByLabel('IANA time zone').fill('Europe/Stockholm');
  await page.getByRole('checkbox', { name: /Allow at most ten/ }).check();
  await page.getByRole('button', { name: 'Save enabled routine' }).click();
  const routine = page.getByRole('article', { name: 'Routine Daily briefing' });
  await routine.waitFor();
  await show();
  await routine.getByRole('button', { name: 'Pause routine' }).click();
  await routine.getByText('paused', { exact: true }).waitFor();
  await show();
  await routine.getByRole('button', { name: 'Resume routine' }).click();
  await routine.getByText('enabled', { exact: true }).waitFor();
  await show();
  await routine.getByRole('button', { name: 'Preview next times' }).click();
  await page.getByLabel('Schedule preview').waitFor();
  await page.getByLabel('Schedule preview').scrollIntoViewIfNeeded();
  await show();
  await routine.getByRole('button', { name: 'Run live bounded test' }).click();
  await page.getByRole('region', { name: 'Durable inbox' }).waitFor();
  await page.getByRole('region', { name: 'Durable inbox' }).scrollIntoViewIfNeeded();
  await show();
  await routine.getByRole('button', { name: 'Routine history', exact: true }).click();
  await page
    .getByLabel('History for Daily briefing')
    .getByText(/Finished: completed/)
    .waitFor();
  await page.getByLabel('History for Daily briefing').scrollIntoViewIfNeeded();
  await show();
  await routine.getByRole('button', { name: 'Edit routine' }).click();
  const editor = page.getByRole('textbox', { name: 'Definition for Daily briefing' });
  const definition = JSON.parse(await editor.inputValue());
  definition.name = 'Edited briefing';
  definition.template.prompt = 'Prepare the revised daily briefing.';
  await editor.fill(JSON.stringify(definition, null, 2));
  await page.getByRole('button', { name: 'Save routine changes' }).click();
  await page.getByRole('article', { name: 'Routine Edited briefing' }).waitFor();
  await show();
  await page.getByLabel('Message', { exact: true }).fill('/routine help');
  await page.getByRole('button', { name: 'Send', exact: true }).click();
  await page
    .getByRole('region', { name: 'Owner command reply' })
    .getByText(/Owner commands/)
    .waitFor();
  await page.getByRole('region', { name: 'Owner command reply' }).scrollIntoViewIfNeeded();
  await show();
  await page.getByText('Bot lifecycle', { exact: true }).click();
  await page.getByRole('button', { name: 'Duplicate bot with paused routines' }).click();
  await page.getByRole('button', { name: /Inference bot copy/ }).waitFor();
  await show();
  await page.screenshot({ path: path.join(evidence, 'component-after.png') });
  await page
    .getByRole('article', { name: 'Routine Edited briefing' })
    .getByRole('button', { name: 'Delete routine', exact: true })
    .click();
  await page
    .getByRole('article', { name: 'Routine Edited briefing' })
    .waitFor({ state: 'detached' });
  await page.getByRole('button', { name: 'Acknowledge inbox entry' }).click();
  await page.getByRole('region', { name: 'Durable inbox' }).waitFor({ state: 'detached' });
  const video = page.video();
  await context.close();
  await video.saveAs(path.join(evidence, 'routine-controls.webm'));
  assert.deepEqual(errors, []);
  const report = {
    browser: await browser.version(),
    viewport: { width: 1440, height: 1080 },
    source:
      'Unmodified PairedWorkspace and Routines components consuming recorded actual-manager native snapshots.',
    normalization:
      'Local renderer generation and revision counters are normalized across the recorded native process restarts. Durable manager state is unchanged.',
    limitations:
      'Replay responses only in this video. Synthetic inference and worker. Real manager control and journal behavior is checked separately by p03_routine_controls_and_crash_journal_recovery. No OS notification or real guest execution claim.',
    actions: [
      'budget',
      'create',
      'pause',
      'resume',
      'preview',
      'live test result',
      'history',
      'edit',
      'conversation help',
      'paused duplication',
      'delete',
      'acknowledge inbox',
    ],
    errors,
    video: 'docs/evidence/p03/routine-controls.webm',
    sha256: createHash('sha256')
      .update(await fs.readFile(path.join(evidence, 'routine-controls.webm')))
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
