import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { setup } from '../support/harness.mjs';
import { REPLY, FAKE_PNG_B64 } from '../support/fake-extension.mjs';
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

  it('a url image source is rejected with 400 and nothing is dispatched', async () => {
    const before = s.ext.runTurns.length;
    const r = await s.messages({ stream: false, messages: [{ role: 'user', content: [{ type: 'image', source: { type: 'url', url: 'https://example.com/a.png' } }] }] });
    assert.equal(r.status, 400);
    assert.equal(s.ext.runTurns.length, before);
  });

  it('a tool_result image goes to the extension, not into the prompt', async () => {
    const r = await s.messages({
      stream: false,
      messages: [
        { role: 'user', content: 'screenshot please' },
        { role: 'assistant', content: [{ type: 'tool_use', id: 'toolu_1', name: 'shot', input: {} }] },
        { role: 'user', content: [{ type: 'tool_result', tool_use_id: 'toolu_1', content: [
          { type: 'text', text: 'done' },
          { type: 'image', source: { type: 'base64', media_type: 'image/png', data: FAKE_PNG_B64 } },
        ] }] },
      ],
    });
    assert.equal(r.status, 200);
    const turn = s.ext.runTurns.at(-1);
    assert.equal(turn.images.length, 1);
    assert.ok(!JSON.stringify(turn).replace(turn.images[0], '').includes(FAKE_PNG_B64));
  });
});
