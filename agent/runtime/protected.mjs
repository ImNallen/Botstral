import fs from 'node:fs/promises';
import net from 'node:net';
import path from 'node:path';
import crypto from 'node:crypto';

export async function protection(profile) {
  try {
    const history = JSON.parse(await fs.readFile(path.join(path.dirname(profile), 'automation-protection.json'), 'utf8'));
    const marker = JSON.parse(await fs.readFile(path.join(profile, '.botstral-profile.json'), 'utf8'));
    const p = history.protection;
    if (p.type !== 'tracked' || p.profile !== marker.profile || !Array.isArray(p.releases)
      || !Array.isArray(marker.releases) || p.releases.length !== marker.releases.length
      || p.releases.some((release, index) => release !== marker.releases[index])
      || typeof p.complete !== 'boolean') return { type: 'unknown' };
    return p;
  } catch { return { type: 'unknown' }; }
}
export function isProtected(p) { return p.type !== 'tracked' || !p.complete || p.releases.length > 0; }
export function channel(specification) {
  const socket = path.join(path.dirname(specification), 'protected.sock');
  return async operation => {
    const request = Buffer.from(JSON.stringify(operation));
    if (request.length > 65536) throw Error('protected request bound');
    try {
      return await new Promise((resolve, reject) => {
        const stream = net.createConnection(socket);
        let size = 0;
        const chunks = [];
        stream.setTimeout(6000, () => stream.destroy(Error('protected deadline')));
        stream.on('connect', () => stream.end(request));
        stream.on('data', chunk => {
          size += chunk.length;
          if (size > 65536) stream.destroy(Error('protected response bound'));
          else chunks.push(chunk);
        });
        stream.on('error', () => reject(Error('protected request unresolved')));
        stream.on('end', () => {
          const bytes = Buffer.concat(chunks);
          try { resolve(JSON.parse(bytes.toString('utf8'))); }
          catch { reject(Error('protected response invalid')); }
          finally { bytes.fill(0); for (const chunk of chunks) chunk.fill(0); }
        });
      });
    } finally { request.fill(0); }
  };
}
export async function safePath(target, profile, runtime) {
  let resolved;
  try { resolved = await fs.realpath(target); }
  catch { resolved = path.join(await fs.realpath(path.dirname(target)), path.basename(target)); }
  for (const root of [await fs.realpath(path.dirname(profile)), await fs.realpath(runtime)]) {
    if (resolved === root || resolved.startsWith(root + path.sep)) throw Error('protected control path');
  }
  return resolved;
}

