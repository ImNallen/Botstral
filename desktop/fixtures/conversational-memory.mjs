import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { build } from 'vite';
import react from '@vitejs/plugin-react';

const module = process.argv[2];
assert(module, 'Pass an ephemeral Playwright module path.');
const { chromium } = await import(pathToFileURL(path.resolve(module)).href);
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const temporary = await fs.mkdtemp(path.join(os.tmpdir(), 'botstral-memory-proof-'));
const evidence = path.join(root, 'docs/evidence/conversational-memory');
const baseline = process.argv.includes('--baseline');
const baselineRef = '41aec24ae8b62b7cb35874cdd4feb3760425e198';
const fixture = baseline ? 'p01-memory-proof' : 'conversational-memory';
const baselineFiles = baseline
  ? new Set(
      execFileSync(
        'git',
        ['ls-tree', '-r', '--name-only', baselineRef, 'desktop', 'docs/evidence'],
        { cwd: root, encoding: 'utf8' },
      )
        .trim()
        .split('\n'),
    )
  : new Set();
const previous = (file) =>
  execFileSync('git', ['show', `${baselineRef}:${file}`], { cwd: root, encoding: 'utf8' });
let browser;
try {
  const outDir = path.join(temporary, 'dist');
  await build({
    configFile: false,
    root: path.join(root, 'desktop'),
    base: './',
    plugins: [
      baseline && {
        name: 'accepted-baseline',
        enforce: 'pre',
        resolveId(source, importer) {
          if (!source.startsWith('.') && !path.isAbsolute(source)) return;
          const base = path.isAbsolute(source)
            ? source
            : path.resolve(path.dirname(importer?.split('?')[0] ?? root), source);
          for (const candidate of [base, `${base}.tsx`, `${base}.ts`, `${base}.css`]) {
            if (baselineFiles.has(path.relative(root, candidate))) return candidate;
          }
        },
        load(id) {
          const relative = path.relative(root, id.split('?')[0]);
          if (baselineFiles.has(relative)) return previous(relative);
        },
      },
      react(),
    ],
    build: {
      outDir,
      emptyOutDir: true,
      rollupOptions: { input: path.join(root, `desktop/fixtures/${fixture}.html`) },
    },
  });
  const filename = path.join(outDir, `fixtures/${fixture}.html`);
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
  browser = await chromium.launch({
    headless: true,
    ...(process.env.BOTSTRAL_CHROME ? { executablePath: process.env.BOTSTRAL_CHROME } : {}),
  });
  const context = await browser.newContext({
    viewport: { width: 1440, height: 1080 },
    timezoneId: 'Europe/Stockholm',
    ...(baseline ? {} : { recordVideo: { dir: temporary, size: { width: 1440, height: 1080 } } }),
  });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto(pathToFileURL(filename).href);
  await page.getByRole('button', { name: /Inference bot/ }).click();
  await page.getByRole('heading', { name: 'Work with Inference bot' }).waitFor();
  if (baseline) {
    await page.getByText('Memory', { exact: true }).click();
    await page.getByRole('button', { name: 'Edit memory', exact: true }).waitFor();
    await page.screenshot({ path: path.join(evidence, 'before.png') });
    await context.close();
  } else {
    assert.equal(
      await page.getByRole('button', { name: /Edit memory|Create memory|Save memory/ }).count(),
      0,
    );
    const steps = [
      [
        'Remember that I prefer concise answers for this bot.',
        'Saved. Individual memory for this bot.',
      ],
      ['Remember morning meetings for all bots.', 'Saved. Shared memory for all bots.'],
      [
        'Update my individual preference to short, complete sentences.',
        'Updated. Individual memory for this bot.',
      ],
      ['List our shared memories.', 'Shared memory for all bots.'],
      ['Forget my individual response preference.', 'Deleted. Individual memory for this bot.'],
      [
        'Can you save a reusable login for me?',
        'Reusable login-item storage is not supported until P06. Do not send login secrets in chat. No secret was saved.',
      ],
    ];
    for (const [prompt, notice] of steps) {
      await page.getByLabel('Message', { exact: true }).fill(prompt);
      assert.equal(
        await page.getByRole('button', { name: 'Send', exact: true }).isDisabled(),
        false,
      );
      await page.getByRole('button', { name: 'Send', exact: true }).click();
      await page.getByText(notice, { exact: true }).last().scrollIntoViewIfNeeded();
      await page.waitForTimeout(1200);
    }
    await page.getByRole('button', { name: 'Refresh workspace', exact: true }).click();
    const updated = page
      .getByRole('article', { name: 'Committed memory change' })
      .filter({ hasText: 'Updated.' });
    await updated.scrollIntoViewIfNeeded();
    await updated.getByText('Use short, complete sentences.', { exact: true }).waitFor();
    await page.screenshot({ path: path.join(evidence, 'after.png') });
    await page.waitForTimeout(1500);
    const video = page.video();
    await context.close();
    await video.saveAs(path.join(evidence, 'conversation.webm'));
    await fs.writeFile(
      path.join(evidence, 'component-proof.json'),
      JSON.stringify(
        {
          browser: await browser.version(),
          baseline_ref: baselineRef,
          source:
            'Real PairedWorkspace and MemoryNotice components consuming recorded native snapshots from a real manager and HTTP provider fixture.',
          normalization:
            'Renderer generation and revision counters are normalized. Durable runs, outcomes, scopes and saved text are unchanged.',
          limitations:
            'Component replay, not packaged Tauri execution. Send validates manager-only policy and returns the corresponding native capture. The separate native contract proves persistence and manager restart. No paid inference or worker execution occurs in this replay.',
          actions: steps.map(([prompt]) => prompt),
          errors,
          sha256: createHash('sha256')
            .update(await fs.readFile(path.join(evidence, 'conversation.webm')))
            .digest('hex'),
        },
        null,
        2,
      ) + '\n',
    );
  }
  assert.deepEqual(errors, []);
  console.log(
    baseline
      ? 'Captured accepted baseline components.'
      : 'Recorded conversation notices through the real components.',
  );
} finally {
  await browser?.close();
  await fs.rm(temporary, { recursive: true, force: true });
}
