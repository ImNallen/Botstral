import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { StreamableHTTPServerTransport } from '@modelcontextprotocol/sdk/server/streamableHttp.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { z } from 'zod';
import fs from 'node:fs';
import path from 'node:path';
import https from 'node:https';
import { spawn } from 'node:child_process';

const root = fs.realpathSync(process.argv[3]);
function server() {
  const mcp = new McpServer({ name: 'm06-filesystem', version: '1.0.0' });
  mcp.registerTool('write_receipt', {
    description: 'Write a bounded UTF-8 receipt in the selected fixture directory.',
    inputSchema: { name: z.string().regex(/^[a-z0-9-]+\.txt$/), text: z.string().max(512), delay: z.boolean().optional() },
  }, async ({ name, text, delay }) => {
    fs.writeFileSync(path.join(root, name), text, { mode: 0o600 });
    const child = delay ? spawn('/usr/bin/sleep', ['120'], { stdio: 'ignore' }) : null;
    const event = { name, text, pid: process.pid, parent: process.ppid,
      cgroup: fs.readFileSync('/proc/self/cgroup', 'utf8'), descendant: child?.pid ?? null };
    fs.appendFileSync(path.join(root, 'effects.jsonl'), JSON.stringify(event) + '\n', { mode: 0o600 });
    if (delay) await new Promise(resolve => setTimeout(resolve, 120000));
    return { content: [{ type: 'text', text: fs.readFileSync(path.join(root, name), 'utf8') }] };
  });
  return mcp;
}
if (process.argv[2] === 'stdio') {
  await server().connect(new StdioServerTransport());
} else {
  const secret = fs.readFileSync(path.join(root, 'bearer'), 'utf8');
  const sessions = new Map();
  const listener = https.createServer({ key: fs.readFileSync(path.join(root, 'manager.key')),
    cert: fs.readFileSync(path.join(root, 'manager.pem')) }, async (request, response) => {
    if (request.headers.authorization !== 'Bearer ' + secret) {
      fs.appendFileSync(path.join(root, 'auth-rejected'), 'rejected\n');
      response.writeHead(401).end(); return;
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
  listener.listen(0, '0.0.0.0', () => fs.writeFileSync(path.join(root, 'https.json'), JSON.stringify({ port: listener.address().port })));
}
