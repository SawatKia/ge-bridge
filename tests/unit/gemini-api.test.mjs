import test from 'node:test';
import assert from 'node:assert/strict';
import { loadLib } from './load-extension-lib.mjs';

const lib = loadLib();
const plain = (v) => JSON.parse(JSON.stringify(v));
const CID = '00000000-0000-4000-8000-000000000000';
const ORIGIN = 'https://vertexaisearch.cloud.google';
const target = { origin: ORIGIN, location: 'us', cid: CID, csesidx: '123', homeUrl: `${ORIGIN}/us/home/cid/${CID}?csesidx=123` };
const XSRF = Buffer.from('made-up-hmac-key-bytes').toString('base64url');
const SESSION = 'projects/1/locations/us/collections/default/engines/e/sessions/made-up-session-77';
const API = 'https://content-us-discoveryengine.googleapis.com/v1alpha/locations/us';

const obj = (...texts) => ({
  streamAssistResponse: { answer: { replies: texts.map((t) => ({ groundedContent: { content: typeof t === 'string' ? { text: t } : t } })) } }
});

// ---------- parser ----------

const SAMPLE_OBJS = [
  obj('Hel'),
  { sessionInfo: { note: 'brace } and "quote" and \\ in string {' } },
  obj('lo ', 'wor\n"ld"')
];
const SAMPLE = '[' + SAMPLE_OBJS.map((o) => JSON.stringify(o)).join(',\n') + '\n]';

test('parser: one object', () => {
  const p = lib.createAssistStreamParser();
  assert.deepEqual(plain(p.push(JSON.stringify(obj('a')))), [obj('a')]);
});

test('parser: array split at every character boundary', () => {
  for (let i = 0; i <= SAMPLE.length; i++) {
    const p = lib.createAssistStreamParser();
    const got = [...p.push(SAMPLE.slice(0, i)), ...p.push(SAMPLE.slice(i))];
    assert.deepEqual(plain(got), SAMPLE_OBJS, `split at ${i}`);
  }
});

test('parser: one char at a time', () => {
  const p = lib.createAssistStreamParser();
  const got = [];
  for (const ch of SAMPLE) got.push(...p.push(ch));
  assert.deepEqual(plain(got), SAMPLE_OBJS);
});

test('parser: several objects in one chunk, commas, whitespace, trailing bracket', () => {
  const p = lib.createAssistStreamParser();
  const got = p.push(' \n[ ,{"a":1} , ,\r\n{"b":{"c":2}},{"d":[1,{"e":3}]}\n ] ');
  assert.deepEqual(plain(got), [{ a: 1 }, { b: { c: 2 } }, { d: [1, { e: 3 }] }]);
  assert.deepEqual(plain(p.push(']')), []);
});

// ---------- replies / deltas ----------

test('extractReplies handles shapes defensively', () => {
  assert.deepEqual(plain(lib.extractReplies({})), []);
  assert.deepEqual(plain(lib.extractReplies(null)), []);
  assert.deepEqual(plain(lib.extractReplies(obj('x', { thought: true, text: 'why' }, { text: '' }))), [
    { index: 0, text: 'x', thought: false },
    { index: 1, text: 'why', thought: true }
  ]);
});

test('delta tracker: delta text, cumulative text, thought slots independent', () => {
  let t = lib.createDeltaTracker();
  assert.deepEqual([t({ index: 0, text: 'Hel', thought: false }), t({ index: 0, text: 'lo', thought: false })], ['Hel', 'lo']);
  t = lib.createDeltaTracker();
  assert.deepEqual(
    [t({ index: 0, text: 'Hel', thought: false }), t({ index: 0, text: 'Hello', thought: false }),
      t({ index: 0, text: 'Hello wor', thought: false }), t({ index: 0, text: 'Hello wor', thought: false })],
    ['Hel', 'lo', 'ld'.slice(0, 0) + ' wor', '']
  );
  t = lib.createDeltaTracker();
  assert.equal(t({ index: 0, text: 'ab', thought: true }), 'ab');
  assert.equal(t({ index: 0, text: 'ab', thought: false }), 'ab');
});

