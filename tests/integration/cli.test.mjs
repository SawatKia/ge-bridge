import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { setup } from '../support/harness.mjs';
import { BIN_DIR } from '../support/server.mjs';
import { FAKE_PNG_B64 } from '../support/fake-extension.mjs';

const CLI = path.join(BIN_DIR, 'ge-bridge-cli.exe');

describe('ge-bridge-cli against an isolated server', () => {
  let s;
  const run = (...args) => new Promise((resolve) => {
    execFile(CLI, args, { env: { ...process.env, GE_BRIDGE_PORT: String(s.port), GE_BRIDGE_DATA_DIR: s.dataDir }, timeout: 60000 },
      (err, stdout, stderr) => resolve({ code: err ? err.code : 0, stdout, stderr }));
  });
  before(async () => { s = await setup(); });
  after(async () => { await s?.stop(); });

  it('status, tabs and models read the running server', async () => {
    const st = await run('status');
    assert.equal(st.code, 0, st.stderr);
    assert.match(st.stdout, /extension\s+connected/);
    const tabs = await run('tabs');
    assert.match(tabs.stdout, /#101/);
    const models = await run('models');
    assert.match(models.stdout, /gemini-test-flash/);
  });

  it('chat prints the answer', async () => {
    const r = await run('chat', 'say', 'hi');
    assert.equal(r.code, 0, r.stderr);
    assert.match(r.stdout, /Hello from the fake extension\./);
  });

  it('image saves the decoded file', async () => {
    const out = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'ge-cli-')), 'x.png');
    const r = await run('image', 'a', 'red', 'apple', '--out', out);
    assert.equal(r.code, 0, r.stderr);
    assert.deepEqual(fs.readFileSync(out), Buffer.from(FAKE_PNG_B64, 'base64'));
  });

  it('logs prints readable lines filtered by level', async () => {
    const dir = path.join(s.dataDir, 'logs');
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(path.join(dir, 'extension.log'), [
      '{"ts":1794652015823,"level":"debug","src":"content","tabId":7,"msg":"message received","data":{"type":"PING_TAB"}}',
      '{"ts":1794652052501,"level":"info","src":"bg","tabId":900002,"msg":"RUN_TURN dispatch","data":{"jobId":"job_1","model":"gemini-4-argon"}}',
      '{"ts":1794652053550,"level":"error","src":"bg","msg":"RUN_TURN attempt failed","data":{"httpStatus":401}}',
    ].join('\n') + '\n');
    const info = await run('logs', '--ext');
    assert.equal(info.code, 0, info.stderr);
    assert.match(info.stdout, /^\d\d:\d\d:\d\d\.501 INFO  bg +tab 900002 +RUN_TURN dispatch  jobId=job_1 model=gemini-4-argon$/m);
    assert.doesNotMatch(info.stdout, /PING_TAB/, 'debug hidden by default');
    assert.match((await run('logs', '--ext', '--level', 'debug')).stdout, /DEBUG content tab 7 +message received  type=PING_TAB/);
    const errors = await run('logs', '--ext', '--level', 'error');
    assert.equal(errors.stdout.trim().split('\n').length, 1);
    assert.match(errors.stdout, /ERROR bg +RUN_TURN attempt failed  httpStatus=401/);
    assert.match((await run('logs', '--ext', '--json', '--grep', 'argon')).stdout, /^\{"ts":1794652052501/);
    const bad = await run('logs', '--level', 'loud');
    assert.notEqual(bad.code, 0);
    assert.match(bad.stderr, /unknown level/);
  });

  it('bad usage exits non-zero with a hint', async () => {
    const r = await run('chat');
    assert.notEqual(r.code, 0);
    assert.match(r.stderr, /needs a prompt/);
  });
});
