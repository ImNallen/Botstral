import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { protection, isProtected } from '../runtime/protected.mjs';

const root = await fs.mkdtemp(path.join(os.tmpdir(), 'p06-provenance-'));
try {
  const profile = path.join(root, 'profile');
  await fs.mkdir(profile);
  const epoch = crypto.randomUUID();
  const journal = crypto.randomUUID();
  const historyPath = path.join(root, 'automation-protection.json');
  const markerPath = path.join(profile, '.botstral-profile.json');
  const history = { journal, protection: { type: 'tracked', profile: epoch, releases: [], complete: true }, consumed: [] };
  const marker = { profile: epoch, releases: [] };
  await fs.writeFile(historyPath, JSON.stringify(history));
  await fs.writeFile(markerPath, JSON.stringify(marker));
  assert.equal(isProtected(await protection(profile)), false, 'verified empty profile permits ordinary capture');
  const oldHistory = await fs.readFile(historyPath);
  const oldMarker = await fs.readFile(markerPath);
  const release = crypto.randomUUID();
  history.protection.releases.push(release);
  history.consumed.push(release);
  marker.releases.push(release);
  await fs.writeFile(historyPath, JSON.stringify(history));
  await fs.writeFile(markerPath, JSON.stringify(marker));
  await fs.writeFile(path.join(profile, 'Cookies'), 'newer synthetic browser state');
  assert.equal(isProtected(await protection(profile)), true, 'possible release protects the profile');
  await fs.writeFile(historyPath, oldHistory);
  assert.equal(isProtected(await protection(profile)), true, 'metadata rollback cannot clear a newer profile');
  await fs.writeFile(historyPath, JSON.stringify(history));
  await fs.writeFile(markerPath, oldMarker);
  assert.equal(isProtected(await protection(profile)), true, 'incomplete history publication refuses capture');
  await fs.writeFile(markerPath, JSON.stringify(epoch));
  assert.equal(isProtected(await protection(profile)), true, 'legacy provenance cannot establish coverage');
  console.log(JSON.stringify({ emptyProfile: 'clear', possibleRelease: 'protected', metadataRollback: 'protected', partialPublication: 'protected', legacyMarker: 'protected' }));
} finally {
  await fs.rm(root, { recursive: true, force: true });
}