// ---------- client ----------

const res = (status, body, chunks) => ({
  ok: status >= 200 && status < 300,
  status,
  statusText: String(status),
  text: async () => (typeof body === 'string' ? body : JSON.stringify(body)),
  json: async () => (typeof body === 'string' ? JSON.parse(body) : body),
  body: chunks
    ? { getReader: () => { let i = 0; const enc = new TextEncoder(); return { read: async () => (i < chunks.length ? { done: false, value: enc.encode(chunks[i++]) } : { done: true }) }; } }
    : undefined
});

function harness({ stream, session, auth, lookup, download } = {}) {
  const calls = [];
  let authN = 0;
  const fetch = async (url, opts = {}) => {
    calls.push({ url, opts, body: opts.body ? JSON.parse(opts.body) : undefined });
    if (url.includes('/auth/getoxsrf')) {
      authN++;
      return auth ? auth(authN) : res(200, `)]}'\n${JSON.stringify({ xsrfToken: XSRF, keyId: `key-${authN}` })}`);
    }
    if (url.endsWith('widgetCreateSession')) return session ? session(calls) : res(200, { session: { name: SESSION } });
    if (url.endsWith('widgetStreamAssist')) return stream(calls.filter((c) => c.url.endsWith('widgetStreamAssist')).length);
    if (url.endsWith('lookupWidgetConfig')) return lookup ? lookup() : res(200, { engineResourceDetails: { projectNumber: '4242' } });
    if (url.includes(':downloadFile?')) return download(url, opts);
    if (url.endsWith('widgetAddContextFile')) {
      const n = calls.filter((c) => c.url.endsWith('widgetAddContextFile')).length;
      return res(200, { addContextFileResponse: { session: SESSION, fileId: `file-${n}` } });
    }
    throw new Error(`unexpected ${url}`);
  };
  const logs = [];
  const mk = (l) => (msg, data) => logs.push({ l, msg, data });
  let clock = 1700000000000;
  const client = lib.createGeminiClient({
    target, fetch, origin: ORIGIN, now: () => clock, timeZone: 'Europe/Test',
    log: { debug: mk('debug'), info: mk('info'), warn: mk('warn'), error: mk('error') }
  });
  return { client, calls, logs, tick: (ms) => { clock += ms; } };
}

const STREAM_CHUNKS = [
  '[' + JSON.stringify(obj({ thought: true, text: 'thinking' })) + ',',
  JSON.stringify(obj('Hel')).slice(0, 20),
  JSON.stringify(obj('Hel')).slice(20) + ',' + JSON.stringify(obj('lo')) + ']'
];