export async function login(page, op, send, profile) {
  const plan = op.plan;
  if (new URL(plan.origin).origin !== plan.origin || !plan.origin.startsWith('https://') || plan.version !== 1) throw Error('unsupported login');
  const cdp = await page.context().newCDPSession(page);
  // Playwright routes only the first request of a redirect chain. Stop the response itself.
  cdp.on('Fetch.requestPaused', event => {
    const command = event.responseStatusCode >= 300 && event.responseStatusCode < 400
      ? cdp.send('Fetch.failRequest', { requestId: event.requestId, errorReason: 'BlockedByClient' })
      : cdp.send('Fetch.continueRequest', { requestId: event.requestId });
    void command.catch(() => {});
  });
  await cdp.send('Fetch.enable', { patterns: [{ resourceType: 'Document', requestStage: 'Response' }] });
  let submission;
  await page.route('**/*', async route => {
    try {
      const request = route.request();
      if (request.isNavigationRequest() && submission) {
        const pending = submission;
        const fields = [...new URLSearchParams(request.postData() ?? '')];
        const permitted = pending.state === 'pending' && request.frame() === frame && !request.redirectedFrom()
          && request.method() === 'POST' && request.url() === plan.origin + plan.action
          && fields.length === pending.entries.length
          && fields.every(([name, value], i) => name === pending.entries[i][0] && value === pending.entries[i][1]);
        if (!permitted || !await pending.verdict) { await route.abort(); return; }
        submission = { state: 'consumed' };
        await route.continue();
      } else if (request.isNavigationRequest() && (request.redirectedFrom() || request.method() !== 'GET' && (request.method() !== 'POST' || request.url() !== plan.origin + plan.action))) await route.abort();
      else await route.continue();
    } catch { await route.abort().catch(() => {}); }
  });
  await page.goto(plan.origin + plan.route, { waitUntil: 'domcontentloaded', timeout: 15000 });
  if (page.url() !== plan.origin + plan.route) throw Error('login redirect');
  let frame = page.mainFrame();
  if (plan.frame !== null) {
    const frames = await page.locator(plan.frame).elementHandles();
    if (frames.length !== 1) throw Error('ambiguous frame');
    frame = await frames[0].contentFrame();
    if (!frame || new URL(frame.url()).origin !== plan.origin) throw Error('unsupported frame');
  }
  const identities = Array.from({ length: op.purpose === 'password' ? 4 : 3 }, () => crypto.randomUUID());
  const initialize = ({ plan, purpose }) => {
    const one = selector => { const nodes = document.querySelectorAll(selector); if (nodes.length !== 1) throw Error(); return nodes[0]; };
    const form = one(plan.form);
    const submitter = one(plan.submitter);
    const fields = purpose === 'password' ? [one(plan.username), one(plan.password)] : [one(plan.totp)];
    if (!(form instanceof HTMLFormElement) || !(submitter instanceof HTMLButtonElement) || submitter.type !== 'submit'
      || fields.some(f => !(f instanceof HTMLInputElement)) || new Set(fields).size !== fields.length || new Set(fields.map(f => f.name)).size !== fields.length
      || fields[0].type !== 'text' && fields[0].type !== 'email' && fields[0].type !== 'tel'
      || purpose === 'password' && fields[1].type !== 'password') throw Error();
    const elements = [...form.elements];
    if (elements.length !== fields.length + 1 || elements.some(e => e !== submitter && !fields.includes(e))) throw Error();
    return { doc: document, top: window.top.document, form, submitter, fields, elements,
      names: fields.map(f => f.name), types: fields.map(f => f.type), plan,
      certain: form.getAttribute('data-botstral-login') === 'v1' };
  };
  const tree = (await cdp.send('Page.getFrameTree')).frameTree;
  let frameId = tree.frame.id;
  if (plan.frame !== null) {
    const root = (await cdp.send('DOM.getDocument')).root;
    const nodeId = (await cdp.send('DOM.querySelector', { nodeId: root.nodeId, selector: plan.frame })).nodeId;
    frameId = (await cdp.send('DOM.describeNode', { nodeId })).node.frameId;
    if (!frameId) throw Error('unsupported frame identity');
  }
  const { executionContextId } = await cdp.send('Page.createIsolatedWorld', { frameId, worldName: 'botstral-protected-' + crypto.randomUUID() });
  const prepared = await cdp.send('Runtime.evaluate', { expression: '(' + initialize.toString() + ')(' + JSON.stringify({ plan, purpose: op.purpose }) + ')', contextId: executionContextId, returnByValue: false });
  if (prepared.exceptionDetails || !prepared.result.objectId) throw Error('unsupported login form');
  const held = prepared.result.objectId;
  const call = async (fn, value = null) => {
    const result = await cdp.send('Runtime.callFunctionOn', { objectId: held, functionDeclaration: 'function(value) { return (' + fn.toString() + ')(this, value); }', arguments: [{ value }], returnByValue: true });
    if (result.exceptionDetails) throw Error('protected document changed');
    return result.result.value;
  };
  const validate = h => {
    const visible = e => e.isConnected && e.ownerDocument === document && !e.disabled
      && getComputedStyle(e).visibility === 'visible' && getComputedStyle(e).display !== 'none'
      && e.getBoundingClientRect().width > 0 && e.getBoundingClientRect().height > 0;
    return document === h.doc && window.top.document === h.top
      && window.top.location.href === h.plan.origin + h.plan.route
      && location.origin === h.plan.origin && h.form.isConnected && h.form.ownerDocument === document
      && h.form.method === 'post' && h.form.action === h.plan.origin + h.plan.action
      && h.form.enctype === 'application/x-www-form-urlencoded' && (!h.form.target || h.form.target === '_self')
      && h.submitter.form === h.form && h.submitter.type === 'submit' && !h.submitter.name
      && ['formaction', 'formmethod', 'formenctype', 'formtarget'].every(a => !h.submitter.hasAttribute(a))
      && visible(h.submitter) && h.fields.every((f, i) => visible(f) && !f.readOnly && f.form === h.form && f.name === h.names[i] && f.type === h.types[i] && f.name)
      && h.elements.length === h.form.elements.length && h.elements.every((e, i) => e === h.form.elements[i]);
  };
  const valid = () => call(validate);
  if (!await valid()) throw Error('unsupported login form');
  const p = await protection(profile);
  if (p.type !== 'tracked') throw Error('unknown profile');
  const destination = {
    profile: p.profile, browser: crypto.randomUUID(), page: crypto.randomUUID(),
    top_document: crypto.randomUUID(), frame: crypto.randomUUID(), frame_document: crypto.randomUUID(),
    top_origin: plan.origin, frame_origin: plan.origin, plan,
    form: identities[0], fields: (await call(h => h.fields.map((f, i) => ({ name: h.names[i], input_type: h.types[i] })))).map((field, i) => ({ ...field, object: identities[i + 2], role: op.purpose === 'totp' ? 'totp' : i === 0 ? 'username' : 'password' })), submitter: identities[1],
    certain: await call(h => h.certain),
  };
  let response = await send({ type: 'prepare', destination, previous: null });
  if (response.type !== 'review') throw Error('review refused');
  let review = response.review;
  while (review.state === 'awaiting_approval') {
    if (!await valid() || Date.now() >= review.expires_at * 1000) throw Error('review expired or destination changed');
    await new Promise(resolve => setTimeout(resolve, 200));
    response = await send({ type: 'poll', id: review.id });
    if (response.type !== 'review') throw Error('review unavailable');
    review = response.review;
  }
  if (review.state === 'revoked') {
    response = await send({ type: 'prepare', destination, previous: review.id });
    if (response.type !== 'review') throw Error('reprepare refused');
    review = response.review;
  }
  if (review.state !== 'authorized' || !await valid()) throw Error('fill refused');
  response = await send({ type: 'release', id: review.id, destination, protection: p });
  if (response.type !== 'released' || response.release !== review.release || Date.now() >= response.valid_until * 1000) throw Error('release refused');
  const privateValues = response.values;
  let applied = false;
  let settleSubmission;
  try {
    const validation = validate.toString();
    const apply = (h, { values, until }) => {
      const valid = VALIDATION;
      if (Date.now() >= until * 1000 || !valid(h)) throw Error();
      const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
      if (values.type === 'password' && h.fields.length === 2) {
        setter.call(h.fields[0], values.username); setter.call(h.fields[1], values.password);
      } else if (values.type === 'totp' && h.fields.length === 1) setter.call(h.fields[0], values.code);
      else throw Error();
      const expected = values.type === 'password' ? [values.username, values.password] : [values.code];
      if (h.fields.some((field, i) => field.value !== expected[i])) { for (const field of h.fields) setter.call(field, ''); throw Error(); }
      for (const field of h.fields) { field.dispatchEvent(new Event('input', { bubbles: true })); field.dispatchEvent(new Event('change', { bubbles: true })); }
      if (Date.now() >= until * 1000 || !valid(h)) { for (const field of h.fields) setter.call(field, ''); throw Error(); }
      HTMLFormElement.prototype.submit.call(h.form);
      if (!valid(h)) throw Error();
    };
    const navigation = frame.waitForNavigation({ waitUntil: 'domcontentloaded', timeout: 15000 }).then(() => true, () => false);
    const values = privateValues.type === 'password' ? [privateValues.username, privateValues.password] : [privateValues.code];
    const verdict = new Promise(resolve => settleSubmission = resolve);
    submission = { state: 'pending', verdict, entries: destination.fields.map((field, i) => [field.name, values[i]]) };
    const result = await cdp.send('Runtime.callFunctionOn', { objectId: held,
      functionDeclaration: 'function(value) { return (' + apply.toString().replace('VALIDATION', '(' + validation + ')') + ')(this, value); }',
      arguments: [{ value: { values: privateValues, until: response.valid_until } }], returnByValue: true });
    settleSubmission(!result.exceptionDetails);
    if (result.exceptionDetails) throw Error('protected fill refused');
    if (!await navigation) throw Error('protected submission outcome unresolved');
    applied = true;
  } finally {
    settleSubmission?.(false);
    submission = { state: 'consumed' };
    for (const key of ['username', 'password', 'code']) if (key in privateValues) privateValues[key] = '';
    await send({ type: 'settle', id: review.id, applied }).catch(() => {});
    await cdp.send('Runtime.releaseObject', { objectId: held }).catch(() => {});
  }
}
