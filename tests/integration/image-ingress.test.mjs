import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { setup } from '../support/harness.mjs';

const dataUrl = (mime, n, ch = 'A') => `data:${mime};base64,${ch.repeat(n)}`;
const withImage = (text, url) => ({ role: 'user', content: [{ type: 'text', text }, { type: 'image_url', image_url: { url } }] });
const MB8 = 8 * 1024 * 1024 * 4 / 3 | 0;

describe('image ingress limits', () => {
  let s;
  before(async () => { s = await setup(); });
  after(async () => { await s?.stop(); });

  it('a ~50MB multi-turn history is not rejected with 413 and the current-turn image reaches RUN_TURN only', async () => {
    const hist = ['B', 'C', 'D', 'E', 'F', 'G'].map((c) => dataUrl('image/png', MB8, c));
    const current = dataUrl('image/png', 16, 'Z');
    const messages = [];
    hist.forEach((u, i) => {
      messages.push(withImage(`old ${i}`, u));
      messages.push({ role: 'assistant', content: `seen ${i}` });
    });
    messages.push(withImage('now this one', current));
    const before = s.ext.runTurns.length;
    const r = await s.chat({ stream: false, messages });
    assert.equal(r.status, 200, await r.clone().text().then((t) => t.slice(0, 200)));
    assert.equal(s.ext.runTurns.length, before + 1);
    assert.deepEqual(s.ext.runTurns.at(-1).images, [current]);
  });

  it('an image over 10MB decoded is rejected with 400 and nothing is dispatched', async () => {
    const before = s.ext.runTurns.length;
    const r = await s.chat({ stream: false, messages: [withImage('big', dataUrl('image/png', 13_981_015))] });
    assert.equal(r.status, 400);
    assert.match((await r.json()).error.message, /too large/);
    assert.equal(s.ext.runTurns.length, before);
  });

  it('a disallowed MIME type is rejected with 400', async () => {
    const r = await s.chat({ stream: false, messages: [withImage('svg', dataUrl('image/svg+xml', 16))] });
    assert.equal(r.status, 400);
    assert.match((await r.json()).error.message, /image\/svg\+xml/);
  });
});
