import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { newContext, loadLib } from './load-extension-lib.mjs';

const lib = loadLib();
// Objects from the vm realm have foreign prototypes; compare plain copies.
const plain = (v) => JSON.parse(JSON.stringify(v));
const CID = '00000000-0000-4000-8000-000000000000';
const G = 'https://vertexaisearch.cloud.google';
const GC = 'https://vertexaisearch.cloud.google.com';

test('parseGeminiUrl accept/reject table', () => {
  const accept = [
    [`${G}/us/home/cid/${CID}?csesidx=123`, 'us', `${G}/us/home/cid/${CID}?csesidx=123`],
    [`${G}/home/cid/${CID}?csesidx=123`, 'us', `${G}/home/cid/${CID}?csesidx=123`],
    [`${GC}/eu/home/cid/${CID}?csesidx=456`, 'eu', `${GC}/eu/home/cid/${CID}?csesidx=456`],
    [`${GC}/home/cid/${CID}?csesidx=456`, 'us', `${GC}/home/cid/${CID}?csesidx=456`],
    [`${G}/us/home/cid/${CID}/r/session/x?csesidx=123`, 'us', `${G}/us/home/cid/${CID}?csesidx=123`],
    [`  ${G}/us/home/cid/${CID}?csesidx=123  `, 'us', `${G}/us/home/cid/${CID}?csesidx=123`],
    [`${G}/us/home/cid/${CID}?foo=1&csesidx=7#frag`, 'us', `${G}/us/home/cid/${CID}?csesidx=7`]
  ];
  for (const [url, location, homeUrl] of accept) {
    const r = lib.parseGeminiUrl(url);
    assert.ok(r, `should accept ${url}`);
    assert.equal(r.location, location, url);
    assert.equal(r.cid, CID, url);
    assert.equal(r.homeUrl, homeUrl, url);
    assert.equal(r.origin, new URL(url.trim()).origin, url);
  }
  assert.equal(lib.parseGeminiUrl(`${G}/home/cid/${CID}?csesidx=123`).csesidx, '123');

  const reject = [
    `${G}/us/home/cid/${CID}`,
    `${G}/us/home/cid/${CID}?csesidx=`,
    `${G}/us/home/cid/${CID}?csesidx=abc`,
    `${G}/us/home/cid/${CID}?csesidx=12a`,
    `http://vertexaisearch.cloud.google/home/cid/${CID}?csesidx=1`,
    `https://evil.example/us/home/cid/${CID}?csesidx=1`,
    `https://vertexaisearch.cloud.google.com.evil.io/home/cid/${CID}?csesidx=1`,
    `https://evilvertexaisearch.cloud.google/home/cid/${CID}?csesidx=1`,
    `${G}/us/home/cid/short?csesidx=1`,
    `${G}/us/home/nocid?csesidx=1`,
    'not a url',
    '',
    undefined,
    null
  ];
  for (const url of reject) assert.equal(lib.parseGeminiUrl(url), null, `should reject ${url}`);
});

test('isGeminiUrl table', () => {
  const yes = [`${G}/x`, `${GC}/x`, `${G}/`, `${GC}/us/home/cid/${CID}?csesidx=1`];
  const no = [
    'https://vertexaisearch.cloud.google.evil/x',
    'http://vertexaisearch.cloud.google/x',
    'https://example.com/',
    'https://vertexaisearch.cloud.googlefoo/x',
    '',
    null,
    undefined,
    42
  ];
  for (const u of yes) assert.equal(lib.isGeminiUrl(u), true, String(u));
  for (const u of no) assert.equal(lib.isGeminiUrl(u), false, String(u));
});

