import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { StreamableHTTPServerTransport } from '@modelcontextprotocol/sdk/server/streamableHttp.js';
import { z } from 'zod';
import fs from 'node:fs';
import path from 'node:path';
import https from 'node:https';

const root = fs.realpathSync(process.argv[2]);
const wait = async name => {
  while (!fs.existsSync(path.join(root, name))) {
    await new Promise(resolve => setTimeout(resolve, 50));
  }
};
const server = () => {
  const mcp = new McpServer({ name: 'first-acceptance-receipts', version: '1.0.0' });
  mcp.registerTool('write_receipt', {
    description: 'Write one bounded receipt to the owner-selected acceptance directory.',
    inputSchema: { name: z.string().regex(/^[a-z0-9-]+\.txt$/), text: z.string().max(512) },
  }, async ({ name, text }) => {
    await wait(name + '.release');
    fs.writeFileSync(path.join(root, name), text, { mode: 0o600 });
    const descriptor = fs.openSync(path.join(root, 'effects.jsonl'), 'a', 0o600);
    try {
      fs.writeSync(descriptor, JSON.stringify({ name, text, pid: process.pid }) + '\n');
      fs.fsyncSync(descriptor);
    } finally {
      fs.closeSync(descriptor);
    }
    await wait(name + '.return');
    return { content: [{ type: 'text', text }] };
  });
  return mcp;
};
const bearer = fs.readFileSync(path.join(root, 'bearer'), 'utf8');
const sessions = new Map();
const listener = https.createServer({ key: fs.readFileSync(path.join(root, 'manager.key')),
  cert: fs.readFileSync(path.join(root, 'manager.pem')) }, async (request, response) => {
  if (request.headers.authorization !== 'Bearer ' + bearer) {
    response.writeHead(401).end();
    return;
  }
  try {
    const id = request.headers['mcp-session-id'];
    let transport = id ? sessions.get(id) : null;
    if (!transport) {
      transport = new StreamableHTTPServerTransport({ sessionIdGenerator: () => crypto.randomUUID(),
        enableJsonResponse: true, onsessioninitialized: sid => sessions.set(sid, transport) });
      await server().connect(transport);
    }
    await transport.handleRequest(request, response);
  } catch {
    if (!response.headersSent) response.writeHead(500).end();
  }
});
const savedAddress = path.join(root, 'https.json');
const port = fs.existsSync(savedAddress) ? JSON.parse(fs.readFileSync(savedAddress, 'utf8')).port : 0;
listener.listen(port, '0.0.0.0', () => fs.writeFileSync(path.join(root, 'https.json'),
  JSON.stringify({ port: listener.address().port }), { mode: 0o600 }));