test('client happy path: auth, jwt, createSession, streamAssist with ordered deltas and exact bodies', async () => {
  const h = harness({ stream: () => res(200, '', STREAM_CHUNKS) });
  const session = await h.client.createSession();
  assert.equal(session, SESSION);
  const events = [];
  const out = await h.client.streamAssist(session, 'hi there', {
    onThought: (d) => events.push(['t', d]),
    onText: (d) => events.push(['x', d])
  });
  assert.deepEqual(events, [['t', 'thinking'], ['x', 'Hel'], ['x', 'lo']]);
  assert.equal(out.text, 'Hello');

  assert.deepEqual(h.calls.map((c) => c.url), [`${ORIGIN}/auth/getoxsrf?csesidx=123`, `${API}/widgetCreateSession`, `${API}/widgetStreamAssist`]);
  assert.equal(h.calls[0].opts.credentials, 'include');
  const hdr = h.calls[1].opts.headers;
  assert.equal(hdr['Content-Type'], 'application/json');
  assert.equal(hdr['X-Goog-Api-Client'], 'gemini-enterprise-classic/1.0.0');
  assert.match(hdr.Authorization, /^Bearer [\w-]+\.[\w-]+\.[\w-]+$/);
  const [hd, pl] = hdr.Authorization.slice(7).split('.').slice(0, 2).map((s) => JSON.parse(Buffer.from(s, 'base64url').toString()));
  assert.deepEqual(hd, { alg: 'HS256', typ: 'JWT', kid: 'key-1' });
  assert.deepEqual(pl, {
    iss: ORIGIN, aud: 'https://content-us-discoveryengine.googleapis.com', sub: 'csesidx/123',
    iat: 1700000000, exp: 1700000300, nbf: 1700000000
  });
  assert.deepEqual(h.calls[1].body, {
    configId: CID, additionalParams: { token: '-', origin: 'ORIGIN_UNSPECIFIED' },
    createSessionRequest: { session: { name: '-', displayName: '' } }
  });
  assert.deepEqual(h.calls[2].body, {
    configId: CID, additionalParams: { token: '-', origin: 'ORIGIN_UNSPECIFIED' },
    streamAssistRequest: {
      session: SESSION, query: { parts: [{ text: 'hi there' }] }, filter: '', fileIds: [],
      answerGenerationMode: 'NORMAL', agentsSpec: {}, languageCode: 'en-US',
      userMetadata: { timeZone: 'Europe/Test' }, assistSkippingMode: 'REQUEST_ASSIST'
    }
  });
  assert.ok(!h.calls[2].opts.body.includes('dataStoreSpecs'));
  assert.ok(!h.calls[2].opts.body.includes('toolsSpec'));

  const blob = JSON.stringify([h.logs, out]);
  for (const secret of [XSRF, hdr.Authorization.slice(7), SESSION, 'made-up-session-77', 'Bearer']) {
    assert.ok(!blob.includes(secret), `leaks ${secret}`);
  }
  assert.ok(h.logs.length > 0);
});

test('client: jwt cached across turns, refreshed after 270 s', async () => {
  const h = harness({ stream: () => res(200, JSON.stringify([obj('ok')])) });
  await lib.runApiTurn(h.client, null, 'one');
  await lib.runApiTurn(h.client, SESSION, 'two');
  assert.equal(h.calls.filter((c) => c.url.includes('getoxsrf')).length, 1);
  h.tick(271000);
  await lib.runApiTurn(h.client, SESSION, 'three');
  assert.equal(h.calls.filter((c) => c.url.includes('getoxsrf')).length, 2);
});

test('client: 401 on streamAssist re-auths once then succeeds', async () => {
  const h = harness({ stream: (n) => (n === 1 ? res(401, 'expired') : res(200, JSON.stringify([obj('fine')]))) });
  const out = await lib.runApiTurn(h.client, SESSION, 'q');
  assert.equal(out.text, 'fine');
  assert.equal(h.calls.filter((c) => c.url.includes('getoxsrf')).length, 2);
  assert.equal(h.calls.filter((c) => c.url.endsWith('widgetStreamAssist')).length, 2);
  const kids = h.calls.filter((c) => c.url.endsWith('widgetStreamAssist'))
    .map((c) => JSON.parse(Buffer.from(c.opts.headers.Authorization.slice(7).split('.')[0], 'base64url').toString()).kid);
  assert.deepEqual(kids, ['key-1', 'key-2']);
});

test('client: second 401 is surfaced with step and status', async () => {
  const h = harness({ stream: () => res(401, 'nope') });
  await assert.rejects(() => h.client.streamAssist(SESSION, 'q'), (e) => e.httpStatus === 401 && e.step === 'stream');
  assert.equal(h.calls.filter((c) => c.url.includes('getoxsrf')).length, 2);
  assert.equal(h.calls.filter((c) => c.url.endsWith('widgetStreamAssist')).length, 2);
});