test('base64url round-trip with all byte values, url-safe output', () => {
  const all = Uint8Array.from({ length: 256 }, (_, i) => i);
  const enc = lib.base64urlEncode(all);
  assert.doesNotMatch(enc, /[+/=]/);
  assert.equal(enc, Buffer.from(all).toString('base64url'));
  assert.deepEqual(Buffer.from(lib.base64urlDecode(enc)), Buffer.from(all));
  for (let n = 0; n < 8; n++) {
    const b = crypto.randomBytes(n);
    const e = lib.base64urlEncode(b);
    assert.doesNotMatch(e, /[+/=]/);
    assert.deepEqual(Buffer.from(lib.base64urlDecode(e)), b);
  }
});

test('stripXssi', () => {
  assert.equal(lib.stripXssi(")]}'\n{\"a\":1}"), '{"a":1}');
  assert.equal(lib.stripXssi(")]}'{\"a\":1}"), '{"a":1}');
  assert.equal(lib.stripXssi('{"a":1}'), '{"a":1}');
  assert.equal(lib.stripXssi('x)]}\'y'), 'x)]}\'y');
});

test('geminiJwtClaims + signJwtHS256 match node HMAC', async () => {
  const secret = crypto.randomBytes(32);
  const claims = lib.geminiJwtClaims(G, 'us', '123', 1700000000);
  assert.deepEqual({ ...claims }, {
    iss: G,
    aud: 'https://content-us-discoveryengine.googleapis.com',
    sub: 'csesidx/123',
    iat: 1700000000,
    exp: 1700000300,
    nbf: 1700000000
  });
  const jwt = await lib.signJwtHS256(lib.base64urlDecode(secret.toString('base64url')), { alg: 'HS256', typ: 'JWT', kid: 'k1' }, claims);
  assert.doesNotMatch(jwt, /[+/=]/);
  const [h, p, s] = jwt.split('.');
  assert.equal(jwt.split('.').length, 3);
  assert.equal(Buffer.from(h, 'base64url').toString(), '{"alg":"HS256","typ":"JWT","kid":"k1"}');
  const payload = JSON.parse(Buffer.from(p, 'base64url').toString());
  assert.deepEqual(payload, { ...claims });
  assert.equal(payload.exp, payload.iat + 300);
  assert.equal(s, crypto.createHmac('sha256', secret).update(`${h}.${p}`).digest('base64url'));
});

// ---- probe with fake fetch ----
const XSRF = crypto.randomBytes(32).toString('base64url');
const SESSION_NAME = 'projects/p/locations/us/sessions/secret-session-name';
const target = lib.parseGeminiUrl(`${G}/us/home/cid/${CID}?csesidx=123`);

function res(status, body, statusText = '') {
  const text = typeof body === 'string' ? body : JSON.stringify(body);
  return {
    ok: status >= 200 && status < 300,
    status,
    statusText,
    text: async () => text,
    json: async () => JSON.parse(text)
  };
}

function fakeFetch(handlers) {
  const calls = [];
  const fn = async (url, opts) => {
    calls.push({ url, opts });
    const h = url.includes('/auth/getoxsrf') ? handlers.auth : handlers.session;
    return h(url, opts);
  };
  fn.calls = calls;
  return fn;
}

const okAuth = () => res(200, `)]}'\n${JSON.stringify({ xsrfToken: XSRF, keyId: 'k1' })}`);
const okSession = () => res(200, { session: { name: SESSION_NAME } });
const deps = (f) => ({ fetch: f, crypto: crypto.webcrypto, now: () => 1700000000000, origin: G });

