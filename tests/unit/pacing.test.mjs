import test from 'node:test';
import assert from 'node:assert/strict';
import { loadLib } from './load-extension-lib.mjs';

const lib = loadLib();
const plain = (v) => JSON.parse(JSON.stringify(v));

// Deterministic uniform [0, 1) source for repeatable jitter.
function seeded(seed = 42) {
  let s = seed >>> 0;
  return () => {
    s = (s * 1664525 + 1013904223) >>> 0;
    return s / 2 ** 32;
  };
}

test('countWords counts Thai words without spaces and English words', () => {
  assert.equal(lib.countWords(''), 0);
  assert.equal(lib.countWords('hello big world'), 3);
  const thai = lib.countWords('สวัสดีครับวันนี้อากาศดีมาก');
  assert.ok(thai >= 5 && thai <= 8, `thai words: ${thai}`);
  assert.equal(lib.countWords('ok, 42!'), 2, 'punctuation is not a word');
});

test('normalizePacing fills defaults and clamps bad values', () => {
  assert.deepEqual(plain(lib.normalizePacing(undefined)), plain(lib.PACING_DEFAULTS));
  const p = lib.normalizePacing({ wpmMin: 300, wpmMax: 100, minDelayMs: 99999, maxDelayMs: 5000, jitter: 9, enabled: 'yes' });
  assert.equal(p.wpmMax, 300, 'max is never below min');
  assert.equal(p.minDelayMs, 5000, 'min delay never above max delay');
  assert.equal(p.jitter, 1);
  assert.equal(p.enabled, true);
  assert.equal(lib.normalizePacing({ maxDelayMs: 1e9 }).maxDelayMs, 600000, 'max delay capped at 10 minutes');
  assert.equal(lib.normalizePacing({ maxDelayMs: 'abc' }).maxDelayMs, 20000);
  assert.equal(lib.normalizePacing({ enabled: false }).enabled, false);
});

test('pacingDelay follows reading time at 200-250 wpm with Gaussian jitter', () => {
  const words = Array.from({ length: 50 }, (_, i) => `word${i}`).join(' ');
  const settings = { maxDelayMs: 600000, minDelayMs: 0 };
  const rand = seeded(7);
  const samples = Array.from({ length: 2000 }, () => lib.pacingDelay(words, settings, rand));
  for (const d of samples) {
    assert.equal(d.words, 50);
    assert.ok(d.wpm >= 200 && d.wpm <= 250, `wpm ${d.wpm}`);
  }
  // 50 words at 200-250 wpm = 12-15 s; the mean with zero-mean jitter sits inside that band.
  const mean = samples.reduce((a, d) => a + d.ms, 0) / samples.length;
  assert.ok(mean > 12000 && mean < 15000, `mean ${mean}`);
  const sd = Math.sqrt(samples.reduce((a, d) => a + (d.ms - mean) ** 2, 0) / samples.length);
  assert.ok(sd > 1000 && sd < 4000, `jitter spread ${sd}`);
});

test('pacingDelay clamps to the min and the user-set max, and is 0 when off', () => {
  const long = 'word '.repeat(2000);
  assert.equal(lib.pacingDelay(long, { maxDelayMs: 8000 }, seeded()).ms, 8000);
  assert.equal(lib.pacingDelay('', {}, seeded()).ms, 1500);
  assert.equal(lib.pacingDelay(long, { enabled: false }, seeded()).ms, 0);
});

test('gaussian is roughly standard normal', () => {
  const rand = seeded(3);
  const xs = Array.from({ length: 5000 }, () => lib.gaussian(rand));
  const mean = xs.reduce((a, b) => a + b, 0) / xs.length;
  const sd = Math.sqrt(xs.reduce((a, b) => a + (b - mean) ** 2, 0) / xs.length);
  assert.ok(Math.abs(mean) < 0.05, `mean ${mean}`);
  assert.ok(Math.abs(sd - 1) < 0.05, `sd ${sd}`);
});

test('auto mode tries the page tab first, then the extension, then typing', () => {
  assert.deepEqual(plain(lib.transportOrder('auto')), ['api-tab', 'api-sw', 'dom']);
  assert.deepEqual(plain(lib.transportOrder('auto', { hasImages: true })), ['api-tab', 'api-sw'], 'typing cannot attach files');
  assert.deepEqual(plain(lib.transportOrder('auto', { hasTarget: false })), ['dom']);
  assert.deepEqual(plain(lib.transportOrder('api-sw')), ['api-sw']);
  assert.deepEqual(plain(lib.transportOrder('auto', { kind: 'image' })), ['api-tab', 'api-sw']);
  assert.deepEqual(plain(lib.transportOrder('dom', { kind: 'image' })), ['api-tab', 'api-sw']);
});

test('a failure on every transport names each reason and says to open the tab', () => {
  const msg = lib.transportFailureMessage([
    { mode: 'api-tab', message: 'No scriptable Gemini tab for API-from-tab transport' },
    { mode: 'api-sw', message: '{\n  "error": { "code": 401 } }', httpStatus: 401 },
    { mode: 'dom', message: 'No scriptable Gemini tab available for DOM transport' },
  ]);
  assert.match(msg, /^Turn failed on every way to reach Gemini \(API from the Gemini tab: No scriptable Gemini tab for API-from-tab transport; API from the extension: HTTP 401; typing into the Gemini tab: No scriptable/);
  assert.match(msg, /Open your Gemini Enterprise page in Chrome, signed in, and try again\.$/);
  assert.equal(lib.transportFailureMessage([{ mode: 'api-tab', message: 'boom' }]), 'boom', 'one failure keeps its own message');
  assert.equal(lib.transportFailureMessage([], 'Image generation'), 'Image generation failed');
});
