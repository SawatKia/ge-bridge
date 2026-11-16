import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { setup } from '../support/harness.mjs';
import { REPLY } from '../support/fake-extension.mjs';
import { sse, j, user } from '../support/http.mjs';

const fn = (name, description) => ({ type: 'function', function: { name, description, parameters: { type: 'object' } } });
const bridged = [
  fn('agents_post_to_thread', 'post'),
  fn('agents_find_tool', 'Find a tool. Tools reached this way: trigger, generate_image, publish_page.'),
  fn('agents_use_tool', 'Run a hidden tool'),
];

describe('refusal and policy-block recovery', () => {
  let s;
  before(async () => { s = await setup(); });
  after(async () => { await s?.stop(); });

  const turnsSince = (n) => s.ext.runTurns.slice(n);

  it('a refusal is replaced by one retry in a new chat with the shortened prompt (non-stream)', async () => {
    const n = s.ext.runTurns.length;
    const r = await s.chat({ stream: false, messages: [{ role: 'system', content: 'persona' }, user('old'), { role: 'assistant', content: 'old answer' }, user('draw it [[REFUSE_ONCE]]')] });
    const b = await r.json();
    assert.equal(r.status, 200);
    assert.equal(b.choices[0].message.content, REPLY);
    const turns = turnsSince(n);
    assert.equal(turns.length, 2);
    assert.equal(turns[1].newChat, true);
    assert.ok(turns[1].prompt.startsWith('[ROLE]'));
    assert.ok(turns[1].prompt.includes('[USER REQUEST]\ndraw it [[REFUSE_ONCE]]'));
    assert.ok(!turns[1].prompt.includes('old answer'), 'history dropped in the retry');
  });

  it('streaming clients never see the refused text', async () => {
    const r = await s.chat({ stream: true, messages: [user('draw it [[REFUSE_ONCE]]')] });
    const chunks = (await sse(r)).map((e) => j(e.data)).filter(Boolean);
    const content = chunks.map((c) => c.choices?.[0]?.delta?.content ?? '').join('');
    assert.equal(content, REPLY);
  });

  it('a policy block (HTTP 400) is retried once', async () => {
    const n = s.ext.runTurns.length;
    const r = await s.chat({ stream: false, messages: [user('check this [[BLOCK_ONCE]]')] });
    assert.equal(r.status, 200);
    assert.equal((await r.json()).choices[0].message.content, REPLY);
    assert.equal(turnsSince(n).length, 2);
  });

  it('ordinary failures are not retried', async () => {
    const n = s.ext.runTurns.length;
    const r = await s.chat({ stream: false, messages: [user('boom [[FAIL]]')] });
    assert.ok(r.status >= 400);
    assert.equal(turnsSince(n).length, 1);
  });

  it('an image refusal becomes the agent generate_image call through use_tool', async () => {
    const n = s.ext.runTurns.length;
    const r = await s.chat({ stream: true, tools: bridged, messages: [user('draw a red apple [[IMAGE_REFUSE]]')] });
    const chunks = (await sse(r)).map((e) => j(e.data)).filter(Boolean);
    const content = chunks.map((c) => c.choices?.[0]?.delta?.content ?? '').join('');
    const calls = chunks.flatMap((c) => c.choices?.[0]?.delta?.tool_calls ?? []);
    assert.equal(content, '', 'the refusal never reaches the client');
    assert.equal(calls.length, 1);
    assert.equal(calls[0].function.name, 'agents_use_tool');
    assert.deepEqual(JSON.parse(calls[0].function.arguments), { name: 'generate_image', arguments: { prompt: 'A red apple on a white table', aspect: '1:1' } });
    assert.equal(chunks.at(-1).choices[0].finish_reason, 'tool_calls');
    const turns = turnsSince(n);
    assert.equal(turns.length, 2);
    assert.ok(turns[1].prompt.startsWith('Write one prompt for an AI image generator'));
    assert.ok(turns[1].prompt.includes('Request:\ndraw a red apple'), turns[1].prompt);
  });

  it('the OpenMausBot context note never reaches the prompt writer or the aspect', async () => {
    const n = s.ext.runTurns.length;
    const note = 'Context from OpenMausBot updated since this conversation started; it replaces any earlier copy:\n\nComputers: a desktop with a web browser and a file terminal.';
    const req = 'Ultra-realistic photo of an idol, shot on 85mm portrait lens [[IMAGE_REFUSE]]';
    const r = await s.chat({ stream: false, tools: bridged, messages: [user(`${note}\n\n${req}`)] });
    const call = (await r.json()).choices[0].message.tool_calls[0];
    assert.equal(JSON.parse(call.function.arguments).arguments.aspect, '1:1');
    const writer = turnsSince(n)[1].prompt;
    assert.ok(writer.includes(`Request:\n${req}`), writer);
    assert.ok(!writer.includes('web browser'), writer);
  });

  it('a picture request answered with a brief instead of the tool becomes the image call', async () => {
    const n = s.ext.runTurns.length;
    const r = await s.chat({ stream: true, tools: bridged, messages: [user('draw a sticker of a cute girl [[BRIEF]]')] });
    const chunks = (await sse(r)).map((e) => j(e.data)).filter(Boolean);
    const content = chunks.map((c) => c.choices?.[0]?.delta?.content ?? '').join('');
    const calls = chunks.flatMap((c) => c.choices?.[0]?.delta?.tool_calls ?? []);
    assert.equal(content, '', 'the brief never reaches the client');
    assert.equal(calls.length, 1);
    assert.equal(JSON.parse(calls[0].function.arguments).name, 'generate_image');
    assert.ok(turnsSince(n)[1].prompt.startsWith('Write one prompt for an AI image generator'));
  });

  it('after the image tool ran, the follow-up answer is not turned into another picture', async () => {
    const n = s.ext.runTurns.length;
    const call = { id: 'c1', type: 'function', function: { name: 'agents_use_tool', arguments: '{"name":"generate_image","arguments":{"prompt":"x"}}' } };
    const r = await s.chat({
      stream: false,
      tools: bridged,
      messages: [user('draw a sticker of a cute girl [[BRIEF]]'), { role: 'assistant', content: null, tool_calls: [call] }, { role: 'tool', tool_call_id: 'c1', content: 'Image made.' }],
    });
    const b = await r.json();
    assert.equal(r.status, 200);
    assert.equal(b.choices[0].message.content, REPLY);
    assert.equal(turnsSince(n).length, 1, 'no rescue on a tool-result turn');
  });

  it('an image refusal without an image tool gets the normal retry', async () => {
    const n = s.ext.runTurns.length;
    const r = await s.chat({ stream: false, tools: [fn('get_weather', 'weather')], messages: [user('draw [[IMAGE_REFUSE]]')] });
    assert.equal(r.status, 200);
    const turns = turnsSince(n);
    assert.equal(turns.length, 2);
    assert.ok(turns[1].prompt.startsWith('[ROLE]'));
  });

  it('anthropic stream puts thoughts in a thinking block before the text block', async () => {
    const r = await s.messages({ stream: true });
    const ev = (await sse(r)).filter((e) => e.ev).map((e) => ({ ev: e.ev, d: j(e.data) }));
    const starts = ev.filter((e) => e.ev === 'content_block_start').map((e) => [e.d.index, e.d.content_block.type]);
    assert.deepEqual(starts, [[0, 'thinking'], [1, 'text']]);
    const thinking = ev.filter((e) => e.d?.delta?.type === 'thinking_delta').map((e) => e.d.delta.thinking).join('');
    const text = ev.filter((e) => e.d?.delta?.type === 'text_delta').map((e) => e.d.delta.text).join('');
    assert.equal(thinking, 'Thinking about it. ');
    assert.equal(text, REPLY);
    assert.equal(ev.at(-1).ev, 'message_stop');
  });
});
