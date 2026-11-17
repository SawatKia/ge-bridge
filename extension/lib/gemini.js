// Pure / injectable Gemini Enterprise helpers shared by background.js (importScripts)
// and content.js (content script). Classic script: re-injection safe via `var` + reuse.
var GeBridgeLib = globalThis.GeBridgeLib || (() => {
  const GEMINI_HOST_PATTERNS = [
    'https://vertexaisearch.cloud.google/*',
    'https://vertexaisearch.cloud.google.com/*'
  ];
  const GEMINI_FALLBACK_URL = 'https://vertexaisearch.cloud.google/';
  const GEMINI_URL_RE = /^https:\/\/vertexaisearch\.cloud\.google(\.com)?\/(?:([a-z0-9-]+)\/)?home\/cid\/([0-9a-f-]{36})/;

  function isGeminiUrl(url) {
    return typeof url === 'string' && GEMINI_HOST_PATTERNS.some((p) => url.startsWith(p.slice(0, -1)));
  }

  function parseGeminiUrl(raw) {
    let u;
    try { u = new URL(String(raw).trim()); } catch { return null; }
    const m = GEMINI_URL_RE.exec(u.origin + u.pathname);
    const csesidx = u.searchParams.get('csesidx') || '';
    if (!m || !/^\d+$/.test(csesidx)) return null;
    const location = m[2] || 'us';
    return {
      origin: u.origin,
      location,
      cid: m[3],
      csesidx,
      homeUrl: `${m[0]}?csesidx=${csesidx}`
    };
  }

  function base64urlEncode(bytes) {
    let bin = '';
    for (const b of bytes) bin += String.fromCharCode(b);
    return btoa(bin).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
  }

  function base64urlDecode(str) {
    const b64 = str.replace(/-/g, '+').replace(/_/g, '/');
    const bin = atob(b64 + '='.repeat((4 - (b64.length % 4)) % 4));
    return Uint8Array.from(bin, (c) => c.charCodeAt(0));
  }

  function stripXssi(text) {
    return text.replace(/^\)\]\}'\s*/, '');
  }

  function geminiJwtClaims(origin, region, csesidx, now) {
    return {
      iss: origin,
      aud: `https://content-${region}-discoveryengine.googleapis.com`,
      sub: `csesidx/${csesidx}`,
      iat: now,
      exp: now + 300,
      nbf: now
    };
  }

  async function signJwtHS256(secretBytes, header, payload, cryptoImpl = globalThis.crypto) {
    const enc = new TextEncoder();
    const input = `${base64urlEncode(enc.encode(JSON.stringify(header)))}.${base64urlEncode(enc.encode(JSON.stringify(payload)))}`;
    const key = await cryptoImpl.subtle.importKey('raw', secretBytes, { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
    const sig = new Uint8Array(await cryptoImpl.subtle.sign('HMAC', key, enc.encode(input)));
    return `${input}.${base64urlEncode(sig)}`;
  }

  async function httpFail(res) {
    const body = await res.text().catch(() => '');
    const err = new Error(body.slice(0, 200) || res.statusText || `HTTP ${res.status}`);
    err.httpStatus = res.status;
    return err;
  }

  function timeoutSignal(ms) {
    return typeof AbortSignal !== 'undefined' && AbortSignal.timeout ? AbortSignal.timeout(ms) : undefined;
  }

  // deps: { origin, fetch, crypto, now }; now() returns epoch milliseconds.
  async function runGeminiProbe(target, deps = {}) {
    const fetchImpl = deps.fetch || globalThis.fetch;
    const cryptoImpl = deps.crypto || globalThis.crypto;
    const now = deps.now || (() => Date.now());
    const origin = deps.origin ?? globalThis.location?.origin;

    const t0 = now();
    const steps = [];
    const ms = (from) => Math.round(now() - from);
    const finish = () => ({ ok: steps.length > 0 && steps.every((s) => s.ok), ms: ms(t0), steps });

    if (!target) {
      steps.push({
        step: 'target',
        ok: false,
        ms: 0,
        error: 'No cid/csesidx in this tab URL. Open your Gemini Enterprise home page first.'
      });
      return finish();
    }

    const state = {};
    const run = async (name, fn) => {
      const s0 = now();
      try {
        const extra = (await fn()) || {};
        steps.push({ step: name, ok: true, ms: ms(s0), ...extra });
        return true;
      } catch (err) {
        steps.push({ step: name, ok: false, ms: ms(s0), httpStatus: err.httpStatus, error: String(err.message).slice(0, 200) });
        return false;
      }
    };

    (await run('auth', async () => {
      const res = await fetchImpl(`${origin}/auth/getoxsrf?csesidx=${target.csesidx}`, {
        credentials: 'include',
        signal: timeoutSignal(15000)
      });
      if (!res.ok) throw await httpFail(res);
      let data;
      try { data = JSON.parse(stripXssi(await res.text())); } catch { throw new Error('getoxsrf response is not JSON'); }
      if (!data?.xsrfToken || data.keyId == null) throw new Error('getoxsrf response missing xsrfToken/keyId');
      state.xsrf = data.xsrfToken;
      state.keyId = String(data.keyId);
    }))
      && (await run('jwt', async () => {
        const claims = geminiJwtClaims(origin, target.location, target.csesidx, Math.floor(now() / 1000));
        state.jwt = await signJwtHS256(base64urlDecode(state.xsrf), { alg: 'HS256', typ: 'JWT', kid: state.keyId }, claims, cryptoImpl);
      }))
      && (await run('session', async () => {
        const api = `https://content-${target.location}-discoveryengine.googleapis.com`;
        const res = await fetchImpl(`${api}/v1alpha/locations/${target.location}/widgetCreateSession`, {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
            Authorization: `Bearer ${state.jwt}`,
            'X-Goog-Api-Client': 'gemini-enterprise-classic/1.0.0'
          },
          body: JSON.stringify({
            configId: target.cid,
            additionalParams: { token: '-', origin: 'ORIGIN_UNSPECIFIED' },
            createSessionRequest: { session: { name: '-', displayName: '' } }
          }),
          signal: timeoutSignal(15000)
        });
        if (!res.ok) throw await httpFail(res);
        const data = await res.json().catch(() => null);
        if (!data?.session?.name) throw new Error('widgetCreateSession response has no session.name');
        return { hasSession: true };
      }));

    return finish();
  }

  // ---------- chat over the Gemini Enterprise web API ----------

  const JWT_CACHE_MS = 270000;
  const API_CLIENT_HEADER = 'gemini-enterprise-classic/1.0.0';
  const NOOP_LOG = { debug() {}, info() {}, warn() {}, error() {} };

  // Incremental parser for a streamed JSON array of objects. push(chunk) returns the
  // objects completed by that chunk. Skips the framing: '[', ']', ',' and whitespace.
  function createAssistStreamParser() {
    let buf = '';
    let pos = 0;      // next char to examine
    let start = -1;   // start of the object being scanned, -1 when between objects
    let depth = 0;
    let inStr = false;
    let esc = false;
    function push(chunk) {
      buf += chunk;
      const out = [];
      while (pos < buf.length) {
        const c = buf[pos];
        if (start < 0) {
          if (c === '{') { start = pos; depth = 1; inStr = false; esc = false; }
          pos++;
          continue;
        }
        if (inStr) {
          if (esc) esc = false;
          else if (c === '\\') esc = true;
          else if (c === '"') inStr = false;
        } else if (c === '"') inStr = true;
        else if (c === '{') depth++;
        else if (c === '}' && --depth === 0) {
          const raw = buf.slice(start, pos + 1);
          try { out.push(JSON.parse(raw)); } catch { /* skip a malformed object */ }
          start = -1;
        }
        pos++;
      }
      if (start < 0) { buf = ''; pos = 0; }
      else if (start > 0) { buf = buf.slice(start); pos -= start; start = 0; }
      return out;
    }
    return { push };
  }

  // Pure: pulls [{index, text, thought}] out of one streamed object.
  function extractReplies(obj) {
    const replies = obj && obj.streamAssistResponse && obj.streamAssistResponse.answer
      && obj.streamAssistResponse.answer.replies;
    if (!Array.isArray(replies)) return [];
    const out = [];
    replies.forEach((r, index) => {
      const c = r && r.groundedContent && r.groundedContent.content;
      if (c && typeof c.text === 'string' && c.text) out.push({ index, text: c.text, thought: c.thought === true });
    });
    return out;
  }

  // Pure: generated files ([{fileId, mimeType}]) in one streamed object, e.g. an image from Gemini's image tool.
  function extractFiles(obj) {
    const replies = obj && obj.streamAssistResponse && obj.streamAssistResponse.answer
      && obj.streamAssistResponse.answer.replies;
    if (!Array.isArray(replies)) return [];
    const out = [];
    for (const r of replies) {
      const f = r && r.groundedContent && r.groundedContent.content && r.groundedContent.content.file;
      if (f && typeof f.fileId === 'string' && f.fileId) out.push({ fileId: f.fileId, mimeType: String(f.mimeType || '') });
    }
    return out;
  }

  // Pure: the prompt sent to Gemini's image tool. aspect is '1:1' | '16:9' | '9:16' (anything else: no hint).
  const ASPECT_HINTS = { '1:1': 'square (1:1)', '16:9': 'landscape (16:9)', '9:16': 'portrait (9:16)' };
  function imagePrompt(prompt, aspect) {
    const hint = ASPECT_HINTS[aspect];
    return `Generate an image: ${String(prompt).trim()}${hint ? `\nAspect ratio: ${hint}.` : ''}`;
  }

  // Turns replies into deltas. If a text strictly extends what was already emitted for the
  // same (reply index, thought) slot it is treated as cumulative and only the suffix is
  // emitted; otherwise the text is a delta. Once a slot proved cumulative it stays so.
  function createDeltaTracker() {
    const slots = new Map();
    return function next(reply) {
      const key = `${reply.thought ? 't' : 'a'}${reply.index}`;
      const slot = slots.get(key) || { emitted: '', cumulative: false };
      slots.set(key, slot);
      let delta = reply.text;
      if (slot.emitted && reply.text.length > slot.emitted.length && reply.text.startsWith(slot.emitted)) {
        slot.cumulative = true;
        delta = reply.text.slice(slot.emitted.length);
        slot.emitted = reply.text;
      } else if (slot.cumulative && slot.emitted.startsWith(reply.text)) {
        delta = '';
      } else {
        slot.emitted += reply.text;
      }
      return delta;
    };
  }

  // deps: { target, fetch, crypto, now, origin, log, timeZone }
  function createGeminiClient(deps = {}) {
    const target = deps.target;
    const fetchImpl = deps.fetch || globalThis.fetch;
    const cryptoImpl = deps.crypto || globalThis.crypto;
    const now = deps.now || (() => Date.now());
    const origin = deps.origin ?? globalThis.location?.origin;
    const log = deps.log || NOOP_LOG;
    if (!target) throw new Error('No Gemini target (cid/csesidx) known');
    const api = `https://content-${target.location}-discoveryengine.googleapis.com`;
    const apiBase = `${api}/v1alpha/locations/${target.location}`;
    let jwt = null;
    let jwtAt = 0;

    const fail = (step, err) => {
      if (!err.step) err.step = step;
      return err;
    };
    // fetch() itself throwing (not an HTTP status) is a network/CORS/policy failure
    async function doFetch(step, url, opts) {
      try {
        return await fetchImpl(url, opts);
      } catch (err) {
        const e = new Error(String(err && err.message || err).slice(0, 200));
        e.network = true;
        throw fail(step, e);
      }
    }

    async function ensureJwt(force = false) {
      if (!force && jwt && now() - jwtAt < JWT_CACHE_MS) return jwt;
      jwt = null;
      const t0 = now();
      const res = await doFetch('auth', `${origin}/auth/getoxsrf?csesidx=${target.csesidx}`, {
        credentials: 'include',
        signal: timeoutSignal(15000)
      });
      if (!res.ok) {
        const e = await httpFail(res);
        log.warn('api auth failed', { httpStatus: e.httpStatus, ms: Math.round(now() - t0) });
        throw fail('auth', e);
      }
      let data;
      try { data = JSON.parse(stripXssi(await res.text())); } catch { throw fail('auth', new Error('getoxsrf response is not JSON')); }
      if (!data?.xsrfToken || data.keyId == null) throw fail('auth', new Error('getoxsrf response missing xsrfToken/keyId'));
      log.info('api auth ok', { httpStatus: res.status, ms: Math.round(now() - t0) });
      try {
        const claims = geminiJwtClaims(origin, target.location, target.csesidx, Math.floor(now() / 1000));
        jwt = await signJwtHS256(base64urlDecode(data.xsrfToken), { alg: 'HS256', typ: 'JWT', kid: String(data.keyId) }, claims, cryptoImpl);
      } catch (err) {
        throw fail('jwt', err);
      }
      jwtAt = now();
      log.info('api jwt ok', {});
      return jwt;
    }

    // POST with one re-auth retry on 401/403. Returns the ok Response.
    async function post(step, path, body) {
      for (let attempt = 0; ; attempt++) {
        const token = await ensureJwt();
        const t0 = now();
        const res = await doFetch(step, `${apiBase}/${path}`, {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
            Authorization: `Bearer ${token}`,
            'X-Goog-Api-Client': API_CLIENT_HEADER
          },
          body: JSON.stringify(body),
          signal: timeoutSignal(step === 'stream' ? 300000 : step === 'upload' ? 120000 : 30000)
        });
        if (res.ok) {
          log.info(`api ${step} response`, { httpStatus: res.status, ms: Math.round(now() - t0) });
          return res;
        }
        const e = await httpFail(res);
        log.warn(`api ${step} failed`, { httpStatus: e.httpStatus, attempt, error: e.message });
        if ((res.status === 401 || res.status === 403) && attempt === 0) {
          jwt = null;
          log.info('api dropping cached jwt, re-authenticating', { step });
          continue;
        }
        throw fail(step, e);
      }
    }

    const baseBody = () => ({ configId: target.cid, additionalParams: { token: '-', origin: 'ORIGIN_UNSPECIFIED' } });

    async function createSession() {
      const res = await post('session', 'widgetCreateSession', {
        ...baseBody(),
        createSessionRequest: { session: { name: '-', displayName: '' } }
      });
      const data = await res.json().catch(() => null);
      const name = data?.session?.name;
      if (!name) throw fail('session', new Error('widgetCreateSession response has no session.name'));
      return name;
    }

    // callbacks: { onThought(delta), onText(delta) }; resolves { text } (answer text only)
    // opts.modelId: API model id (e.g. 'gemini-3.5-flash'); omitted or 'auto' lets Gemini choose.
    async function streamAssist(session, prompt, callbacks = {}, opts = {}) {
      let tz = deps.timeZone;
      if (!tz) { try { tz = Intl.DateTimeFormat().resolvedOptions().timeZone; } catch { tz = 'UTC'; } }
      const res = await post('stream', 'widgetStreamAssist', {
        ...baseBody(),
        streamAssistRequest: {
          session,
          query: { parts: [{ text: prompt }] },
          filter: '',
          fileIds: opts.fileIds || [],
          answerGenerationMode: 'NORMAL',
          agentsSpec: {},
          languageCode: 'en-US',
          userMetadata: { timeZone: tz || 'UTC' },
          assistSkippingMode: 'REQUEST_ASSIST',
          ...(opts.images ? { toolsSpec: { imageGenerationSpec: {} } } : {}),
          ...(opts.modelId && opts.modelId !== 'auto' ? { assistGenerationConfig: { modelId: opts.modelId } } : {})
        }
      });
      const parser = createAssistStreamParser();
      const track = createDeltaTracker();
      let text = '';
      let objects = 0;
      let thoughtChars = 0;
      const files = [];
      const handle = (chunk) => {
        for (const obj of parser.push(chunk)) {
          objects++;
          for (const f of extractFiles(obj)) if (!files.some((x) => x.fileId === f.fileId)) files.push(f);
          for (const reply of extractReplies(obj)) {
            const delta = track(reply);
            if (!delta) continue;
            if (reply.thought) {
              thoughtChars += delta.length;
              if (callbacks.onThought) callbacks.onThought(delta);
            } else {
              text += delta;
              if (callbacks.onText) callbacks.onText(delta);
            }
          }
        }
      };
      try {
        if (res.body && typeof res.body.getReader === 'function') {
          const reader = res.body.getReader();
          const decoder = new TextDecoder();
          for (;;) {
            const { done, value } = await reader.read();
            if (done) break;
            handle(decoder.decode(value, { stream: true }));
          }
          handle(decoder.decode());
        } else {
          handle(await res.text());
        }
      } catch (err) {
        throw fail('stream', err);
      }
      log.info('api stream finished', { objects, answerChars: text.length, thoughtChars, files: files.length });
      if (!text && !objects) throw fail('stream', new Error('widgetStreamAssist returned no parseable objects'));
      return { text, files };
    }

    // The tenant's project number; download URLs need it and session names leave it out.
    let projectNumber = null;
    async function getProjectNumber() {
      if (projectNumber) return projectNumber;
      const res = await post('project', 'lookupWidgetConfig', { widgetConfigId: target.cid });
      const data = await res.json().catch(() => null);
      const n = data && data.engineResourceDetails && data.engineResourceDetails.projectNumber;
      if (!n || !/^\d+$/.test(String(n))) throw fail('project', new Error('lookupWidgetConfig response has no projectNumber'));
      projectNumber = String(n);
      return projectNumber;
    }

    // Downloads a generated file as base64. The API refuses raw media to a browser origin
    // ("Request unsafe for browser client domain") unless asked to base64-encode the body.
    async function downloadFile(session, fileId) {
      const project = await getProjectNumber();
      const url = `${api}/v1alpha/projects/${project}/locations/${target.location}/${session}:downloadFile?fileId=${encodeURIComponent(fileId)}&alt=media`;
      for (let attempt = 0; ; attempt++) {
        const token = await ensureJwt();
        const t0 = now();
        const res = await doFetch('download', url, {
          headers: {
            Authorization: `Bearer ${token}`,
            'X-Goog-Api-Client': API_CLIENT_HEADER,
            'X-Goog-Encode-Response-If-Executable': 'base64'
          },
          signal: timeoutSignal(60000)
        });
        if (res.ok) {
          const b64 = (await res.text()).replace(/\s+/g, '');
          log.info('api download response', { httpStatus: res.status, ms: Math.round(now() - t0), b64Chars: b64.length });
          if (!b64 || b64.length % 4 || !/^[A-Za-z0-9+/]+={0,2}$/.test(b64)) throw fail('download', new Error('downloadFile did not return base64 data'));
          return b64;
        }
        const e = await httpFail(res);
        log.warn('api download failed', { httpStatus: e.httpStatus, attempt, error: e.message });
        if ((res.status === 401 || res.status === 403) && attempt === 0) { jwt = null; continue; }
        throw fail('download', e);
      }
    }

    // Uploads one `data:<mime>;base64,` image into `session` (what the web UI's attach button does);
    // resolves the fileId that a later streamAssist passes in `fileIds`.
    async function addContextFile(session, dataUrl, fileName) {
      const m = /^data:([^;,]+);base64,(.*)$/s.exec(dataUrl || '');
      if (!m) throw fail('upload', new Error('attachment is not a base64 data URL'));
      const res = await post('upload', 'widgetAddContextFile', {
        ...baseBody(),
        addContextFileRequest: { name: session, fileName, mimeType: m[1], fileContents: m[2] }
      });
      const data = await res.json().catch(() => null);
      const fileId = data?.addContextFileResponse?.fileId;
      if (!fileId) throw fail('upload', new Error('widgetAddContextFile response has no fileId'));
      log.info('api file uploaded', { mimeType: m[1], b64Chars: m[2].length });
      return fileId;
    }

    return { ensureJwt, createSession, streamAssist, downloadFile, addContextFile };
  }

  // One chat turn: reuses `session` when given, else creates one. A 404 on a reused session
  // (expired/deleted) gets one retry on a fresh session. Resolves { text, session }.
  // opts.attachments: data URLs uploaded into the session first and sent as the turn's fileIds.
  async function runApiTurn(client, session, prompt, callbacks = {}, opts = {}) {
    const { attachments = [], ...rest } = opts;
    const turn = async (s) => {
      const fileIds = [];
      for (const [i, url] of attachments.entries()) {
        const ext = (/^data:[^/]+\/([a-z0-9]+)/i.exec(url) || [])[1] || 'bin';
        fileIds.push(await client.addContextFile(s, url, `image-${i + 1}.${ext}`));
      }
      return { ...(await client.streamAssist(s, prompt, callbacks, { ...rest, fileIds })), session: s };
    };
    let s = session || await client.createSession();
    try {
      return await turn(s);
    } catch (err) {
      if (!session || err.httpStatus !== 404) throw err;
      s = await client.createSession();
      return await turn(s);
    }
  }

  // One image: a fresh session with Gemini's image tool on, then the first generated image file.
  // Resolves { mime, b64, text, session }. Rejects with step 'image' when Gemini answers without an image.
  async function runImageGeneration(client, prompt, aspect, opts = {}) {
    const session = await client.createSession();
    const out = await client.streamAssist(session, imagePrompt(prompt, aspect), {}, { images: true, modelId: opts.modelId });
    const file = (out.files || []).find((f) => /^image\/(png|jpeg|webp)$/.test(f.mimeType)) || (out.files || [])[0];
    if (!file) {
      const err = new Error(`Gemini returned no image${out.text ? `: ${out.text.slice(0, 200)}` : ''}`);
      err.step = 'image';
      throw err;
    }
    const b64 = await client.downloadFile(session, file.fileId);
    return { mime: file.mimeType || 'image/png', b64, text: out.text, session };
  }

  function toModelSlug(raw) {
    return String(raw || '')
      .toLowerCase()
      .replace(/\(.*?\)/g, '')
      .replace(/[^a-z0-9.+-]+/g, '-')
      .replace(/^-+|-+$/g, '');
  }

  // Model picker label -> API modelId, as the web UI sends it:
  // "Gemini 4 Carbon" -> "gemini-4-carbon", "Gemini 3.1 Pro · Preview" (any UI language) -> "gemini-3.1-pro-preview".
  function toApiModelId(label) {
    const [name, ...rest] = String(label || '').split(/\s*[·•]\s*/);
    const id = toModelSlug(name);
    if (!id) return '';
    const preview = /preview|แสดงตัวอย่าง/i.test(rest.join(' '));
    return preview && !id.endsWith('-preview') ? `${id}-preview` : id;
  }

  // ---------- Transport order and failure text ----------
  // The page tab goes first in auto mode: from the extension's own service worker Gemini answered 401
  // on every session/stream call in testing (Nov 2026) even with a fresh JWT, while the same calls from
  // the Gemini page always worked. The service worker stays as the fallback for when no tab is open.

  const TRANSPORT_LABELS = Object.freeze({ 'api-tab': 'API from the Gemini tab', 'api-sw': 'API from the extension', dom: 'typing into the Gemini tab' });

  function transportOrder(transport, { kind = 'turn', hasImages = false, hasTarget = true } = {}) {
    if (kind === 'image') return transport === 'api-tab' || transport === 'api-sw' ? [transport] : ['api-tab', 'api-sw'];
    if (transport === 'dom' || !hasTarget) return ['dom'];
    // Typing into the page cannot attach files, so a turn with images has no DOM fallback.
    if (transport === 'auto') return hasImages ? ['api-tab', 'api-sw'] : ['api-tab', 'api-sw', 'dom'];
    return [transport];
  }

  // One message for the harness when every transport failed: each mode's reason, plus what to do.
  // failures: [{ mode, message, httpStatus }]
  function transportFailureMessage(failures, what = 'Turn') {
    if (!failures.length) return `${what} failed`;
    if (failures.length === 1) return failures[0].message || `${what} failed`;
    const reason = (f) => (f.httpStatus ? `HTTP ${f.httpStatus}` : String(f.message || 'failed').split(/\r?\n/)[0].slice(0, 120));
    const parts = failures.map((f) => `${TRANSPORT_LABELS[f.mode] || f.mode}: ${reason(f)}`);
    const noTab = failures.some((f) => /no scriptable gemini tab/i.test(f.message || ''));
    return `${what} failed on every way to reach Gemini (${parts.join('; ')}).`
      + (noTab ? ' Open your Gemini Enterprise page in Chrome, signed in, and try again.' : '');
  }

  // ---------- Waking a Gemini tab ----------
  // Chrome's Memory Saver unloads background tabs (discarded / status 'unloaded'); such a tab cannot
  // run a turn until it loads again.

  function isSleepingTab(t) {
    return Boolean(t && (t.discarded || t.status === 'unloaded'));
  }

  // Which tab to reload when none is scriptable: the preferred one, else a sleeping one, else any Gemini
  // tab on the target's host. null means none is open (open a new one).
  function pickTabToWake(tabs, { preferId = null, host = null } = {}) {
    const hostOfUrl = (u) => { try { return new URL(u).host; } catch { return ''; } };
    const usable = (tabs || []).filter((t) => t && t.url && (!host || hostOfUrl(t.url) === host));
    return usable.find((t) => t.id === preferId) || usable.find(isSleepingTab) || usable[0] || null;
  }

  // ---------- Human pacing ----------
  // Before the next question, wait about as long as a person would take to read the last answer:
  // words / (random 200-250 wpm), plus Gaussian jitter, clamped to [minDelayMs, maxDelayMs].

  const PACING_DEFAULTS = Object.freeze({ enabled: true, wpmMin: 200, wpmMax: 250, minDelayMs: 1500, maxDelayMs: 20000, jitter: 0.15 });
  const PACING_MAX_DELAY_CAP_MS = 10 * 60 * 1000;

  // Settings from storage, with defaults filled in and every number forced into a sane range.
  function normalizePacing(raw) {
    const p = { ...PACING_DEFAULTS, ...(raw && typeof raw === 'object' ? raw : {}) };
    const num = (v, d, lo, hi) => { const n = Number(v); return Number.isFinite(n) ? Math.min(hi, Math.max(lo, n)) : d; };
    const wpmMin = Math.round(num(p.wpmMin, PACING_DEFAULTS.wpmMin, 50, 1000));
    const wpmMax = Math.round(num(p.wpmMax, PACING_DEFAULTS.wpmMax, wpmMin, 1000));
    const maxDelayMs = Math.round(num(p.maxDelayMs, PACING_DEFAULTS.maxDelayMs, 0, PACING_MAX_DELAY_CAP_MS));
    const minDelayMs = Math.round(num(p.minDelayMs, PACING_DEFAULTS.minDelayMs, 0, maxDelayMs));
    return { enabled: p.enabled !== false, wpmMin, wpmMax, minDelayMs, maxDelayMs, jitter: num(p.jitter, PACING_DEFAULTS.jitter, 0, 1) };
  }

  // Words in Thai and English alike (Thai has no spaces between words).
  function countWords(text) {
    if (!text) return 0;
    if (typeof Intl !== 'undefined' && typeof Intl.Segmenter === 'function') {
      let n = 0;
      for (const seg of new Intl.Segmenter('th', { granularity: 'word' }).segment(text)) if (seg.isWordLike) n++;
      return n;
    }
    const latin = (text.match(/[A-Za-z0-9]+/g) || []).length;
    const thai = (text.match(/[฀-๿]/g) || []).length;
    return latin + Math.round(thai / 5);
  }

  // Standard normal sample (Box-Muller) from a uniform `rand` in [0, 1).
  function gaussian(rand = Math.random) {
    const u1 = Math.max(rand(), 1e-12);
    const u2 = rand();
    return Math.sqrt(-2 * Math.log(u1)) * Math.cos(2 * Math.PI * u2);
  }

  // { ms, words, wpm, readMs } for the wait after an answer of `text`. ms is 0 when pacing is off.
  function pacingDelay(text, settings, rand = Math.random) {
    const p = normalizePacing(settings);
    const words = countWords(text);
    if (!p.enabled) return { ms: 0, words, wpm: 0, readMs: 0 };
    const wpm = p.wpmMin + rand() * (p.wpmMax - p.wpmMin);
    const readMs = (words / wpm) * 60000;
    const noisy = readMs + gaussian(rand) * p.jitter * readMs;
    const ms = Math.round(Math.min(p.maxDelayMs, Math.max(p.minDelayMs, noisy)));
    return { ms, words, wpm: Math.round(wpm), readMs: Math.round(readMs) };
  }

  return {
    createAssistStreamParser,
    isSleepingTab,
    pickTabToWake,
    TRANSPORT_LABELS,
    transportOrder,
    transportFailureMessage,
    PACING_DEFAULTS,
    normalizePacing,
    countWords,
    gaussian,
    pacingDelay,
    extractReplies,
    createDeltaTracker,
    createGeminiClient,
    runApiTurn,
    runImageGeneration,
    extractFiles,
    imagePrompt,
    toModelSlug,
    toApiModelId,
    GEMINI_HOST_PATTERNS,
    GEMINI_FALLBACK_URL,
    GEMINI_URL_RE,
    isGeminiUrl,
    parseGeminiUrl,
    base64urlEncode,
    base64urlDecode,
    stripXssi,
    geminiJwtClaims,
    signJwtHS256,
    runGeminiProbe
  };
})();
