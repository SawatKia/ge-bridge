import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { setup } from '../support/harness.mjs';
import { sse, j, user, oaiTools, anthTools } from '../support/http.mjs';

const ask = [user('weather? [[TOOL]]')];

describe('tool calls', () => {
  let s;
  before(async () => { s = await setup(); });
  after(async () => { await s?.stop(); });

  it('openai non-stream: tool_calls parsed, finish_reason tool_calls, no raw tag', async () => {
    const r = await s.chat({ stream: false, tools: oaiTools, messages: ask });
    const ch = (await r.json()).choices[0];
    const tc = ch.message.tool_calls[0];
    assert.equal(r.status, 200);
    assert.equal(tc.function.name, 'get_weather');
    assert.equal(j(tc.function.arguments).city, 'Bangkok');
    assert.equal(ch.finish_reason, 'tool_calls');
    assert.ok(!(ch.message.content ?? '').includes('<tool_call>'), 'raw tag must be stripped from content');
  });

  it('openai stream: tool_calls parsed, finish_reason tool_calls, no raw tag', async () => {
    const r = await s.chat({ stream: true, tools: oaiTools, messages: ask });
    const ev = await sse(r);
    const chunks = ev.map((e) => j(e.data)).filter(Boolean);
    const content = chunks.map((c) => c.choices?.[0]?.delta?.content ?? '').join('');
    const calls = {};
    for (const c of chunks) for (const t of c.choices?.[0]?.delta?.tool_calls ?? []) {
      const k = t.index ?? 0;
      calls[k] ??= { name: '', args: '' };
      calls[k].name += t.function?.name ?? '';
      calls[k].args += t.function?.arguments ?? '';
    }
    const finish = chunks.map((c) => c.choices?.[0]?.finish_reason).filter(Boolean).at(-1);
    assert.equal(r.status, 200);
    assert.equal(calls[0].name, 'get_weather');
    assert.equal(j(calls[0].args).city, 'Bangkok');
    assert.equal(finish, 'tool_calls');
    assert.ok(!content.includes('<tool_call'), 'raw tag must not reach content deltas');
    assert.ok(!ev.some((e) => e.data.includes('<tool_call>')), 'raw tag must not appear in any event');
  });

  it('anthropic non-stream: tool_use block, stop_reason tool_use, no raw tag', async () => {
    const r = await s.messages({ tools: anthTools, messages: ask });
    const b = await r.json();
    const tu = b.content.find((c) => c.type === 'tool_use');
    assert.equal(r.status, 200);
    assert.equal(tu.name, 'get_weather');
    assert.equal(tu.input.city, 'Bangkok');
    assert.equal(b.stop_reason, 'tool_use');
    assert.ok(!JSON.stringify(b.content).includes('<tool_call>'));
  });

  it('anthropic stream: tool_use block, stop_reason tool_use, no raw tag', async () => {
    const r = await s.messages({ stream: true, tools: anthTools, messages: ask });
    const ev = (await sse(r)).filter((e) => e.ev);
    const tu = ev.filter((e) => e.ev === 'content_block_start').map((e) => j(e.data)?.content_block).find((b) => b?.type === 'tool_use');
    const deltas = ev.filter((e) => e.ev === 'content_block_delta').map((e) => j(e.data)?.delta);
    const pj = deltas.map((d) => d?.partial_json ?? '').join('');
    const text = deltas.map((d) => d?.text ?? '').join('');
    const stop = ev.filter((e) => e.ev === 'message_delta').map((e) => j(e.data)?.delta?.stop_reason).at(-1);
    assert.equal(tu.name, 'get_weather');
    assert.equal(j(pj).city, 'Bangkok');
    assert.equal(stop, 'tool_use');
    assert.ok(!text.includes('<tool_call'));
    assert.equal(ev.at(-1).ev, 'message_stop');
  });
});
