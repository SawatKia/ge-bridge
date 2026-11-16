import test from 'node:test';
import assert from 'node:assert/strict';
import { loadLib } from './load-extension-lib.mjs';

const lib = loadLib();
const G = 'https://vertexaisearch.cloud.google/us/home/cid/x/r/session/1?csesidx=1';
const GC = 'https://vertexaisearch.cloud.google.com/us/home/cid/x?csesidx=1';

test('isSleepingTab: discarded or unloaded', () => {
  assert.equal(lib.isSleepingTab({ discarded: true, status: 'unloaded' }), true);
  assert.equal(lib.isSleepingTab({ discarded: false, status: 'unloaded' }), true);
  assert.equal(lib.isSleepingTab({ discarded: false, status: 'complete' }), false);
  assert.equal(lib.isSleepingTab(null), false);
});

test('pickTabToWake prefers the asked tab, then a sleeping one, on the right host', () => {
  const awakeBroken = { id: 1, url: G, discarded: false, status: 'complete' };
  const asleep = { id: 2, url: G, discarded: true, status: 'unloaded' };
  const otherHost = { id: 3, url: GC, discarded: true, status: 'unloaded' };
  // The case from the logs: the only Gemini tab was discarded by Chrome.
  assert.equal(lib.pickTabToWake([asleep], {}).id, 2);
  assert.equal(lib.pickTabToWake([awakeBroken, asleep], {}).id, 2, 'a sleeping tab before a loaded one');
  assert.equal(lib.pickTabToWake([awakeBroken, asleep], { preferId: 1 }).id, 1, 'the asked tab first');
  assert.equal(lib.pickTabToWake([otherHost, asleep], { host: 'vertexaisearch.cloud.google' }).id, 2);
  assert.equal(lib.pickTabToWake([otherHost], { host: 'vertexaisearch.cloud.google' }), null, 'none on the host: open a new one');
  assert.equal(lib.pickTabToWake([], {}), null);
  assert.equal(lib.pickTabToWake([{ id: 9 }], {}), null, 'a tab without a URL is skipped');
});
