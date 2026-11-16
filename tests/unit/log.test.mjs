import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
import { fileURLToPath } from 'node:url';
import { newContext } from './load-extension-lib.mjs';

const LOG_PATH = fileURLToPath(new URL('../../extension/lib/log.js', import.meta.url));

function loadLog(ctx = newContext({ console, setTimeout, setInterval, clearTimeout })) {
  vm.runInContext(fs.readFileSync(LOG_PATH, 'utf8'), ctx, { filename: LOG_PATH });
  return vm.runInContext('GeBridgeLog', ctx);
}

const L = loadLog();
// Objects built inside the vm context have a different prototype; compare through JSON.
const plain = (x) => JSON.parse(JSON.stringify(x));

const UUID = '59c0a1b2-1111-2222-3333-444455556666';
const JWT = 'eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abcDEF123_-xyz';

test('redactString table', () => {
  const rows = [
    ['token gemb_AbC123_-xyz here', (o) => !o.includes('AbC123') && o.includes('gemb_[redacted]')],
    ['Authorization: Bearer abcdef123456', (o) => !o.includes('abcdef123456')],
    ['{"authorization":"Basic Zm9vOmJhcg=="}', (o) => !o.includes('Zm9v')],
    ['xsrfToken=SECRETVALUE99', (o) => !o.includes('SECRETVALUE99')],
    [`jwt ${JWT} end`, (o) => !o.includes('eyJhbGci') && o.includes('[jwt]')],
    ['Cookie: SID=aaa; HSID=bbb', (o) => !o.includes('aaa') && !o.includes('bbb')],
    ['projects/p/sessions/9876543210abcdef failed', (o) => !o.includes('9876543210abcdef')],
    [`path /home/cid/${UUID} x`, (o) => o.includes('/cid/59c0…') && !o.includes('a1b2')],
    ['csesidx=1234567890 only', (o) => o.includes('csesidx=123…') && !o.includes('1234567')]
  ];
  for (const [input, ok] of rows) {
    const out = L.redactString(input);
    assert.ok(ok(out), `${input} -> ${out}`);
  }
});

test('redactUrl masks cid/csesidx and drops query values', () => {
  const u = `https://vertexaisearch.cloud.google/us/home/cid/${UUID}?csesidx=1234567890&authuser=0&q=secret#frag`;
  const out = L.redactString(`opened ${u}.`);
  assert.equal(out, 'opened https://vertexaisearch.cloud.google/us/home/cid/59c0…?csesidx=123…&authuser&q.');
  assert.equal(L.redactUrl('https://x.test/a'), 'https://x.test/a');
});

test('redact is recursive and key-aware', () => {
  const out = plain(L.redact({
    token: 'gemb_abc',
    headers: { Authorization: 'Bearer zzzzzzzzzz', ok: 'fine' },
    xsrfToken: 'x1',
    cookie: 'a=b',
    sessionName: 'projects/1/sessions/2',
    cid: UUID,
    csesidx: 1234567890,
    hasToken: true,
    nested: { list: [{ url: `https://h.test/cid/${UUID}?csesidx=999888` }, JWT] },
    err: new Error(`boom gemb_leak123`)
  }));
  assert.equal(out.token, '[redacted]');
  assert.equal(out.headers.Authorization, '[redacted]');
  assert.equal(out.headers.ok, 'fine');
  assert.equal(out.xsrfToken, '[redacted]');
  assert.equal(out.cookie, '[redacted]');
  assert.equal(out.sessionName, '[redacted]');
  assert.equal(out.cid, '59c0…');
  assert.equal(out.csesidx, '123…');
  assert.equal(out.hasToken, true);
  assert.equal(out.nested.list[0].url, 'https://h.test/cid/59c0…?csesidx=999…');
  assert.equal(out.nested.list[1], '[jwt]');
  assert.ok(!out.err.message.includes('leak123'));
  assert.ok(!JSON.stringify(out).includes(UUID));
});

test('redact is idempotent', () => {
  const once = L.redactString(`https://a.test/cid/${UUID}?csesidx=1234567`);
  assert.equal(L.redactString(once), once);
});

