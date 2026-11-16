import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { setup } from '../support/harness.mjs';
import { FAKE_PNG_B64 } from '../support/fake-extension.mjs';

describe('openai /v1/images/generations', () => {
  let s;
  before(async () => { s = await setup(); });
  after(async () => { await s?.stop(); });

  it('OpenMausBot custom-provider request returns b64_json and sends the aspect to the extension', async () => {
    const r = await s.post('/v1/images/generations', { model: 'gemini-image', prompt: 'a red apple', response_format: 'b64_json', size: '1536x1024', n: 1 });
    const b = await r.json();
    assert.equal(r.status, 200);
    assert.deepEqual(b.data, [{ b64_json: FAKE_PNG_B64 }]);
    assert.equal(typeof b.created, 'number');
    const cmd = s.ext.received.find((m) => m.type === 'RUN_IMAGE');
    assert.equal(cmd.prompt, 'a red apple');
    assert.equal(cmd.aspect, '16:9');
  });

  it('response_format url returns a data URL', async () => {
    const b = await (await s.post('/v1/images/generations', { prompt: 'cube', response_format: 'url' })).json();
    assert.equal(b.data[0].url, `data:image/png;base64,${FAKE_PNG_B64}`);
  });

  it('errors: 401 without token, 400 for bad input, 502 when Gemini makes no image', async () => {
    const noAuth = await fetch(`${s.base}/v1/images/generations`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: '{"prompt":"x"}' });
    assert.equal(noAuth.status, 401);
    assert.equal((await s.post('/v1/images/generations', { prompt: ' ' })).status, 400);
    assert.equal((await s.post('/v1/images/generations', { prompt: 'x', n: 2 })).status, 400);
    const fail = await s.post('/v1/images/generations', { prompt: '[[FAIL]]' });
    assert.equal(fail.status, 502);
    assert.match((await fail.json()).error.message, /no image/);
  });
});

describe('images without an extension', () => {
  let s;
  before(async () => { s = await setup(false); });
  after(async () => { await s?.stop(); });

  it('returns 503', async () => {
    assert.equal((await s.post('/v1/images/generations', { prompt: 'x' })).status, 503);
  });
});
