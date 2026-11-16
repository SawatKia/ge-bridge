import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { setup } from '../support/harness.mjs';
import { startFakeExtension, waitFor, makeTabs } from '../support/fake-extension.mjs';
import { user } from '../support/http.mjs';

describe('extension link', () => {
  let s;
  before(async () => { s = await setup(false); });
  after(async () => { await s?.stop(); });

  it('no extension: health reports disconnected and chat fails cleanly', async () => {
    const h = await s.health();
    assert.equal(h.extensionConnected, false);
    assert.equal(h.tabs, 0);
    const r = await s.chat({ stream: false });
    assert.ok(r.status >= 400, `expected error status, got ${r.status}`);
    await r.text();
  });

  it('closing the first extension after a second one connected keeps the second live', async () => {
    const a = await startFakeExtension({ base: s.base, token: s.token, name: 'A', tabIds: [101, 102] });
    await waitFor(async () => (await s.health()).tabs === 2, 'A tabs synced');
    const b = await startFakeExtension({ base: s.base, token: s.token, name: 'B', tabIds: [201] });
    await waitFor(async () => (await s.health()).tabs === 1, 'B tabs replace A tabs');

    a.close();
    await a.closed;
    await new Promise((r) => setTimeout(r, 300));

    const h = await s.health();
    assert.equal(h.extensionConnected, true, 'B must still be the live connection');
    assert.equal(h.tabs, 1, 'tabs from B must survive A closing');

    const r = await s.chat({ stream: false, messages: [user('served by B')] });
    assert.equal(r.status, 200);
    await r.json();
    assert.equal(b.runTurns.length, 1, 'turn must reach B');
    assert.equal(b.runTurns[0].tabId, 201);
    assert.equal(a.runTurns.length, 0, 'closed A must receive nothing');
    b.close();
    await b.closed;
  });

  it('a stale TABS_SYNC from a superseded extension does not change tabs', async () => {
    const a = await startFakeExtension({ base: s.base, token: s.token, name: 'A2', tabIds: [101, 102] });
    await waitFor(async () => (await s.health()).tabs === 2, 'A2 tabs synced');
    const b = await startFakeExtension({ base: s.base, token: s.token, name: 'B2', tabIds: [301] });
    await waitFor(async () => (await s.health()).tabs === 1, 'B2 tabs replace A2 tabs');

    a.send({ type: 'TABS_SYNC', tabs: makeTabs([401, 402, 403]) });
    await new Promise((r) => setTimeout(r, 300));
    assert.equal((await s.health()).tabs, 1, 'stale sync must be ignored');

    const r = await s.chat({ stream: false, messages: [user('after stale sync')] });
    assert.equal(r.status, 200);
    await r.json();
    assert.equal(b.runTurns[0].tabId, 301);
    a.close();
    b.close();
    await Promise.all([a.closed, b.closed]);
  });
});