test('makeEntry shape and caps', () => {
  const e = plain(L.makeEntry({ level: 'warn', src: 'content', msg: 'm', data: { tabId: 7, a: 1 }, ts: 1000 }));
  assert.deepEqual(e, { ts: 1000, level: 'warn', src: 'content', tabId: 7, msg: 'm', data: { tabId: 7, a: 1 } });
  const bare = plain(L.makeEntry({ level: 'bogus', src: 'x', msg: 'hi' }));
  assert.equal(bare.level, 'info');
  assert.equal(bare.src, 'bg');
  assert.ok(Number.isInteger(bare.ts));
  assert.ok(!('tabId' in bare) && !('data' in bare));
  assert.equal(L.makeEntry({ msg: 'x'.repeat(2000) }).msg.length, 500);
  const big = L.makeEntry({ msg: 'm', data: { blob: 'y'.repeat(10000) } });
  assert.equal(big.data.truncated, true);
  assert.ok(JSON.stringify(big.data).length <= 4096);
  assert.equal(L.makeEntry({ msg: 'k', tabId: 3 }).tabId, 3);
});

test('ring buffer keeps only the newest entries', () => {
  const r = L.createRing(3);
  for (let i = 1; i <= 5; i++) r.push(i);
  assert.deepEqual(plain(r.toArray()), [3, 4, 5]);
  assert.equal(r.size, 3);
});

test('bg logger buffers, caps, and flushes EXT_LOG frames', () => {
  const lg = L.createLogger({ mode: 'bg', src: 'bg', capacity: 600 });
  for (let i = 0; i < 700; i++) lg.info(`m${i}`);
  assert.equal(lg.count(), 600);
  const frames = [];
  assert.equal(lg.flush(), 0, 'no sender yet');
  lg.attachSender((f) => { frames.push(f); return true; });
  assert.equal(lg.flush(), 600);
  assert.equal(frames.length, 2);
  assert.equal(frames[0].type, 'EXT_LOG');
  assert.equal(frames[0].entries.length, 500);
  assert.equal(frames[1].entries.length, 100);
  assert.equal(lg.unsentCount(), 0);
  const e = plain(frames[0].entries[0]);
  assert.deepEqual(Object.keys(e).sort(), ['level', 'msg', 'src', 'ts']);
});

test('flush keeps entries when sender reports closed socket', () => {
  const lg = L.createLogger({ mode: 'bg', capacity: 100 });
  lg.attachSender(() => false);
  lg.error('x');
  assert.equal(lg.flush(), 0);
  assert.equal(lg.unsentCount(), 1);
  assert.equal(plain(lg.lastError()).msg, 'x');
});

test('ingest sanitizes entries from other contexts and sets tabId', () => {
  const lg = L.createLogger({ mode: 'bg' });
  lg.ingest({ ts: 5, level: 'info', src: 'evil', msg: 'got gemb_secret1', data: { token: 'a' } }, 42);
  const [e] = plain(lg.entries());
  assert.equal(e.src, 'content');
  assert.equal(e.tabId, 42);
  assert.equal(e.msg, 'got gemb_[redacted]');
  assert.equal(e.data.token, '[redacted]');
});

test('forward logger sends entries and retries after failure', async () => {
  const got = [];
  let fail = true;
  const lg = L.createLogger({
    mode: 'forward', src: 'popup',
    forward: async (entry) => { if (fail) throw new Error('asleep'); got.push(entry); }
  });
  lg.info('one');
  await new Promise((r) => setTimeout(r, 10));
  assert.equal(got.length, 0);
  fail = false;
  lg.info('two');
  await new Promise((r) => setTimeout(r, 10));
  assert.deepEqual(plain(got.map((e) => e.msg)), ['one', 'two']);
  assert.equal(got[0].src, 'popup');
});

test('loading the lib twice keeps the same instance', () => {
  const ctx = newContext({ console, setTimeout, setInterval, clearTimeout });
  const a = loadLog(ctx);
  a.__marker = 1;
  const b = loadLog(ctx);
  assert.equal(b.__marker, 1);
});
