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
assert(module, 'Pass an installed Playwright module path.');
const { chromium } = await import(pathToFileURL(path.resolve(module)).href);
const source = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const temporary = await fs.mkdtemp(path.join(os.tmpdir(), 'botstral-p01-memory-proof-'));
const baseline = process.argv.includes('--baseline');
const initial = baseline || process.argv.includes('--initial');
const baselineRef = process.env.BOTSTRAL_MEMORY_BASELINE_REF ?? 'HEAD';
const evidence = path.join(
  source,
  initial ? 'docs/evidence/p01-initial-memory' : 'docs/evidence/p01-desktop-memory',
);
await fs.mkdir(evidence, { recursive: true });
let browser;
try {
  const outDir = path.join(temporary, 'dist');
  await build({
    configFile: false,
    root: path.join(source, 'desktop'),
    base: './',
    plugins: [
      baseline && {
        name: 'baseline-memory',
        enforce: 'pre',
        transform(_code, id) {
          if (id === path.join(source, 'desktop/src/Memory.tsx'))
            return execFileSync('git', ['show', `${baselineRef}:desktop/src/Memory.tsx`], {
              cwd: source,
              encoding: 'utf8',
            });
        },
      },
      react(),
    ],
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
  const show = () => page.waitForTimeout(1500);
  page.on('pageerror', (e) => errors.push(e.message));
  await page.goto(pathToFileURL(filename).href + (initial ? '?initial=1' : ''));
  await page.getByRole('button', { name: /Inference bot/ }).click();
  await page.getByRole('heading', { name: 'Work with Inference bot' }).waitFor();
  await page.getByText('Memory', { exact: true }).click();
  if (baseline) {
    await page
      .getByText('No saved Memory. Initial authoring requires trusted manager administration.', {
        exact: true,
      })
      .waitFor();
    await page.screenshot({ path: path.join(evidence, 'before.png') });
    assert.deepEqual(errors, []);
    await fs.writeFile(
      path.join(evidence, 'baseline-proof.json'),
      JSON.stringify(
        {
          component_ref: execFileSync('git', ['rev-parse', baselineRef], {
            cwd: source,
            encoding: 'utf8',
          }).trim(),
          fixture: 'native-controls.json absent stage',
          limitations:
            'The Memory component comes from the baseline commit. The surrounding workspace and normalized native snapshot use the current evidence fixture.',
          errors,
        },
        null,
        2,
      ) + '\n',
    );
    await context.close();
    console.log('Captured baseline absent Memory from the unchanged Memory component.');
    process.exitCode = 0;
  } else if (initial) {
    await page.getByRole('button', { name: 'Create memory', exact: true }).click();
    assert.equal(await page.getByLabel('Source message').inputValue(), '');
    await page
      .getByLabel('Memory draft')
      .fill('Use concise answers.\nKeep the preference across new runs.');
    assert.equal(
      await page.getByRole('button', { name: 'Save memory', exact: true }).isDisabled(),
      true,
    );
    await show();
    const option = page.getByRole('option', { name: /Prefer concise answers\./ });
    const sourceMessage = await option.getAttribute('value');
    assert(sourceMessage);
    await page.getByLabel('Source message').selectOption(sourceMessage);
    const selector = await page.getByLabel('Source message').boundingBox();
    const panel = await page.locator('.memory-panel').boundingBox();
    assert(selector && panel && selector.x + selector.width <= panel.x + panel.width);
    await page.screenshot({ path: path.join(evidence, 'selected-source.png') });
    await show();
    await page.getByRole('button', { name: 'Save memory', exact: true }).click();
    await page.getByText('Saved revision 1', { exact: true }).waitFor();
    await page.getByText('Current memory matches this creation', { exact: true }).waitFor();
    await page.getByText(`Message ${sourceMessage}`, { exact: true }).waitFor();
    await show();
    await page.getByRole('button', { name: 'Refresh workspace', exact: true }).click();
    await page.getByText('Saved revision 1', { exact: true }).waitFor();
    await page.screenshot({ path: path.join(evidence, 'after.png') });
    await show();
    const video = page.video();
    await context.close();
    await video.saveAs(path.join(evidence, 'initial-memory.webm'));
    assert.deepEqual(errors, []);
    const report = {
      browser: await browser.version(),
      viewport: { width: 1440, height: 1080 },
      source:
        'Actual PairedWorkspace and Memory components consuming snapshots recorded by the real-manager native initial creation contract.',
      normalization:
        'Renderer generation and revision counters are normalized across native restarts. Saved documents and source previews are unchanged.',
      limitations:
        'This video replays recorded snapshots. Save checks the selected message and exact Markdown before returning the recorded native creation. Refresh returns the recorded post-restart snapshot. Native journal tests separately prove manager writes, manager restart and captured provider input. No external inference or execution occurs in this video.',
      actions: [
        'open initial authoring',
        'write a multiline preference',
        'verify save is blocked without a source',
        'explicitly select a same-bot message',
        'explicitly save revision one',
        'inspect authoritative provenance',
        'refresh to recorded post-restart state',
      ],
      errors,
      video: 'docs/evidence/p01-initial-memory/initial-memory.webm',
      sha256: createHash('sha256')
        .update(await fs.readFile(path.join(evidence, 'initial-memory.webm')))
        .digest('hex'),
    };
    await fs.writeFile(
      path.join(evidence, 'component-proof.json'),
      JSON.stringify(report, null, 2) + '\n',
    );
    console.log(JSON.stringify(report));
  } else {
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
  }
} finally {
  await browser?.close();
  await fs.rm(temporary, { recursive: true, force: true });
}
