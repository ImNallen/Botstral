import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';
import net from 'node:net';
import https from 'node:https';
import crypto from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { chromium } from '../../agent/runtime/node_modules/playwright/index.mjs';
import { verifyReviewUi } from '../../desktop/fixtures/p06-review.mjs';
import { verifyRuntime } from './p06_runtime.mjs';
import { login, protection, isProtected, safePath } from '../../agent/runtime/protected.mjs';

const [root, evidence] = process.argv.slice(2);
await fs.mkdir(evidence, { recursive: true });
const results = [];
async function manager(message) {
  return await new Promise((resolve, reject) => {
    const socket = net.createConnection(path.join(root, 'manager.sock'));
    const chunks = [];
    socket.on('connect', () => socket.end(JSON.stringify(message)));
    socket.on('data', chunk => chunks.push(chunk));
    socket.on('error', reject);
    socket.on('end', () => {
      const bytes = Buffer.concat(chunks);
      try { resolve(JSON.parse(bytes.toString())); } catch { reject(Error('fixture response')); }
      finally { bytes.fill(0); for (const chunk of chunks) chunk.fill(0); }
    });
  });
}
execFileSync('openssl', ['req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-keyout', path.join(root, 'key.pem'), '-out', path.join(root, 'cert.pem'), '-days', '1', '-subj', '/CN=localhost'], { stdio: 'ignore' });
let mode = 'password';
let behavior = '';
let submitted = 0;
let valid = 0;
let lastBody = '';
let wrongDestination = 0;
const server = https.createServer({ key: await fs.readFile(path.join(root, 'key.pem')), cert: await fs.readFile(path.join(root, 'cert.pem')) }, async (request, response) => {
  if (request.url.startsWith('/wrong') || request.url.startsWith('/session?')) wrongDestination++;
  if (behavior === 'redirect' && request.url === '/login') { response.writeHead(302, { location: '/wrong' }); response.end(); return; }
  if (request.method === 'POST') {
    const chunks = [];
    for await (const chunk of request) chunks.push(chunk);
    const bytes = Buffer.concat(chunks);
    lastBody = bytes.toString(); bytes.fill(0); for (const chunk of chunks) chunk.fill(0);
    submitted++;
    if ((await manager({ type: 'submission', body: lastBody })).valid) valid++;
    if (behavior === 'post_redirect') { response.writeHead(307, { location: '/wrong' }); response.end(); return; }
    response.setHeader('set-cookie', 'signed-in=yes; Secure; HttpOnly; SameSite=Strict');
    if (behavior === 'echo') {
      const value = new URLSearchParams(lastBody).get('password');
      response.end('<body><div id="echo"></div><canvas></canvas><script>const value=' + JSON.stringify(value).replaceAll('<', '\\u003c') + ';document.querySelector("#echo").textContent=value;document.cookie="echo="+encodeURIComponent(value);localStorage.setItem("echo",value);history.replaceState(null,"", "#"+encodeURIComponent(value));document.querySelector("canvas").getContext("2d").fillText(value,10,10)</script></body>');
    } else response.end('<body>Signed in</body>');
    return;
  }
  const fields = mode === 'password'
    ? '<input id="username" name="username" type="text"><input id="password" name="password" type="password">'
    : '<input id="totp" name="totp" type="text">';
  let body = `<form id="login" method="post" action="/session" data-botstral-login="v1">${fields}<button id="submit" type="submit">Sign in</button></form>`;
  if (behavior === 'event_action') body += '<script>document.querySelector("input").addEventListener("input",()=>document.querySelector("form").action="/changed")</script>';
  if (behavior === 'formdata_get') body += '<script>document.querySelector("form").addEventListener("formdata",()=>document.querySelector("form").method="get")</script>';
  if (behavior === 'formdata_payload') body += '<script>document.querySelector("form").addEventListener("formdata",e=>e.formData.append("extra","unapproved"))</script>';
  if (behavior === 'prototype') body += '<script>window.eval=()=>()=>true;HTMLFormElement.prototype.submit=function(){fetch("/wrong",{method:"POST",body:"tampered"})}</script>';
  if (behavior === 'frame' && request.url === '/login') body = '<iframe id="frame" src="/frame"></iframe>';
  response.end('<!doctype html><body>' + body + '</body>');
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
const origin = `https://127.0.0.1:${server.address().port}`;
let context;
try {
  for (const scenario of ['approved', 'remember', 'totp', 'redirect', 'post_redirect', 'echo', 'field_replace', 'form_replace', 'top_reload', 'action', 'method', 'hidden', 'submit_override', 'event_action', 'formdata_get', 'formdata_payload', 'prototype', 'frame', 'frame_replace', 'after_release_field', 'deny', 'dropped_release']) {
    mode = scenario === 'totp' ? 'totp' : 'password';
    behavior = ['event_action', 'formdata_get', 'formdata_payload', 'prototype', 'frame', 'redirect', 'post_redirect', 'echo'].includes(scenario) ? scenario : scenario === 'frame_replace' ? 'frame' : '';
    submitted = 0; valid = 0; lastBody = ''; wrongDestination = 0;
    while (scenario === 'totp' && Math.floor(Date.now() / 1000) % 30 >= 24) await new Promise(resolve => setTimeout(resolve, 1000));
    const caseRoot = path.join(root, scenario);
    const profile = path.join(caseRoot, 'profile');
    await fs.mkdir(profile, { recursive: true, mode: 0o700 });
    const profileId = crypto.randomUUID();
    let history = { protection: { type: 'tracked', profile: profileId, releases: [], complete: true } };
    await fs.writeFile(path.join(profile, '.botstral-profile.json'), JSON.stringify(profileId));
    await fs.writeFile(path.join(caseRoot, 'automation-protection.json'), JSON.stringify(history));
    context = await chromium.launchPersistentContext(profile, { executablePath: process.env.BOTSTRAL_CHROME ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true, ignoreHTTPSErrors: true, serviceWorkers: 'block', acceptDownloads: false });
    const page = context.pages()[0];
    const plan = { version: 1, origin, route: '/login', frame: behavior === 'frame' ? '#frame' : null, form: '#login', username: '#username', password: '#password', totp: '#totp', submitter: '#submit', action: '/session' };
    const { operation } = await manager({ type: 'start', plan, purpose: mode, policy: scenario === 'remember' ? 'remembered_origins' : 'every_fill' });
    let review;
    let settlement;
    const send = async operation => {
      if (operation.type === 'release') {
        history.protection.releases.push(review.release);
        await fs.writeFile(path.join(caseRoot, 'automation-protection.json'), JSON.stringify(history));
        operation.protection = history.protection;
      }
      let reply = await manager({ type: 'operation', operation });
      if (operation.type === 'prepare' && reply.type === 'review') {
        review = reply.review;
        if (!operation.previous) {
          if (scenario === 'field_replace') await page.locator('#password').evaluate(e => e.replaceWith(e.cloneNode()));
          if (scenario === 'form_replace') await page.locator('#login').evaluate(e => e.replaceWith(e.cloneNode(true)));
          if (scenario === 'top_reload') await page.reload();
          if (scenario === 'action') await page.locator('#login').evaluate(e => e.action = '/changed');
          if (scenario === 'method') await page.locator('#login').evaluate(e => e.method = 'get');
          if (scenario === 'hidden') await page.locator('#password').evaluate(e => e.style.display = 'none');
          if (scenario === 'submit_override') await page.locator('#submit').evaluate(e => e.setAttribute('formaction', '/changed'));
          if (scenario === 'frame_replace') await page.locator('#frame').evaluate(e => e.replaceWith(e.cloneNode()));
          reply = scenario === 'approved' ? await verifyReviewUi(root, review, (review, decision) => manager({ type: 'review_decision', review, decision }), evidence) : await manager({ type: 'decision', id: review.id, decision: scenario === 'deny' ? 'deny' : scenario === 'remember' ? 'remember_password' : 'allow_once' });
          if (reply.type === 'review') review = reply.review;
        }
      }
      if (operation.type === 'release' && scenario === 'after_release_field') await page.locator('#password').evaluate(e => e.replaceWith(e.cloneNode()));
      if (operation.type === 'release' && scenario === 'dropped_release') {
        assert.equal(reply.type, 'released');
        assert.equal((await manager({ type: 'operation', operation })).type, 'refused');
        throw Error('simulated lost response');
      }
      if (operation.type === 'settle') settlement = reply.review?.state;
      return reply;
    };
    let completed = true;
    try { await login(page, operation, send, profile); } catch { completed = false; }
    await new Promise(resolve => setTimeout(resolve, 250));
    const expected = ['approved', 'remember', 'totp', 'post_redirect', 'echo', 'prototype', 'frame'].includes(scenario);
    assert.equal(completed, expected && scenario !== 'post_redirect', `${scenario} completion`);
    assert.equal(submitted, expected ? 1 : 0, `${scenario} submission count`);
    assert.equal(valid, expected ? 1 : 0, `${scenario} server authentication`);
    if (expected && scenario !== 'post_redirect') assert.equal(settlement, 'applied', `${scenario} settlement`);
    if (['post_redirect', 'formdata_get', 'formdata_payload', 'event_action', 'after_release_field'].includes(scenario)) assert.equal(settlement, 'uncertain', `${scenario} uncertain settlement`);
    assert.equal(wrongDestination, 0, `${scenario} redirect reached an unapproved destination`);
    if (scenario === 'echo') assert.equal((await send({ type: 'screen', protection: history.protection, text: await page.locator('body').innerText() })).type, 'refused');
    if (expected) {
      const text = new URLSearchParams(lastBody).values().next().value;
      assert.equal((await send({ type: 'screen', protection: history.protection, text })).type, 'refused');
      for (const value of new URLSearchParams(lastBody).values()) for (const encoded of [encodeURIComponent(value), Buffer.from(value).toString('base64'), JSON.stringify(value), JSON.stringify(JSON.stringify(value))]) assert.equal((await send({ type: 'screen', protection: history.protection, text: encoded })).type, 'refused');
      assert.equal((await send({ type: 'screen', protection: history.protection, text: 'Safe account status' })).type, 'screened');
      assert.equal(isProtected(await protection(profile)), true);
      assert.equal((await send({ type: 'screen', protection: { ...history.protection, releases: [crypto.randomUUID()] }, text: 'Safe text with missing inventory' })).type, 'refused');
      await assert.rejects(safePath(path.join(profile, 'Cookies'), profile, path.resolve('agent/runtime')));
    }
    await context.close(); context = null;
    const runtime = scenario === 'echo' ? await verifyRuntime(caseRoot, profile, send, lastBody) : undefined;
    lastBody = '';
    results.push({ scenario, passed: true, submissions: submitted, authenticated: valid, runtime });
    await fs.writeFile(path.join(evidence, 'browser.json'), JSON.stringify({ boundary: 'Real Chromium and production protected runtime module against a controlled HTTPS form. Real manager state admission, owner decisions, releases and settlement through a private fixture socket. Linux supervisor, worker TLS and packaged native interaction are not proved by this fixture.', results }, null, 2) + '\n');
  }
  console.log(`P06 controlled browser acceptance passed ${results.length} cases.`);
} finally {
  await context?.close();
  await new Promise(resolve => server.close(resolve));
}
