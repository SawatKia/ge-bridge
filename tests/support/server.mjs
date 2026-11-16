import { spawn } from 'node:child_process';
import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
// GE_BRIDGE_TEST_BIN_DIR points at another build, e.g. while the release exe is running and locked.
export const BIN_DIR = process.env.GE_BRIDGE_TEST_BIN_DIR || path.join(ROOT, 'target', 'release');
const EXE = path.join(BIN_DIR, 'ge-bridge.exe');
export const EXT_ORIGIN = 'chrome-extension://lkbfapcoeagmekmncfbdgiefneohalej';

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function freePort() {
  return new Promise((resolve, reject) => {
    const srv = net.createServer();
    srv.on('error', reject);
    srv.listen(0, '127.0.0.1', () => {
      const { port } = srv.address();
      srv.close(() => resolve(port));
    });
  });
}

export async function startServer() {
  if (!fs.existsSync(EXE)) throw new Error(`${EXE} not found: run "cargo build --release" first`);
  const port = await freePort();
  const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'ge-bridge-test-'));
  const base = `http://127.0.0.1:${port}`;
  let stderr = '';
  let exited = false;
  const child = spawn(EXE, [], {
    env: { ...process.env, GE_BRIDGE_PORT: String(port), GE_BRIDGE_DATA_DIR: dataDir, GE_BRIDGE_NO_TRAY: '1' },
    stdio: ['ignore', 'ignore', 'pipe'],
    windowsHide: true,
  });
  child.stderr.on('data', (c) => { stderr += c; });
  const exit = new Promise((r) => child.once('exit', () => { exited = true; r(); }));

  const stop = async () => {
    if (!exited) {
      child.kill();
      await Promise.race([exit, sleep(5000)]);
    }
    fs.rmSync(dataDir, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
  };

  const deadline = Date.now() + 10000;
  for (;;) {
    if (exited) { await stop(); throw new Error(`ge-bridge exited during startup. stderr:\n${stderr}`); }
    try {
      if ((await fetch(`${base}/api/health`)).status === 200) break;
    } catch {}
    if (Date.now() > deadline) { await stop(); throw new Error(`ge-bridge not healthy on ${base} after 10s. stderr:\n${stderr}`); }
    await sleep(100);
  }

  const r = await fetch(`${base}/api/pair`, { method: 'POST', headers: { origin: EXT_ORIGIN } });
  if (!r.ok) { await stop(); throw new Error(`/api/pair -> ${r.status}`); }
  const { token } = await r.json();
  return { base, port, dataDir, token, stop };
}