test('client: auth failure and network failure are tagged', async () => {
  let h = harness({ auth: () => res(403, 'forbidden'), stream: () => res(200, '[]') });
  await assert.rejects(() => h.client.createSession(), (e) => e.step === 'auth' && e.httpStatus === 403);
  const client = lib.createGeminiClient({ target, origin: ORIGIN, fetch: async () => { throw new TypeError('Failed to fetch'); } });
  await assert.rejects(() => client.ensureJwt(), (e) => e.step === 'auth' && e.network === true);
});

test('runApiTurn: 404 on a reused session retries on a fresh one', async () => {
  const h = harness({ stream: (n) => (n === 1 ? res(404, 'gone') : res(200, JSON.stringify([obj('again')]))) });
  const out = await lib.runApiTurn(h.client, 'old-session', 'q');
  assert.equal(out.text, 'again');
  assert.equal(out.session, SESSION);
  assert.equal(h.calls.filter((c) => c.url.endsWith('widgetCreateSession')).length, 1);
});

test('runApiTurn uploads attachments into the session and sends their fileIds', async () => {
  const h = harness({ stream: () => res(200, JSON.stringify([obj('a cat')])) });
  const out = await lib.runApiTurn(h.client, null, 'what is this', {}, { attachments: ['data:image/png;base64,QUJD', 'data:image/jpeg;base64,REVG'] });
  assert.equal(out.text, 'a cat');
  const ups = h.calls.filter((c) => c.url.endsWith('widgetAddContextFile')).map((c) => c.body.addContextFileRequest);
  assert.deepEqual(plain(ups), [
    { name: SESSION, fileName: 'image-1.png', mimeType: 'image/png', fileContents: 'QUJD' },
    { name: SESSION, fileName: 'image-2.jpeg', mimeType: 'image/jpeg', fileContents: 'REVG' }
  ]);
  const req = h.calls.find((c) => c.url.endsWith('widgetStreamAssist')).body.streamAssistRequest;
  assert.deepEqual(plain(req.fileIds), ['file-1', 'file-2']);
  assert.ok(!('toolsSpec' in req), 'attachments do not turn the image tool on');
});

test('addContextFile failures carry step upload', async () => {
  const h = harness({ stream: () => res(200, '[]') });
  await assert.rejects(() => h.client.addContextFile(SESSION, 'not-a-data-url', 'image-1.png'), (e) => e.step === 'upload');
  const noId = lib.createGeminiClient({
    target, origin: ORIGIN,
    fetch: async (url) => (url.includes('/auth/getoxsrf')
      ? res(200, `)]}'
${JSON.stringify({ xsrfToken: XSRF, keyId: 'k' })}`)
      : res(200, {}))
  });
  await assert.rejects(() => noId.addContextFile(SESSION, 'data:image/png;base64,QUJD', 'image-1.png'), (e) => e.step === 'upload');
});

test('runApiTurn without attachments uploads nothing and sends empty fileIds', async () => {
  const h = harness({ stream: () => res(200, JSON.stringify([obj('ok')])) });
  await lib.runApiTurn(h.client, SESSION, 'q');
  assert.equal(h.calls.filter((c) => c.url.endsWith('widgetAddContextFile')).length, 0);
  assert.deepEqual(plain(h.calls.find((c) => c.url.endsWith('widgetStreamAssist')).body.streamAssistRequest.fileIds), []);
});

test('streamAssist sends assistGenerationConfig.modelId only for a concrete model', async () => {
  const h = harness({ stream: () => res(200, JSON.stringify([obj('ok')])) });
  await lib.runApiTurn(h.client, SESSION, 'a', {}, { modelId: 'gemini-3.5-flash' });
  await lib.runApiTurn(h.client, SESSION, 'b', {}, { modelId: 'auto' });
  await lib.runApiTurn(h.client, SESSION, 'c');
  const reqs = h.calls.filter((c) => c.url.endsWith('widgetStreamAssist')).map((c) => c.body.streamAssistRequest);
  assert.deepEqual(reqs[0].assistGenerationConfig, { modelId: 'gemini-3.5-flash' });
  assert.ok(!('assistGenerationConfig' in reqs[1]));
  assert.ok(!('assistGenerationConfig' in reqs[2]));
});

