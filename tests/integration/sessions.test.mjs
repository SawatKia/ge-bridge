import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { setup } from '../support/harness.mjs';
import { waitFor } from '../support/fake-extension.mjs';
import { user } from '../support/http.mjs';

describe('sessions and tabs', () => {
  let s;
  let release;
  const held = new Promise((r) => { release = r; });
  before(async () => {
    s = await setup({ gate: (m) => (m.prompt.includes('hold-me') ? held : undefined) });
  });
  after(async () => { release(); await s?.stop(); });

  it('multi-turn continuation sends only the new message with newChat=false', async () => {
    const m1 = [user('first question about cats')];
    const r1 = await s.post('/v1/chat/completions', { model: 'gemini-test-flash', messages: m1 });
    const reply = (await r1.json()).choices[0].message.content;
    const r2 = await s.post('/v1/chat/completions', {
      model: 'gemini-test-flash',
      messages: [...m1, { role: 'assistant', content: reply }, user('second question about dogs')],
    });
    await r2.json();
    const [t1, t2] = s.ext.runTurns;
    assert.equal(t1.newChat, true);
    assert.equal(t2.newChat, false, 'continuation must reuse the chat');
    assert.equal(t2.tabId, t1.tabId, 'continuation must stay on the same tab');
    assert.ok(t2.prompt.includes('second question about dogs'));
    assert.ok(!t2.prompt.includes('first question about cats'), 'old turns must not be resent');
  });

  it('two concurrent requests are dispatched to different tabs while the first is still open', async () => {
    const before = s.ext.runTurns.length;
    const first = s.post('/v1/chat/completions', { model: 'gemini-test-flash', messages: [user('concurrent A hold-me')] });
    await waitFor(() => s.ext.runTurns.length === before + 1, 'first RUN_TURN');
    const second = s.post('/v1/chat/completions', { model: 'gemini-test-flash', messages: [user('concurrent B unique-b')] });
    await waitFor(() => s.ext.runTurns.length === before + 2, 'second RUN_TURN while first is held');
    release();
    const rs = await Promise.all([first, second]);
    await Promise.all(rs.map((r) => r.text()));
    assert.deepEqual(rs.map((r) => r.status), [200, 200]);
    const tabs = s.ext.runTurns.slice(before).map((t) => t.tabId).sort();
    assert.deepEqual(tabs, [101, 102]);
  });
});
