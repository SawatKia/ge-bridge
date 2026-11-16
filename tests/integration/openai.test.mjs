import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { setup } from '../support/harness.mjs';
import { REPLY } from '../support/fake-extension.mjs';
import { sse, j, user } from '../support/http.mjs';

describe('openai /v1/chat/completions', () => {
  let s;
  before(async () => { s = await setup(); });
  after(async () => { await s?.stop(); });

  it('non-stream: message content equals the reply', async () => {
    const r = await s.chat({ stream: false });
    const b = await r.json();
    assert.equal(r.status, 200);
    assert.equal(b.choices[0].message.content, REPLY);
  });

  it('stream: deltas concatenate to the reply, reasoning is separate, ends with [DONE]', async () => {
    const r = await s.chat({ stream: true });
    const ev = await sse(r);
    const chunks = ev.map((e) => j(e.data)).filter(Boolean);
    const content = chunks.map((c) => c.choices?.[0]?.delta?.content ?? '').join('');
    const reasoning = chunks.map((c) => c.choices?.[0]?.delta?.reasoning_content ?? '').join('');
    assert.equal(r.status, 200);
    assert.equal(content, REPLY);
    assert.ok(reasoning.length > 0, 'reasoning_content delta must be present');
    assert.ok(!content.includes('Thinking about it'), 'reasoning must not leak into content');
    assert.equal(ev.at(-1).data, '[DONE]');
  });

  it('a failing turn yields a JSON error and the server stays up', async () => {
    const r = await s.chat({ stream: false, messages: [user('boom [[FAIL]]')] });
    const t = await r.text();
    assert.ok(r.status >= 400, `expected error status, got ${r.status}`);
    assert.notEqual(j(t), null, 'error body must be JSON');
    const h = await fetch(`${s.base}/api/status`, { headers: s.auth });
    assert.equal(h.status, 200);
  });
});