test('toApiModelId maps picker labels to the ids the web UI sends', () => {
  assert.equal(lib.toApiModelId('Gemini 4 Carbon'), 'gemini-4-carbon');
  assert.equal(lib.toApiModelId('Gemini 3.5 Flash'), 'gemini-3.5-flash');
  assert.equal(lib.toApiModelId('Gemini 3.1 Pro · แสดงตัวอย่าง'), 'gemini-3.1-pro-preview');
  assert.equal(lib.toApiModelId('Gemini 3.1 Pro · Preview'), 'gemini-3.1-pro-preview');
  assert.equal(lib.toApiModelId('Auto · We choose the best fit · check'), 'auto');
  assert.equal(lib.toApiModelId(''), '');
});

// ---------- image generation ----------

const PNG_B64 = Buffer.from('\x89PNG\r\n\x1a\nfake-image-bytes', 'latin1').toString('base64');
const IMG_SESSION = 'collections/default_collection/engines/e/sessions/777';
const imageStream = (withFile = true) => () => res(200, '', [JSON.stringify([
  obj('Image generated.'),
  ...(withFile ? [obj({ file: { mimeType: 'image/png', fileId: 'file-1' } })] : [])
])]);

test('extractFiles and imagePrompt', () => {
  assert.deepEqual(plain(lib.extractFiles(obj('x', { file: { mimeType: 'image/png', fileId: 'f' } }))), [{ fileId: 'f', mimeType: 'image/png' }]);
  assert.deepEqual(plain(lib.extractFiles({})), []);
  assert.equal(lib.imagePrompt(' a red apple ', '16:9'), 'Generate an image: a red apple\nAspect ratio: landscape (16:9).');
  assert.equal(lib.imagePrompt('cube', undefined), 'Generate an image: cube');
});

test('runImageGeneration: image tool on, project looked up once, base64 download with the browser-safe header', async () => {
  const h = harness({
    session: () => res(200, { session: { name: IMG_SESSION } }),
    stream: imageStream(),
    download: () => res(200, PNG_B64 + '\n')
  });
  const out = await lib.runImageGeneration(h.client, 'a red apple', '1:1');
  assert.equal(out.mime, 'image/png');
  assert.equal(out.b64, PNG_B64);
  const req = h.calls.find((c) => c.url.endsWith('widgetStreamAssist')).body.streamAssistRequest;
  assert.deepEqual(req.toolsSpec, { imageGenerationSpec: {} });
  assert.match(req.query.parts[0].text, /^Generate an image: a red apple/);
  const dl = h.calls.find((c) => c.url.includes(':downloadFile?'));
  assert.equal(dl.url, `https://content-us-discoveryengine.googleapis.com/v1alpha/projects/4242/locations/us/${IMG_SESSION}:downloadFile?fileId=file-1&alt=media`);
  assert.equal(dl.opts.headers['X-Goog-Encode-Response-If-Executable'], 'base64');
  assert.match(dl.opts.headers.Authorization, /^Bearer /);
  await lib.runImageGeneration(h.client, 'again');
  assert.equal(h.calls.filter((c) => c.url.endsWith('lookupWidgetConfig')).length, 1);
  assert.ok(!JSON.stringify(h.logs).includes(PNG_B64), 'image bytes must not be logged');
});

test('runImageGeneration: no image in the answer rejects with step image', async () => {
  const h = harness({ stream: imageStream(false), download: () => { throw new Error('no download expected'); } });
  await assert.rejects(() => lib.runImageGeneration(h.client, 'x'), (e) => e.step === 'image' && /no image/.test(e.message));
});

test('plain chat turns do not turn the image tool on', async () => {
  const h = harness({ stream: imageStream(false) });
  await lib.runApiTurn(h.client, SESSION, 'hi');
  assert.ok(!('toolsSpec' in h.calls.find((c) => c.url.endsWith('widgetStreamAssist')).body.streamAssistRequest));
});