test('probe success: ok and no secrets in result', async () => {
  const f = fakeFetch({ auth: okAuth, session: okSession });
  const r = await lib.runGeminiProbe(target, deps(f));
  assert.equal(r.ok, true);
  assert.deepEqual(plain(r.steps.map((s) => [s.step, s.ok])), [['auth', true], ['jwt', true], ['session', true]]);
  assert.equal(r.steps[2].hasSession, true);
  assert.equal(f.calls.length, 2);
  assert.equal(f.calls[0].url, `${G}/auth/getoxsrf?csesidx=123`);
  assert.equal(f.calls[0].opts.credentials, 'include');
  assert.equal(f.calls[1].url, 'https://content-us-discoveryengine.googleapis.com/v1alpha/locations/us/widgetCreateSession');
  const bearer = f.calls[1].opts.headers.Authorization;
  assert.match(bearer, /^Bearer [\w-]+\.[\w-]+\.[\w-]+$/);
  const jwt = bearer.slice('Bearer '.length);
  const [h, p, s] = jwt.split('.');
  assert.equal(s, crypto.createHmac('sha256', Buffer.from(XSRF, 'base64url')).update(`${h}.${p}`).digest('base64url'));
  assert.equal(JSON.parse(Buffer.from(p, 'base64url').toString()).iat, 1700000000);
  const json = JSON.stringify(r);
  for (const secret of [XSRF, jwt, SESSION_NAME, 'secret-session-name', 'Bearer', 'cookie']) {
    assert.ok(!json.includes(secret), `result leaks ${secret}`);
  }
});

test('probe fails at target step', async () => {
  const f = fakeFetch({ auth: okAuth, session: okSession });
  const r = await lib.runGeminiProbe(null, deps(f));
  assert.equal(r.ok, false);
  assert.equal(r.steps.length, 1);
  assert.equal(r.steps[0].step, 'target');
  assert.equal(r.steps[0].ok, false);
  assert.equal(f.calls.length, 0);
});

test('probe fails at auth: HTTP 401', async () => {
  const f = fakeFetch({ auth: () => res(401, 'nope', 'Unauthorized'), session: okSession });
  const r = await lib.runGeminiProbe(target, deps(f));
  assert.equal(r.ok, false);
  assert.equal(r.steps.length, 1);
  assert.equal(r.steps[0].step, 'auth');
  assert.equal(r.steps[0].httpStatus, 401);
  assert.equal(r.steps[0].error, 'nope');
  assert.equal(f.calls.length, 1);
});

test('probe fails at auth: bad JSON and missing fields', async () => {
  let f = fakeFetch({ auth: () => res(200, 'not json at all'), session: okSession });
  let r = await lib.runGeminiProbe(target, deps(f));
  assert.equal(r.ok, false);
  assert.equal(r.steps[0].step, 'auth');
  assert.equal(r.steps[0].error, 'getoxsrf response is not JSON');
  assert.equal(r.steps.length, 1);

  f = fakeFetch({ auth: () => res(200, { keyId: 'k1' }), session: okSession });
  r = await lib.runGeminiProbe(target, deps(f));
  assert.equal(r.ok, false);
  assert.match(r.steps[0].error, /missing xsrfToken\/keyId/);
});

test('probe fails at session: 403 with long body capped at 200 chars', async () => {
  const f = fakeFetch({ auth: okAuth, session: () => res(403, 'denied ' + 'z'.repeat(500)) });
  const r = await lib.runGeminiProbe(target, deps(f));
  assert.equal(r.ok, false);
  assert.deepEqual(plain(r.steps.map((s) => s.step)), ['auth', 'jwt', 'session']);
  const s = r.steps[2];
  assert.equal(s.ok, false);
  assert.equal(s.httpStatus, 403);
  assert.equal(s.error.length, 200);
  assert.ok(s.error.startsWith('denied zzz'));
  assert.ok(!JSON.stringify(r).includes(XSRF));
});

test('probe fails at session: response without session.name', async () => {
  const f = fakeFetch({ auth: okAuth, session: () => res(200, { other: 1 }) });
  const r = await lib.runGeminiProbe(target, deps(f));
  assert.equal(r.ok, false);
  assert.match(r.steps[2].error, /no session\.name/);
});

test('lib loads twice in the same context without throwing and keeps the first instance', () => {
  const ctx = newContext();
  const first = loadLib(ctx);
  const second = loadLib(ctx);
  assert.equal(second, first);
  assert.equal(typeof second.parseGeminiUrl, 'function');
});
