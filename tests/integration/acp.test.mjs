import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { setup } from '../support/harness.mjs';
import { BIN_DIR } from '../support/server.mjs';
import { REPLY, FAKE_PNG_B64 } from '../support/fake-extension.mjs';
import { startAcp, messageText } from '../support/acp-client.mjs';

const CLI = path.join(BIN_DIR, 'ge-bridge-cli.exe');
const FAKE_MCP = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../support/fake-mcp.mjs');

const call = (name, args) => `[[CALL]]${JSON.stringify({ name, arguments: args })}[[/CALL]]`;
const promptOf = (sessionId, text) => ({ sessionId, prompt: [{ type: 'text', text }] });

describe('ge-bridge-cli acp', () => {
  let s;
  let acp;
  const tmp = () => fs.mkdtempSync(path.join(os.tmpdir(), 'ge-acp-'));
  const start = (permission) => startAcp({ cli: CLI, port: s.port, dataDir: s.dataDir, permission });
  const newSession = async (a, cwd, mcpServers = []) => {
    const r = await a.request('session/new', { cwd, mcpServers });
    assert.ok(r.result?.sessionId, JSON.stringify(r));
    return r.result.sessionId;
  };

  before(async () => { s = await setup(); });
  after(async () => { await s?.stop(); });
  const finish = async () => { if (acp) { await acp.close(); acp = null; } };

  it('`acp --version` prints the version and exits, as OpenMausBot probes it', async () => {
    const { execFileSync } = await import('node:child_process');
    const out = execFileSync(CLI, ['acp', '--version'], { encoding: 'utf8', timeout: 5000, input: '' });
    assert.match(out, /^ge-bridge-cli \d+\.\d+\.\d+/);
  });

  it('initialize reports the capabilities', async () => {
    acp = start();
    const r = (await acp.request('initialize', { protocolVersion: 1, clientCapabilities: {} })).result;
    assert.equal(r.protocolVersion, 1);
    assert.equal(r.agentCapabilities.loadSession, false);
    assert.equal(r.agentCapabilities.promptCapabilities.embeddedContext, true);
    assert.equal(r.agentCapabilities.promptCapabilities.image, true);
    assert.equal(r.agentInfo.name, 'ge-bridge');
    assert.deepEqual((await acp.request('authenticate', { methodId: 'x' })).result, {});
    await finish();
  });

  it('session/new lists models; set_config_option and set_model work; bad input is rejected', async () => {
    acp = start();
    await acp.request('initialize', { protocolVersion: 1 });
    const r = (await acp.request('session/new', { cwd: tmp(), mcpServers: [] })).result;
    assert.match(r.sessionId, /^geb-[0-9a-f]+$/);
    const opt = r.configOptions[0];
    assert.equal(opt.id, 'model');
    assert.equal(opt.category, 'model');
    assert.equal(opt.currentValue, 'default');
    assert.ok(opt.options.some((o) => o.value === 'gemini-test-flash'));
    const set = (await acp.request('session/set_config_option', { sessionId: r.sessionId, configId: 'model', value: 'gemini-test-pro' })).result;
    assert.equal(set.configOptions[0].currentValue, 'gemini-test-pro');
    assert.deepEqual((await acp.request('session/set_model', { sessionId: r.sessionId, modelId: 'gemini-test-flash' })).result, {});
    assert.equal((await acp.request('session/set_config_option', { sessionId: r.sessionId, configId: 'nope', value: 'x' })).error.code, -32602);
    assert.equal((await acp.request('session/new', { cwd: 'relative/dir', mcpServers: [] })).error.code, -32602);
    assert.equal((await acp.request('session/new', { cwd: path.join(tmp(), 'missing'), mcpServers: [] })).error.code, -32602);
    assert.equal((await acp.request('session/prompt', promptOf('geb-nope', 'hi'))).error.code, -32602);
    assert.equal((await acp.request('session/frobnicate', {})).error.code, -32601);
    await finish();
  });

  it('a plain prompt streams the answer and ends the turn', async () => {
    acp = start();
    await acp.request('initialize', { protocolVersion: 1 });
    const sid = await newSession(acp, tmp());
    const r = await acp.request('session/prompt', promptOf(sid, 'say hi'));
    assert.equal(r.result.stopReason, 'end_turn', JSON.stringify(r));
    assert.equal(messageText(acp.updates(sid)), REPLY);
    assert.ok(acp.updates(sid).some((u) => u.sessionUpdate === 'agent_thought_chunk'));    await finish();
  });

  it('write_file with allow_once writes the file and reports the steps', async () => {
    const cwd = tmp();
    acp = start(async () => 'allow_once');
    await acp.request('initialize', { protocolVersion: 1 });
    const sid = await newSession(acp, cwd);
    const r = await acp.request('session/prompt', promptOf(sid, call('write_file', { path: 'out/a.txt', content: 'hello acp' })));
    assert.equal(r.result.stopReason, 'end_turn', JSON.stringify(r));
    assert.equal(fs.readFileSync(path.join(cwd, 'out', 'a.txt'), 'utf8'), 'hello acp');
    assert.equal(acp.permissionRequests.length, 1);
    assert.deepEqual(acp.permissionRequests[0].options.map((o) => o.optionId), ['allow_once', 'allow_always', 'reject_once']);
    const ups = acp.updates(sid);
    const tc = ups.find((u) => u.sessionUpdate === 'tool_call');
    assert.equal(tc.kind, 'edit');
    assert.equal(tc.status, 'pending');
    assert.equal(tc.rawInput.path, 'out/a.txt');
    assert.ok(ups.some((u) => u.sessionUpdate === 'tool_call_update' && u.status === 'in_progress'));
    const done = ups.find((u) => u.sessionUpdate === 'tool_call_update' && u.status === 'completed');
    assert.equal(done.toolCallId, tc.toolCallId);
    assert.ok(done.locations.length >= 1);
    assert.equal(messageText(ups), REPLY);
    await finish();
  });

  it('reject leaves the file absent and the call failed', async () => {
    const cwd = tmp();
    acp = start(async () => 'reject_once');
    await acp.request('initialize', { protocolVersion: 1 });
    const sid = await newSession(acp, cwd);
    const r = await acp.request('session/prompt', promptOf(sid, call('write_file', { path: 'no.txt', content: 'x' })));
    assert.equal(r.result.stopReason, 'end_turn');
    assert.equal(fs.existsSync(path.join(cwd, 'no.txt')), false);
    const failed = acp.updates(sid).find((u) => u.sessionUpdate === 'tool_call_update' && u.status === 'failed');
    assert.match(failed.content[0].content.text, /rejected/);
    await finish();
  });

  it('a path outside the folder fails and nothing is written', async () => {
    const parent = tmp();
    const cwd = path.join(parent, 'work');
    fs.mkdirSync(cwd);
    acp = start(async () => 'allow_once');
    await acp.request('initialize', { protocolVersion: 1 });
    const sid = await newSession(acp, cwd);
    await acp.request('session/prompt', promptOf(sid, call('write_file', { path: '../escape.txt', content: 'x' })));
    assert.equal(fs.existsSync(path.join(parent, 'escape.txt')), false);
    assert.ok(acp.updates(sid).some((u) => u.sessionUpdate === 'tool_call_update' && u.status === 'failed'));
    await finish();
  });

  it('allow_always skips the next permission request for the same kind of call', async () => {
    acp = start(async () => 'allow_always');
    await acp.request('initialize', { protocolVersion: 1 });
    const sid = await newSession(acp, tmp());
    for (let i = 0; i < 2; i++) {
      const r = await acp.request('session/prompt', promptOf(sid, call('run_command', { command: 'Write-Output hi' })));
      assert.equal(r.result.stopReason, 'end_turn', JSON.stringify(r));
    }
    assert.equal(acp.permissionRequests.length, 1);
    const done = acp.updates(sid).filter((u) => u.sessionUpdate === 'tool_call_update' && u.status === 'completed');
    assert.equal(done.length, 2);
    assert.match(done[1].content[0].content.text, /hi/);
    await finish();
  });

  it('passes MCP tools through and saves their images', async () => {
    const cwd = tmp();
    acp = start();
    await acp.request('initialize', { protocolVersion: 1 });
    const mcp = [{ name: 'fake', command: process.execPath, args: [FAKE_MCP, 'fake'], env: [] }];
    const sid = await newSession(acp, cwd, mcp);
    let r = await acp.request('session/prompt', promptOf(sid, call('fake_echo', { text: 'ping' })));
    assert.equal(r.result.stopReason, 'end_turn', JSON.stringify(r));
    assert.equal(acp.permissionRequests.length, 0, 'MCP tools need no approval');
    let done = acp.updates(sid).filter((u) => u.status === 'completed');
    assert.match(done[0].content[0].content.text, /echo: ping/);
    const tc = acp.updates(sid).find((u) => u.sessionUpdate === 'tool_call');
    assert.equal(tc.kind, 'other');
    assert.equal(tc.title, 'fake_echo');

    r = await acp.request('session/prompt', promptOf(sid, call('fake_image', { })));
    assert.equal(r.result.stopReason, 'end_turn', JSON.stringify(r));
    const last = s.ext.runTurns.at(-1);
    const dir = path.join(cwd, 'generated-images');
    const files = fs.readdirSync(dir);
    assert.equal(files.length, 1);
    assert.match(files[0], /\.png$/);
    done = acp.updates(sid).filter((u) => u.status === 'completed');
    const img = done[1].content.find((c) => c.content.type === 'image');
    assert.equal(img.content.mimeType, 'image/png');
    assert.match(done[1].content[0].content.text, /Saved to generated-images/);

    assert.equal(last.images.length, 1);
    assert.match(last.images[0], /^data:image\/png;base64,/);
    await finish();
  });

  it('forwards images in the prompt to Gemini', async () => {
    acp = start();
    await acp.request('initialize', { protocolVersion: 1 });
    const sid = await newSession(acp, tmp());
    const r = await acp.request('session/prompt', {
      sessionId: sid,
      prompt: [{ type: 'text', text: 'what is this' }, { type: 'image', mimeType: 'image/png', data: FAKE_PNG_B64 }],
    });
    assert.equal(r.result.stopReason, 'end_turn', JSON.stringify(r));
    assert.deepEqual(s.ext.runTurns.at(-1).images, ['data:image/png;base64,' + FAKE_PNG_B64]);
    await finish();
  });

  it('cancel while a permission is pending ends the turn as cancelled', async () => {
    const cwd = tmp();
    acp = start(() => new Promise(() => {}));
    await acp.request('initialize', { protocolVersion: 1 });
    const sid = await newSession(acp, cwd);
    const p = acp.request('session/prompt', promptOf(sid, call('write_file', { path: 'c.txt', content: 'x' })));
    const t0 = Date.now();
    while (!acp.permissionRequests.length) {
      if (Date.now() - t0 > 20000) throw new Error('no permission request');
      await new Promise((r) => setTimeout(r, 25));
    }
    acp.notify('session/cancel', { sessionId: sid });
    const r = await p;
    assert.equal(r.result.stopReason, 'cancelled', JSON.stringify(r));
    assert.equal(fs.existsSync(path.join(cwd, 'c.txt')), false);
    await finish();
  });

  it('keeps stdout to JSON lines and exits 0 when stdin closes', async () => {
    acp = start();
    await acp.request('initialize', { protocolVersion: 1 });
    const sid = await newSession(acp, tmp());
    await acp.request('session/prompt', promptOf(sid, 'hello'));
    assert.deepEqual(acp.badLines, []);
    assert.equal(await acp.close(), 0);
    acp = null;
  });
});
