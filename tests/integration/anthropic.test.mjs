import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { setup } from '../support/harness.mjs';
import { REPLY } from '../support/fake-extension.mjs';
import { sse, j } from '../support/http.mjs';

describe('anthropic /v1/messages', () => {
  let s;
  before(async () => { s = await setup(); });
  after(async () => { await s?.stop(); });

  it('non-stream: thinking block then text block equal to the reply', async () => {
    const r = await s.messages({});
    const b = await r.json();
    assert.equal(r.status, 200);
    assert.equal(b.type, 'message');
    assert.equal(b.role, 'assistant');
    assert.equal(b.content[0].type, 'thinking');
    assert.equal(b.content.find((c) => c.type === 'text').text, REPLY);
  });

  it('stream: message_start to message_stop and text equals the reply', async () => {
    const r = await s.messages({ stream: true });
    const ev = (await sse(r)).filter((e) => e.ev);
    const names = ev.map((e) => e.ev);
    const text = ev.filter((e) => e.ev === 'content_block_delta').map((e) => j(e.data)?.delta?.text ?? '').join('');
    assert.equal(r.status, 200);
    assert.equal(names[0], 'message_start');
    assert.equal(names.at(-1), 'message_stop');
    assert.ok(names.includes('content_block_start'));
    assert.equal(text, REPLY);
  });
});
