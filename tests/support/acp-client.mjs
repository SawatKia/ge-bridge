// Minimal ACP client for tests: spawns `ge-bridge-cli acp`, sends requests, records notifications and
// agent-to-client requests, and answers `session/request_permission` through a pluggable handler.
import { spawn } from 'node:child_process';
import readline from 'node:readline';

// permission(params) -> optionId string | 'cancelled' | a never-settling promise (to test cancel while pending)
export function startAcp({ cli, port, dataDir, permission = async () => 'allow_once' }) {
  const child = spawn(cli, ['acp'], {
    env: { ...process.env, GE_BRIDGE_PORT: String(port), GE_BRIDGE_DATA_DIR: dataDir },
    stdio: ['pipe', 'pipe', 'pipe'],
  });
  const pending = new Map();
  const state = { permission };
  const notifications = [];
  const permissionRequests = [];
  const badLines = [];
  let stderr = '';
  let nextId = 1;
  const send = (o) => child.stdin.write(JSON.stringify({ jsonrpc: '2.0', ...o }) + '\n');

  child.stderr.on('data', (d) => { stderr += d; });
  readline.createInterface({ input: child.stdout }).on('line', async (line) => {
    let m;
    try { m = JSON.parse(line); } catch { badLines.push(line); return; }
    if (m.method && m.id !== undefined) {
      if (m.method === 'session/request_permission') {
        permissionRequests.push(m.params);
        const choice = await state.permission(m.params);
        send({ id: m.id, result: { outcome: choice === 'cancelled' ? { outcome: 'cancelled' } : { outcome: 'selected', optionId: choice } } });
      } else {
        send({ id: m.id, error: { code: -32601, message: 'not supported' } });
      }
    } else if (m.method) {
      notifications.push(m);
    } else if (pending.has(m.id)) {
      const { resolve } = pending.get(m.id);
      pending.delete(m.id);
      resolve(m);
    }
  });

  // Resolves with the whole JSON-RPC response ({result} or {error}).
  const request = (method, params = {}, timeoutMs = 30000) => new Promise((resolve, reject) => {
    const id = nextId++;
    const t = setTimeout(() => { pending.delete(id); reject(new Error(`timeout waiting for ${method}\n${stderr}`)); }, timeoutMs);
    pending.set(id, { resolve: (m) => { clearTimeout(t); resolve(m); } });
    send({ id, method, params });
  });

  const updates = (sessionId) => notifications
    .filter((n) => n.method === 'session/update' && (!sessionId || n.params.sessionId === sessionId))
    .map((n) => n.params.update);

  const exited = new Promise((r) => child.on('exit', (code) => r(code)));
  return {
    child, request, notify: (method, params) => send({ method, params }), updates, notifications,
    permissionRequests, badLines, state, exited, stderr: () => stderr,
    close: async () => { child.stdin.end(); const t = setTimeout(() => child.kill(), 5000); const code = await exited; clearTimeout(t); return code; },
  };
}

export const messageText = (updates) => updates
  .filter((u) => u.sessionUpdate === 'agent_message_chunk').map((u) => u.content.text).join('');
