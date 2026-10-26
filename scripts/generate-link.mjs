import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { compile } from 'json-schema-to-typescript';

const schemaPath = new URL('../link/schema/protocol.json', import.meta.url);
const outputPath = new URL('../link/typescript/protocol.d.ts', import.meta.url);
const source = await readFile(schemaPath, 'utf8');
const exported = execFileSync(
  'cargo',
  ['run', '--quiet', '--locked', '-p', 'botstral-link', '--bin', 'export-schema'],
  { encoding: 'utf8' },
);
if (source !== exported)
  throw new Error(
    'Rust protocol schema drift. Regenerate link/schema/protocol.json with export-schema.',
  );
const generated = await compile(JSON.parse(source), 'Protocol', {
  bannerComment: '',
  additionalProperties: false,
  style: { singleQuote: true, semi: true, tabWidth: 2 },
});
if (process.argv.includes('--check')) {
  if ((await readFile(outputPath, 'utf8')) !== generated)
    throw new Error('TypeScript protocol drift. Run npm run bindings:generate.');
} else {
  await mkdir(new URL('../link/typescript/', import.meta.url), { recursive: true });
  await writeFile(outputPath, generated);
}
