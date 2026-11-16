import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { setup } from '../support/harness.mjs';
import { waitFor } from '../support/fake-extension.mjs';

const readLines = (f) => (fs.existsSync(f) ? fs.readFileSync(f, 'utf8').split('\n').filter(Boolean) : []);
const appdataLogs = path.join(process.env.APPDATA ?? os.homedir(), 'ge-bridge', 'logs');
const snapshot = () => (fs.existsSync(appdataLogs)
  ? fs.readdirSync(appdataLogs).map((n) => `${n}:${fs.statSync(path.join(appdataLogs, n)).size}`).sort().join(',')
  : null);

describe('file logging', () => {
  let s;
  let before0;
  before(async () => { before0 = snapshot(); s = await setup({}); });
  after(async () => { await s?.stop(); });

  it('writes extension logs to extension.log and server logs to server.log', async () => {
    const t0 = Date.now();
    s.ext.send({
      type: 'EXT_LOG',
      entries: [
        { ts: 1000, level: 'info', src: 'bg', tabId: 101, msg: 'hello', data: { k: 1 } },
        'garbage',
        { ts: 1001, level: 'warn', src: 'content', msg: 'second' },
        { ts: 1002, level: 'error', src: 'popup' },
      ],
    });
    const extLog = path.join(s.dataDir, 'logs', 'extension.log');
    await waitFor(() => readLines(extLog).length >= 2, 'extension.log lines');
    await new Promise((r) => setTimeout(r, 200));
    const lines = readLines(extLog).map((l) => JSON.parse(l));
    assert.equal(lines.length, 2);
    assert.deepEqual([lines[0].msg, lines[0].level, lines[0].src, lines[0].tabId, lines[0].ts], ['hello', 'info', 'bg', 101, 1000]);
    assert.deepEqual(lines[0].data, { k: 1 });
    assert.equal(lines[1].msg, 'second');
    for (const l of lines) assert.ok(l.recv >= t0 - 1000 && l.recv <= Date.now() + 1000, `recv ${l.recv}`);

    const server = readLines(path.join(s.dataDir, 'logs', 'server.log')).map((l) => JSON.parse(l));
    assert.ok(server.length >= 2);
    for (const l of server) assert.ok(l.ts > 1e12 && l.level && l.msg);
    assert.ok(server.some((l) => /starting/.test(l.msg) && l.msg.includes(String(s.port)) && l.msg.includes(s.dataDir) && /tray=disabled/.test(l.msg)), 'startup line');
    assert.ok(server.some((l) => /connected/i.test(l.msg)), 'connected line');
  });

  it('does not touch the real %APPDATA% logs folder', () => {
    assert.equal(snapshot(), before0);
  });
});
