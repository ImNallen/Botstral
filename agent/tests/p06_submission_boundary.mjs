import assert from 'node:assert/strict';
import http from 'node:http';
import { chromium } from '../runtime/node_modules/playwright/index.mjs';
// Premise under test: the original synchronous DOM call can finish while its POST is paused.
const server = http.createServer((request, response) => {
  if (request.method === 'POST') throw Error('unapproved submission escaped');
  response.setHeader('content-type', 'text/html');
  response.end('<form method="post" action="/session"><input name="field" value="public fixture"><script>document.querySelector("form").addEventListener("formdata",()=>document.querySelector("form").method="get")</script></form>');
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
const browser = await chromium.launch({ headless: true });
let resolveDecision;
const decision = new Promise(resolve => resolveDecision = resolve);
const events = [];
try {
  const page = await browser.newPage();
  await page.goto(`http://127.0.0.1:${server.address().port}`);
  const cdp = await page.context().newCDPSession(page);
  cdp.on('Fetch.requestPaused', event => { void cdp.send('Fetch.continueRequest', { requestId: event.requestId }).catch(() => {}); });
  await cdp.send('Fetch.enable', { patterns: [{ resourceType: 'Document', requestStage: 'Response' }] });
  await page.route('**/*', async route => {
    events.push('request_paused');
    await decision;
    await route.abort();
    events.push('request_refused');
  });
  const completed = await Promise.race([
    cdp.send('Runtime.evaluate', { expression: 'document.querySelector("form").submit(); document.querySelector("form").method', returnByValue: true }),
    new Promise(resolve => setTimeout(() => resolve(null), 2000)),
  ]);
  assert.ok(completed, 'original DOM call did not finish while navigation was paused');
  if (completed.exceptionDetails) throw Error(completed.exceptionDetails.exception?.description ?? 'fixture DOM failure');
  assert.equal(completed.result.value, 'get');
  events.push('original_dom_call_completed_with_changed_method');
  resolveDecision(false);
  await page.waitForTimeout(100);
  assert.ok(events.includes('request_refused'));
  console.log(JSON.stringify({ passed: true, events, network_submissions: 0 }));
} finally {
  resolveDecision(false);
  await browser.close();
  await new Promise(resolve => server.close(resolve));
}
