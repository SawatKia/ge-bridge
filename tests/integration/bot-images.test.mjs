import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { setup } from '../support/harness.mjs';
import { FAKE_PNG_B64, REPLY } from '../support/fake-extension.mjs';
import { user } from '../support/http.mjs';

const fn = (name, description) => ({ type: 'function', function: { name, description, parameters: { type: 'object' } } });
const bridged = [
  fn('agents_post_to_thread', 'post'),
  fn('agents_find_tool', 'Find a tool. Tools reached this way: trigger, generate_image, publish_page.'),
  fn('agents_use_tool', 'Run a hidden tool'),
];

describe('generated images are saved into the asking bot\'s working folder', () => {
  let s;
  let workdir;
  before(async () => {
    s = await setup();
    // Isolated run: OpenMausBot's data folder lives at <data dir>/openmausbot.
    workdir = path.join(s.dataDir, 'project with space');
    const omb = path.join(s.dataDir, 'openmausbot');
    fs.mkdirSync(omb, { recursive: true });
    fs.writeFileSync(path.join(omb, 'bots.json'), JSON.stringify([
      { id: 'b1', name: 'Artist', cwd: workdir },
      { id: 'b2', name: 'Helper', cwd: null },
    ]));
  });
  after(async () => { await s?.stop(); });

  async function makeImageThenFollowUp(botName, prompt) {
    const img = await s.post('/v1/images/generations', { model: 'gemini-enterprise-image', prompt, response_format: 'b64_json', size: '1024x1024', n: 1 });
    assert.equal(img.status, 200);
    const call = { id: 'c1', type: 'function', function: { name: 'agents_use_tool', arguments: JSON.stringify({ name: 'generate_image', arguments: { prompt, aspect: '1:1' } }) } };
    const r = await s.chat({
      stream: false,
      tools: bridged,
      messages: [
        { role: 'system', content: `You are ${botName}, a personal bot in OpenMausBot.` },
        user('draw a red apple'),
        { role: 'assistant', content: null, tool_calls: [call] },
        { role: 'tool', tool_call_id: 'c1', content: 'Image made. It will be attached to this turn\'s reply.' },
      ],
    });
    assert.equal(r.status, 200);
    return (await r.json()).choices[0].message.content;
  }

  it('saves into the bot cwd and adds a preview and a link to the reply', async () => {
    const content = await makeImageThenFollowUp('Artist', 'A red apple on a white table');
    const dir = path.join(workdir, 'generated-images');
    const files = fs.readdirSync(dir);
    assert.equal(files.length, 1);
    assert.match(files[0], /^red-apple-white-table-\d+\.png$/);
    assert.deepEqual(fs.readFileSync(path.join(dir, files[0])), Buffer.from(FAKE_PNG_B64, 'base64'));
    assert.ok(content.startsWith(REPLY), 'the model reply comes first');
    const url = `file:///${path.join(dir, files[0]).replace(/\\/g, '/').replace(/ /g, '%20')}`;
    assert.ok(content.includes(`![${files[0]}](${url})`), content);
    assert.ok(content.includes(`Saved to [\`${files[0]}\`](${url})`), content);
  });

  it('a bot without a cwd gets its workspace folder', async () => {
    await makeImageThenFollowUp('Helper', 'A blue cube');
    const dir = path.join(s.dataDir, 'openmausbot', 'workspaces', 'b2', 'generated-images');
    assert.equal(fs.readdirSync(dir).length, 1);
  });

  it('an unknown bot falls back to the Pictures folder (isolated here)', async () => {
    const content = await makeImageThenFollowUp('Stranger', 'A green pear');
    const dir = path.join(s.dataDir, 'Pictures', 'GeBridge', 'generated-images');
    assert.equal(fs.readdirSync(dir).length, 1);
    assert.ok(content.includes('Saved to'));
  });

  it('a normal turn is left alone', async () => {
    const r = await s.chat({ stream: false, tools: bridged, messages: [{ role: 'system', content: 'You are Artist, a personal bot in OpenMausBot.' }, user('hello')] });
    assert.equal((await r.json()).choices[0].message.content, REPLY);
  });
});
