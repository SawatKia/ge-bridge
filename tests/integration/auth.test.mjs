import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { setup } from '../support/harness.mjs';
import { EXT_ORIGIN } from '../support/server.mjs';
import { rawGet, rawPost } from '../support/http.mjs';
import { waitFor } from '../support/fake-extension.mjs';

const FAKE_HOME = 'https://vertexaisearch.cloud.google/us/home/cid/00000000-1111-2222-3333-444444444444?csesidx=1234567890';

describe('auth and origin', () => {
  let s;
  before(async () => { s = await setup(); });
  after(async () => { await s?.stop(); });

  it('health needs no auth and reports expected fields with the extension connected', async () => {
    const h = await fetch(`${s.base}/api/health`);
    const b = await h.json();
    assert.equal(h.status, 200);
    assert.equal(b.ok, true);
    assert.equal(b.name, 'ge-bridge');
    assert.equal(typeof b.version, 'string');
    assert.equal(b.extensionConnected, true);
    assert.equal(b.tabs, 2);
  });

  it('pair: the real extension origin gets a token that works on /v1/models', async () => {
    const ok = await fetch(`${s.base}/api/pair`, { method: 'POST', headers: { origin: EXT_ORIGIN } });
    assert.equal(ok.status, 200);
    const { token } = await ok.json();
    const m = await fetch(`${s.base}/v1/models`, { headers: { authorization: `Bearer ${token}` } });
    assert.equal(m.status, 200);
  });

  it('pair: other extension id, web origin and missing origin are refused without leaking a token', async () => {
    const other = await rawPost(`${s.base}/api/pair`, { origin: 'chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' });
    const evil = await rawPost(`${s.base}/api/pair`, { origin: 'https://evil.example' });
    const none = await rawPost(`${s.base}/api/pair`, {});
    for (const [name, r] of Object.entries({ other, evil, none })) {
      assert.equal(r.status, 403, `${name} must be 403`);
      assert.ok(!r.body.includes('gemb_'), `${name} must not leak a token`);
    }
  });

  it('models: missing or wrong token is 401, x-api-key is accepted', async () => {
    assert.equal((await fetch(`${s.base}/v1/models`)).status, 401);
    assert.equal((await fetch(`${s.base}/v1/models`, { headers: { authorization: 'Bearer gemb_wrong' } })).status, 401);
    assert.equal((await fetch(`${s.base}/v1/models`, { headers: { 'x-api-key': s.token } })).status, 200);
  });

  it('models: valid token lists exactly the available models', async () => {
    const r = await fetch(`${s.base}/v1/models`, { headers: s.auth });
    const ids = (await r.json()).data.map((m) => m.id).sort();
    assert.deepEqual(ids, ['gemini-test-flash', 'gemini-test-pro']);
  });

  it('a web page origin is refused even with a valid token', async () => {
    const st = await rawGet(`${s.base}/v1/models`, { authorization: `Bearer ${s.token}`, origin: 'https://evil.example' });
    assert.equal(st, 403);
  });

  it('websocket with a wrong token never opens', async () => {
    const r = await new Promise((resolve) => {
      const ws = new WebSocket(`ws://127.0.0.1:${s.port}/ws/extension?token=gemb_wrong`);
      let opened = false;
      ws.addEventListener('open', () => { opened = true; });
      ws.addEventListener('error', () => resolve({ opened, how: 'error' }));
      ws.addEventListener('close', (e) => resolve({ opened, how: `close ${e.code}` }));
      setTimeout(() => resolve({ opened, how: 'timeout' }), 4000);
    });
    assert.equal(r.opened, false);
    assert.notEqual(r.how, 'timeout');
  });

  it('GEMINI_TARGET: invalid host or path is ignored, a valid URL is stored', async () => {
    const before = (await s.health()).geminiTargetSet;
    s.ext.send({ type: 'GEMINI_TARGET', homeUrl: 'https://evil.example/us/home/cid/abc?csesidx=1' });
    s.ext.send({ type: 'GEMINI_TARGET', homeUrl: 'https://vertexaisearch.cloud.google.evil.com/us/home/cid/abc' });
    s.ext.send({ type: 'GEMINI_TARGET', homeUrl: 'https://vertexaisearch.cloud.google/us/other/abc' });
    // Barrier: a later message being processed means the earlier invalid ones were too.
    s.ext.send({ type: 'TABS_SYNC', tabs: [] });
    await waitFor(async () => (await s.health()).tabs === 0, 'barrier TABS_SYNC processed');
    assert.equal((await s.health()).geminiTargetSet, before, 'invalid URLs must not be stored');

    s.ext.send({ type: 'GEMINI_TARGET', homeUrl: FAKE_HOME });
    await waitFor(async () => (await s.health()).geminiTargetSet === true, 'valid target stored');
    const st = await (await fetch(`${s.base}/api/status`, { headers: s.auth })).json();
    assert.equal(st.geminiTargetSet, true);
  });
});
