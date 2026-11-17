const DEFAULT_HOST = '127.0.0.1:8787';
importScripts('lib/log.js', 'lib/gemini.js');
const log = GeBridgeLog;
const { GEMINI_HOST_PATTERNS, GEMINI_FALLBACK_URL, isGeminiUrl, parseGeminiUrl } = GeBridgeLib;
const RETRY_BASE_MS = 2000;
const RETRY_MAX_MS = 30000;
const DIAG_MIN_GAP_MS = 60000;
const DIAG_UPDATE_GAP_MS = 5000;
const WATCH_HOST_RE = /vertexaisearch|gemini/i;
const INJECT_FILES = ['lib/log.js', 'lib/gemini.js', 'content.js'];
const TRANSPORTS = ['auto', 'api-sw', 'api-tab', 'dom'];
const VIRTUAL_LANES = [900001, 900002];
const isVirtualLane = (id) => VIRTUAL_LANES.includes(id);
// Failures at these steps with these causes mean the API is unreachable from this context
// 'stream' too: a turn moves on only while nothing has streamed (progress.started stops it otherwise).
const FALLBACK_STEPS = ['auth', 'jwt', 'session', 'stream'];

const laneSessions = new Map(); // laneId -> Gemini session name (memory only)
const swClients = new Map();    // homeUrl -> client (keeps the JWT cache)
let lastPreflight = null;

let ws = null;
let wsConnected = false;
let lastError = null;
let reconnectTimer = null;
let connecting = false;
let needRepair = false;
let serverUp = null;
let retryDelay = RETRY_BASE_MS;
const tabPorts = new Map();

let diagRunning = null;
let lastDiagAt = 0;
let lastDiag = null;

log.attachSender((frame) => wsSend(frame));
log.start();
log.restore().then((n) => log.info('restored persisted log', { entries: n })).catch(() => {});
log.info('service worker start', {
  version: chrome.runtime.getManifest().version,
  extensionId: chrome.runtime.id,
  userAgent: navigator.userAgent
});

async function getConfig() {
  const d = await chrome.storage.local.get(['host', 'token']);
  return {
    host: (d.host || DEFAULT_HOST).replace(/^https?:\/\//, '').replace(/\/+$/, ''),
    token: (d.token || '').trim()
  };
}

function wsSend(obj) {
  if (ws && ws.readyState === WebSocket.OPEN) {
    ws.send(JSON.stringify(obj));
    return true;
  }
  return false;
}

// Chrome-unloaded (Memory Saver / restored-not-loaded) tabs cannot be scripted until they load.
function isSleeping(t) {
  return Boolean(t.discarded) || t.status === 'unloaded';
}

async function geminiTabs({ includeSleeping = false } = {}) {
  const tabs = await chrome.tabs.query({ url: GEMINI_HOST_PATTERNS });
  return includeSleeping ? tabs : tabs.filter((t) => !isSleeping(t));
}

async function sendToTab(tabId, msg) {
  try {
    return await chrome.tabs.sendMessage(tabId, msg);
  } catch (err) {
    log.warn('sendMessage failed, injecting content script', { tabId, type: msg && msg.type, error: err.message });
    try {
      await chrome.scripting.executeScript({ target: { tabId }, files: INJECT_FILES });
      log.info('content script injected', { tabId });
    } catch (injErr) {
      log.error('executeScript failed', { tabId, error: injErr.message });
      throw injErr;
    }
    return chrome.tabs.sendMessage(tabId, msg);
  }
}

function sendTarget(target) {
  if (target) wsSend({ type: 'GEMINI_TARGET', homeUrl: target.homeUrl });
}

async function learnTarget(url) {
  const target = parseGeminiUrl(url);
  if (!target) return;
  const { geminiTarget } = await chrome.storage.local.get('geminiTarget');
  if (geminiTarget?.homeUrl === target.homeUrl) return;
  await chrome.storage.local.set({ geminiTarget: target });
  log.info('learned Gemini home URL', { homeUrl: target.homeUrl });
  sendTarget(target);
  runApiPreflight('target-learned').catch((err) => log.warn('api-sw preflight crashed', { error: err.message }));
  syncTabsToServer();
}

// ---------- API transport ----------

async function getTransport() {
  const { transport } = await chrome.storage.local.get('transport');
  return TRANSPORTS.includes(transport) ? transport : 'auto';
}

// Model for a turn: the harness's targetModel wins, else the popup default; 'auto' = let Gemini choose.
async function resolveModel(msg) {
  if (msg.targetModel) return msg.targetModel;
  const { defaultModel } = await chrome.storage.local.get('defaultModel');
  return defaultModel || 'auto';
}

async function getStoredTarget() {
  const { geminiTarget } = await chrome.storage.local.get('geminiTarget');
  return geminiTarget || null;
}

function swClientFor(target) {
  let c = swClients.get(target.homeUrl);
  if (!c) {
    c = GeBridgeLib.createGeminiClient({ target, origin: target.origin, log });
    swClients.set(target.homeUrl, c);
  }
  return c;
}

// Steps auth + jwt only (no session creation, so no empty chats appear in the user's history).
async function runApiPreflight(reason) {
  const target = await getStoredTarget();
  if (!target) {
    log.info('api-sw preflight skipped: no target', { reason });
    return null;
  }
  const t0 = Date.now();
  const rec = { reason };
  try {
    await GeBridgeLib.createGeminiClient({ target, origin: target.origin, log }).ensureJwt();
    rec.ok = true;
    rec.steps = { auth: 'ok', jwt: 'ok' };
  } catch (err) {
    rec.ok = false;
    rec.failedStep = err.step || 'auth';
    rec.httpStatus = err.httpStatus;
    rec.network = Boolean(err.network);
    rec.error = String(err.message).slice(0, 200);
  }
  rec.ms = Date.now() - t0;
  lastPreflight = rec;
  if (rec.ok) log.info('api-sw preflight', rec);
  else log.error('api-sw preflight', rec);
  return rec;
}

function canFallBack(err) {
  return FALLBACK_STEPS.includes(err.step)
    && (err.network || err.httpStatus === 401 || err.httpStatus === 403);
}

// ---------- Keeping a Gemini tab usable ----------
// Chrome's Memory Saver unloads background tabs, and an unloaded Gemini tab cannot run a turn: every
// "No scriptable Gemini tab" failure in the logs (Nov 2026) happened while the only Gemini tab was
// discarded. Gemini tabs are therefore marked not auto-discardable, and a sleeping one is reloaded (in
// the background) when a turn needs it; with no Gemini tab at all, the saved home page is opened.

const WAKE_TIMEOUT_MS = 30000;
let wakeInFlight = null;

async function keepTabAwake(tab) {
  if (!tab || tab.autoDiscardable === false) return;
  try {
    await chrome.tabs.update(tab.id, { autoDiscardable: false });
    log.info('gemini tab: Chrome will not unload it', { tabId: tab.id });
  } catch (err) {
    log.warn('gemini tab: could not turn off auto-discard', { tabId: tab.id, error: err.message });
  }
}

async function keepGeminiTabsAwake() {
  for (const t of await geminiTabs({ includeSleeping: true })) await keepTabAwake(t);
}

async function waitScriptable(tabId, timeoutMs) {
  const end = Date.now() + timeoutMs;
  while (Date.now() < end) {
    try {
      const t = await chrome.tabs.get(tabId);
      if (t.status === 'complete' && (await sendToTab(tabId, { type: 'PING_TAB' }))) return true;
    } catch { /* still loading, or closed */ }
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  return false;
}

// A scriptable Gemini tab, waking or opening one when needed. One wake at a time: jobs that arrive
// while a tab is waking wait for the same wake.
async function ensureScriptableTab(preferId, target, jobId) {
  const ready = await findScriptableTab(preferId, target);
  if (ready) return ready;
  if (!wakeInFlight) {
    wakeInFlight = (async () => {
      const all = await geminiTabs({ includeSleeping: true });
      let tab = GeBridgeLib.pickTabToWake(all, { preferId, host: target ? hostOf(target.origin) : null });
      const t0 = Date.now();
      if (tab) {
        log.info('gemini tab: waking it for a turn', { jobId, tabId: tab.id, discarded: Boolean(tab.discarded), status: tab.status });
        await chrome.tabs.reload(tab.id);
      } else if (target && target.homeUrl) {
        tab = await chrome.tabs.create({ url: target.homeUrl, active: false });
        log.info('gemini tab: none open, opened the home page in the background', { jobId, tabId: tab.id });
      } else {
        log.warn('gemini tab: none open and no home URL known', { jobId });
        return null;
      }
      await keepTabAwake(tab);
      const ok = await waitScriptable(tab.id, WAKE_TIMEOUT_MS);
      if (!ok) {
        log.warn('gemini tab: not ready after waking', { jobId, tabId: tab.id, ms: Date.now() - t0 });
        return null;
      }
      log.info('gemini tab: ready after waking', { jobId, tabId: tab.id, ms: Date.now() - t0 });
      syncTabsToServer();
      return chrome.tabs.get(tab.id);
    })().finally(() => { wakeInFlight = null; });
  }
  return wakeInFlight;
}

async function findScriptableTab(preferId, target) {
  const tabs = await geminiTabs();
  tabs.sort((a, b) => (b.id === preferId ? 1 : 0) - (a.id === preferId ? 1 : 0));
  for (const t of tabs) {
    if (target && hostOf(t.url) !== hostOf(target.origin)) continue;
    try {
      if (await sendToTab(t.id, { type: 'PING_TAB' })) return t;
    } catch { /* not scriptable */ }
  }
  return null;
}

function apiTurnInSw(msg, target, session, progress) {
  return GeBridgeLib.runApiTurn(swClientFor(target), session, msg.prompt, {
    onThought: (d) => { progress.started = true; wsSend({ type: 'DOM_DELTA', jobId: msg.jobId, thoughtDelta: d, textDelta: '' }); },
    onText: (d) => { progress.started = true; wsSend({ type: 'DOM_DELTA', jobId: msg.jobId, thoughtDelta: '', textDelta: d }); }
  }, { modelId: msg.targetModel, attachments: msg.images || [] });
}

async function apiTurnInTab(msg, target, session, progress) {
  const tab = await ensureScriptableTab(msg.tabId, target, msg.jobId);
  if (!tab) {
    const err = new Error('No scriptable Gemini tab for API-from-tab transport');
    err.step = 'auth';
    err.network = true;
    throw err;
  }
  const tabTarget = parseGeminiUrl(tab.url) || target;
  log.info('RUN_TURN_API to tab', { jobId: msg.jobId, tabId: tab.id });
  const r = await chrome.tabs.sendMessage(tab.id, {
    type: 'RUN_TURN_API', jobId: msg.jobId, prompt: msg.prompt, session, target: tabTarget, modelId: msg.targetModel, images: msg.images || []
  });
  if (!r) throw new Error('No response from tab');
  if (r.started) progress.started = true;
  if (!r.ok) {
    const err = new Error(r.error || 'API turn failed in tab');
    err.step = r.step;
    err.httpStatus = r.httpStatus;
    err.network = r.network;
    throw err;
  }
  return r;
}

async function domTurn(msg) {
  let tabId = msg.tabId;
  const own = isVirtualLane(tabId) ? null : await chrome.tabs.get(tabId).catch(() => null);
  if (!own || isSleeping(own)) {
    const tab = await ensureScriptableTab(own ? own.id : null, null, msg.jobId);
    if (!tab) throw new Error('No scriptable Gemini tab available for DOM transport');
    tabId = tab.id;
  }
  await chrome.tabs.sendMessage(tabId, { ...msg, tabId });
}

// ---------- Human pacing ----------
// After each answer the next question waits about as long as reading that answer takes (random
// 200-250 wpm plus Gaussian jitter, clamped to the popup's min/max delay). One account, one reader:
// the wait is shared by every tab and API lane, and queued jobs pass one at a time.

let pacingUntil = 0;
let pacingGate = Promise.resolve();
// Image requests time out after 115 s in ge-bridge (120 s in OpenMausBot), so their wait is capped.
const IMAGE_PACING_CAP_MS = 20000;

async function getPacing() {
  const { pacing } = await chrome.storage.local.get('pacing');
  return GeBridgeLib.normalizePacing(pacing);
}

function waitForPacing(jobId, kind) {
  const run = pacingGate.then(async () => {
    const p = await getPacing();
    if (!p.enabled) return;
    const wait = Math.min(pacingUntil - Date.now(), kind === 'image' ? IMAGE_PACING_CAP_MS : Infinity);
    if (wait > 0) {
      log.debug('pacing: waiting before the next question', { jobId, kind, ms: wait });
      await new Promise((resolve) => setTimeout(resolve, wait));
    }
    // Hold any job queued behind this one for at least the minimum, until this answer sets the real wait.
    pacingUntil = Date.now() + p.minDelayMs;
  });
  pacingGate = run.catch(() => {});
  return run;
}

async function notePacingAnswer(jobId, text) {
  const p = await getPacing();
  if (!p.enabled) return;
  const d = GeBridgeLib.pacingDelay(text || '', p);
  pacingUntil = Math.max(pacingUntil, Date.now() + d.ms);
  log.debug('pacing: next question not before', { jobId, ms: d.ms, words: d.words, wpm: d.wpm, readMs: d.readMs });
}

async function handleRunTurn(rawMsg) {
  await waitForPacing(rawMsg.jobId, 'turn');
  const msg = { ...rawMsg, targetModel: await resolveModel(rawMsg) };
  const transport = await getTransport();
  const target = await getStoredTarget();
  const hasImages = Boolean(msg.images && msg.images.length);
  log.info('RUN_TURN dispatch', {
    jobId: msg.jobId, tabId: msg.tabId, virtual: isVirtualLane(msg.tabId), transport,
    hasTarget: Boolean(target), newChat: Boolean(msg.newChat), images: hasImages ? msg.images.length : 0,
    requestedModel: rawMsg.targetModel || null, model: msg.targetModel, requestedEffort: msg.reasoningEffort || null
  });

  if (!target && transport !== 'dom') log.warn('RUN_TURN: no Gemini target known, using dom', { jobId: msg.jobId });
  const order = GeBridgeLib.transportOrder(transport, { kind: 'turn', hasImages, hasTarget: Boolean(target) });
  if (hasImages && !order.length) {
    const why = target
      ? 'The DOM transport cannot attach images. Switch the extension transport to auto (or an API mode) and retry.'
      : 'No Gemini target is known, so images cannot be attached. Open a Gemini Enterprise tab (signed in) and use the auto or API transport.';
    log.error('RUN_TURN refused: images need an API transport', { jobId: msg.jobId, transport, hasTarget: Boolean(target), images: msg.images.length });
    wsSend({ type: 'TURN_DONE', jobId: msg.jobId, error: why });
    return;
  }
  const failures = [];

  const lane = msg.tabId;
  const progress = { started: false };
  for (const mode of order) {
    const t0 = Date.now();
    try {
      if (mode === 'dom') {
        await domTurn(msg);
        log.info('RUN_TURN handed to DOM path', { jobId: msg.jobId });
        return;
      }
      const session = msg.newChat ? null : (laneSessions.get(lane) || null);
      const out = mode === 'api-sw'
        ? await apiTurnInSw(msg, target, session, progress)
        : await apiTurnInTab(msg, target, session, progress);
      laneSessions.set(lane, out.session);
      log.info('RUN_TURN done via API', { jobId: msg.jobId, mode, ms: Date.now() - t0, answerChars: out.text.length, reusedSession: Boolean(session) });
      wsSend({ type: 'TURN_DONE', jobId: msg.jobId, text: out.text });
      notePacingAnswer(msg.jobId, out.text);
      return;
    } catch (err) {
      failures.push({ mode, message: err.message, httpStatus: err.httpStatus, step: err.step });
      log.error('RUN_TURN attempt failed', {
        jobId: msg.jobId, mode, step: err.step, httpStatus: err.httpStatus, network: Boolean(err.network),
        started: progress.started, error: err.message
      });
      if (mode === 'dom' || progress.started) break;
      if (transport === 'auto' && canFallBack(err)) continue;
      break;
    }
  }
  wsSend({ type: 'TURN_DONE', jobId: msg.jobId, error: GeBridgeLib.transportFailureMessage(failures, 'Turn') });
  syncTabsToServer();
}

// One generated image via the API: service worker first, then inside a Gemini tab (page origin).
// Replies IMAGE_DONE { jobId, mime, b64 } or { jobId, error }. Image bytes are never logged.
async function handleRunImage(msg) {
  await waitForPacing(msg.jobId, 'image');
  const transport = await getTransport();
  const target = await getStoredTarget();
  log.info('RUN_IMAGE dispatch', { jobId: msg.jobId, aspect: msg.aspect || null, promptChars: (msg.prompt || '').length, transport });
  if (!target) {
    wsSend({ type: 'IMAGE_DONE', jobId: msg.jobId, error: 'No Gemini Enterprise target known. Open your Gemini page once.' });
    return;
  }
  const order = GeBridgeLib.transportOrder(transport, { kind: 'image' });
  const failures = [];
  for (const mode of order) {
    const t0 = Date.now();
    try {
      let out;
      if (mode === 'api-sw') {
        out = await GeBridgeLib.runImageGeneration(swClientFor(target), msg.prompt, msg.aspect, { modelId: msg.modelId });
      } else {
        const tab = await ensureScriptableTab(null, target, msg.jobId);
        if (!tab) throw Object.assign(new Error('No scriptable Gemini tab for API-from-tab image'), { step: 'auth', network: true });
        const r = await chrome.tabs.sendMessage(tab.id, {
          type: 'RUN_IMAGE_API', jobId: msg.jobId, prompt: msg.prompt, aspect: msg.aspect, modelId: msg.modelId,
          target: parseGeminiUrl(tab.url) || target
        });
        if (!r || !r.ok) throw Object.assign(new Error((r && r.error) || 'No response from tab'), { step: r && r.step, httpStatus: r && r.httpStatus, network: r && r.network });
        out = r;
      }
      log.info('RUN_IMAGE done', { jobId: msg.jobId, mode, ms: Date.now() - t0, mime: out.mime, b64Chars: out.b64.length });
      wsSend({ type: 'IMAGE_DONE', jobId: msg.jobId, mime: out.mime, b64: out.b64, text: out.text || '' });
      notePacingAnswer(msg.jobId, '');
      return;
    } catch (err) {
      failures.push({ mode, message: err.message, httpStatus: err.httpStatus });
      log.error('RUN_IMAGE attempt failed', { jobId: msg.jobId, mode, step: err.step, httpStatus: err.httpStatus, network: Boolean(err.network), error: err.message });
      if (!canFallBack(err)) break;
    }
  }
  wsSend({ type: 'IMAGE_DONE', jobId: msg.jobId, error: GeBridgeLib.transportFailureMessage(failures, 'Image generation') });
}

async function learnFromOpenTabs() {
  for (const t of await geminiTabs({ includeSleeping: true })) await learnTarget(t.url);
  await keepGeminiTabsAwake();
}

async function fetchHealth(host) {
  try {
    const t0 = performance.now();
    const res = await fetch(`http://${host}/api/health`, { signal: AbortSignal.timeout(2000) });
    if (!res.ok) {
      log.warn('health check failed', { host, status: res.status });
      return null;
    }
    const body = await res.json();
    if (!body?.ok || body.name !== 'ge-bridge') {
      log.warn('health check: not a ge-bridge server', { host, name: body?.name });
      return null;
    }
    const ms = Math.round(performance.now() - t0);
    log.debug('health ok', { host, status: res.status, ms, version: body.version });
    return { ...body, ms };
  } catch (err) {
    log.warn('health check error', { host, error: err.message });
    return null;
  }
}

async function requestPairToken(host) {
  try {
    const res = await fetch(`http://${host}/api/pair`, { method: 'POST', signal: AbortSignal.timeout(3000) });
    if (!res.ok) {
      log.warn('pair refused', { host, status: res.status });
      return null;
    }
    const { token } = await res.json();
    const ok = typeof token === 'string' && Boolean(token);
    log.info('pair result', { host, status: res.status, gotToken: ok });
    return ok ? token : null;
  } catch (err) {
    log.warn('pair error', { host, error: err.message });
    return null;
  }
}

async function syncTabsToServer() {
  const tabs = await geminiTabs();
  const list = [];
  for (const t of tabs) {
    let info = null;
    try {
      info = await sendToTab(t.id, { type: 'PING_TAB' });
    } catch (err) {
      // Tab still loading, or the content script cannot be injected
      log.warn('PING_TAB failed', { tabId: t.id, status: t.status, error: err.message });
    }
    if (info) {
      list.push({
        tabId: t.id,
        url: t.url,
        title: t.title,
        activeModel: info.activeModel || null,
        calibrated: Boolean(info.calibrated)
      });
    }
  }
  const target = await getStoredTarget();
  if (target && (await getTransport()) !== 'dom') {
    VIRTUAL_LANES.forEach((id, i) => list.push({
      tabId: id,
      url: target.homeUrl,
      title: `Gemini Enterprise (API lane ${i + 1})`,
      activeModel: null,
      calibrated: true
    }));
  }
  wsSend({ type: 'TABS_SYNC', tabs: list });
}

// ---------- injection diagnostics ----------

function hostOf(url) {
  try { return new URL(url).hostname; } catch { return ''; }
}

function probeInPage() {
  return {
    loaded: Boolean(window.__GE_BRIDGE_V2_LOADED__),
    href: location.href,
    readyState: document.readyState,
    hasLib: typeof GeBridgeLib !== 'undefined'
  };
}

async function diagnoseTab(t) {
  const rec = { tabId: t.id, status: t.status, discarded: Boolean(t.discarded) };
  let origin = null;
  try { origin = new URL(t.url).origin; } catch { /* no usable URL */ }
  if (origin) {
    try {
      rec.permission = await chrome.permissions.contains({ origins: [`${origin}/*`] });
    } catch (err) {
      rec.permission = `error: ${err.message}`;
    }
    log.debug('diag: host permission', { tabId: t.id, origin, granted: rec.permission });
  }
  if (isSleeping(t)) {
    log.debug('diag: tab is unloaded by Chrome, skipped until it loads', { tabId: t.id, status: t.status });
    return rec;
  }
  let probe = null;
  try {
    const [res] = await chrome.scripting.executeScript({ target: { tabId: t.id }, func: probeInPage });
    probe = res && res.result;
    rec.probe = probe;
    log.debug('diag: executeScript probe ok', { tabId: t.id, result: probe });
  } catch (err) {
    rec.probeError = err.message;
    log.error('diag: executeScript probe failed', { tabId: t.id, error: err.message });
    return rec;
  }
  if (probe && !probe.loaded) {
    try {
      await chrome.scripting.executeScript({ target: { tabId: t.id }, files: INJECT_FILES });
      const [after] = await chrome.scripting.executeScript({ target: { tabId: t.id }, func: probeInPage });
      rec.injected = true;
      rec.afterInject = after && after.result;
      log.warn('diag: content script was missing, injected manually', { tabId: t.id, after: rec.afterInject });
    } catch (err) {
      rec.injected = false;
      rec.injectError = err.message;
      log.error('diag: manual injection failed', { tabId: t.id, error: err.message });
    }
  }
  return rec;
}

async function doDiagnostics(reason) {
  const result = { reason, at: Date.now(), matched: 0, candidates: 0, mismatch: false, tabs: [] };
  try {
    const matched = await geminiTabs({ includeSleeping: true });
    result.matched = matched.length;
    log.debug('diag: tabs matching manifest patterns', {
      reason,
      count: matched.length,
      patterns: GEMINI_HOST_PATTERNS,
      tabs: matched.map((t) => ({ tabId: t.id, url: t.url }))
    });

    const all = await chrome.tabs.query({});
    const cands = all.filter((t) => WATCH_HOST_RE.test(hostOf(t.url || '')));
    result.candidates = cands.length;
    log.debug('diag: tabs with vertexaisearch/gemini host', {
      totalTabs: all.length,
      tabsWithoutUrl: all.filter((t) => !t.url).length,
      tabs: cands.map((t) => ({
        tabId: t.id,
        url: t.url,
        status: t.status,
        discarded: t.discarded,
        frozen: t.frozen,
        active: t.active,
        incognito: t.incognito,
        windowId: t.windowId
      }))
    });

    const matchedIds = new Set(matched.map((t) => t.id));
    const missed = cands.filter((t) => !matchedIds.has(t.id));
    if (missed.length) {
      result.mismatch = true;
      log.error('match pattern mismatch', {
        patterns: GEMINI_HOST_PATTERNS,
        missed: missed.map((t) => ({ tabId: t.id, url: t.url, host: hostOf(t.url) }))
      });
    }

    if (cands.some((t) => t.incognito) && chrome.extension?.isAllowedIncognitoAccess) {
      try {
        log.warn('diag: a candidate tab is incognito', { allowedInIncognito: await chrome.extension.isAllowedIncognitoAccess() });
      } catch (err) {
        log.warn('diag: incognito access check failed', { error: err.message });
      }
    }

    for (const t of cands) result.tabs.push(await diagnoseTab(t));
  } catch (err) {
    log.error('diag: diagnostics crashed', err);
    result.crash = err.message;
  }
  return result;
}

function runInjectionDiagnostics(reason, { minGap = 0 } = {}) {
  if (diagRunning) return diagRunning;
  if (minGap && Date.now() - lastDiagAt < minGap) return Promise.resolve(lastDiag);
  lastDiagAt = Date.now();
  diagRunning = doDiagnostics(reason).then((r) => {
    lastDiag = r;
    return r;
  }).finally(() => {
    diagRunning = null;
  });
  return diagRunning;
}

// ---------- connection ----------

function scheduleReconnect() {
  clearTimeout(reconnectTimer);
  log.debug('reconnect scheduled', { delayMs: retryDelay });
  reconnectTimer = setTimeout(connectWebSocket, retryDelay);
  retryDelay = Math.min(retryDelay * 2, RETRY_MAX_MS);
}

async function connectWebSocket() {
  if (connecting || (ws && (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CONNECTING))) return;
  connecting = true;
  try {
    const cfg = await getConfig();
    const health = await fetchHealth(cfg.host);
    serverUp = Boolean(health);
    if (!health) {
      lastError = 'Server not running';
      scheduleReconnect();
      return;
    }
    let token = cfg.token;
    if (!token || needRepair) {
      log.info('pairing', { hadToken: Boolean(cfg.token), needRepair });
      token = await requestPairToken(cfg.host);
      if (!token) {
        lastError = 'Pairing refused by server';
        scheduleReconnect();
        return;
      }
      await chrome.storage.local.set({ token });
      needRepair = false;
    }
    openSocket(cfg.host, token);
  } catch (err) {
    log.error('connectWebSocket failed', err);
    scheduleReconnect();
  } finally {
    connecting = false;
  }
}

function openSocket(host, token) {
  let opened = false;
  let socket;
  try {
    socket = new WebSocket(`ws://${host}/ws/extension?token=${encodeURIComponent(token)}`);
  } catch (err) {
    lastError = err.message;
    log.error('WebSocket constructor threw', { host, error: err.message });
    return;
  }
  ws = socket;
  log.info('websocket connecting', { host });

  socket.onopen = () => {
    opened = true;
    wsConnected = true;
    lastError = null;
    retryDelay = RETRY_BASE_MS;
    log.info('websocket open', { host });
    chrome.action.setBadgeText({ text: 'ON' });
    chrome.action.setBadgeBackgroundColor({ color: '#16a34a' });
    chrome.storage.local.get('geminiTarget').then((d) => sendTarget(d.geminiTarget));
    syncTabsToServer();
    log.flush();
  };

  socket.onerror = () => {
    log.warn('websocket error event', { host, opened });
  };

  socket.onmessage = async (event) => {
    let msg;
    try {
      msg = JSON.parse(event.data);
    } catch (err) {
      log.warn('websocket: unparseable frame', { error: err.message });
      return;
    }
    if (msg.type !== 'PONG') log.debug('server message', { type: msg.type, tabId: msg.tabId, jobId: msg.jobId });
    if (msg.type === 'REQUEST_MODELS_SYNC') {
      const tabs = await geminiTabs();
      if (tabs[0]) {
        chrome.tabs.sendMessage(tabs[0].id, {
          type: 'SCAN_MODELS',
          forceOpenMenu: Boolean(msg.forceOpenMenu)
        }).catch((err) => log.warn('SCAN_MODELS delivery failed', { tabId: tabs[0].id, error: err.message }));
      } else {
        log.warn('REQUEST_MODELS_SYNC: no Gemini tab');
      }
    } else if (msg.type === 'START_CALIBRATION') {
      const tabs = await geminiTabs();
      if (tabs[0]) {
        chrome.tabs.update(tabs[0].id, { active: true });
        chrome.tabs.sendMessage(tabs[0].id, { type: 'START_CALIBRATION' })
          .catch((err) => log.warn('START_CALIBRATION delivery failed', { tabId: tabs[0].id, error: err.message }));
      } else {
        log.warn('START_CALIBRATION: no Gemini tab');
      }
    } else if (msg.type === 'RUN_IMAGE') {
      handleRunImage(msg).catch((err) => {
        log.error('RUN_IMAGE failed', { jobId: msg.jobId, error: err.message });
        wsSend({ type: 'IMAGE_DONE', jobId: msg.jobId, error: `Image failed: ${err.message}` });
      });
    } else if (msg.type === 'RUN_TURN') {
      handleRunTurn(msg).catch((err) => {
        log.error('RUN_TURN failed', { tabId: msg.tabId, jobId: msg.jobId, error: err.message });
        wsSend({ type: 'TURN_DONE', jobId: msg.jobId, error: `Turn failed: ${err.message}` });
        syncTabsToServer();
      });
    }
  };

  socket.onclose = (event) => {
    log.warn('websocket close', {
      host, code: event.code, reason: event.reason, wasClean: event.wasClean, opened, current: ws === socket
    });
    if (ws !== socket) return;
    wsConnected = false;
    ws = null;
    chrome.action.setBadgeText({ text: '' });
    if (!opened) {
      needRepair = true;
      lastError = 'Connection rejected, re-pairing';
    }
    scheduleReconnect();
  };
}

async function pingServer() {
  const { host } = await getConfig();
  const health = await fetchHealth(host);
  serverUp = Boolean(health);
  if (!health) return { ok: false, error: 'Server not running' };
  const wsState = ws ? ['connecting', 'open', 'closing', 'closed'][ws.readyState] : 'closed';
  return {
    ok: true,
    ms: health.ms,
    version: health.version,
    wsState,
    extensionConnected: Boolean(health.extensionConnected),
    tabs: health.tabs
  };
}

async function pingViaServiceWorker(reason) {
  const target = await getStoredTarget();
  if (!target) return { ok: false, noTab: true };
  log.info('pingGemini via service worker', { reason });
  const r = await GeBridgeLib.runGeminiProbe(target, { origin: target.origin });
  log.info('pingGemini via service worker result', { ok: r.ok, steps: r.steps });
  return { ...r, via: 'extension (SW)' };
}

async function pingGemini() {
  const tabs = await geminiTabs();
  if (!tabs.length) return pingViaServiceWorker('no tab');
  const tab = tabs.find((t) => parseGeminiUrl(t.url)) || tabs[0];
  try {
    const result = await sendToTab(tab.id, { type: 'PING_GEMINI', target: parseGeminiUrl(tab.url) });
    return result ? { ...result, via: 'page tab' } : { ok: false, error: 'No response from page' };
  } catch (err) {
    log.error('pingGemini: tab unreachable', { tabId: tab.id, error: err.message });
    const viaSw = await pingViaServiceWorker('tab not scriptable');
    return viaSw.noTab ? { ok: false, error: `Tab unreachable: ${err.message}` } : viaSw;
  }
}

// Keep MV3 Service Worker continuously responsive
chrome.alarms.create('ws-keepalive', { periodInMinutes: 0.35 });
chrome.alarms.onAlarm.addListener(() => {
  learnFromOpenTabs().catch((err) => log.warn('learnFromOpenTabs failed', { error: err.message }));
  connectWebSocket();
  if (wsConnected) syncTabsToServer();
  runInjectionDiagnostics('alarm', { minGap: DIAG_MIN_GAP_MS });
});

setInterval(() => {
  if (ws && ws.readyState === WebSocket.OPEN) {
    ws.send(JSON.stringify({ type: 'PING' }));
  }
}, 18000);

chrome.tabs.onUpdated.addListener((tabId, change, tab) => {
  if (change.status === 'complete' && WATCH_HOST_RE.test(hostOf(tab.url || ''))) {
    log.info('tab load complete on candidate host', { tabId, url: tab.url, geminiUrl: isGeminiUrl(tab.url) });
    runInjectionDiagnostics('tab-complete', { minGap: DIAG_UPDATE_GAP_MS });
  }
  if (!isGeminiUrl(tab.url)) return;
  if (change.url || change.status === 'complete') learnTarget(tab.url);
  if (change.status === 'complete') {
    keepTabAwake(tab);
    syncTabsToServer();
  }
});
chrome.tabs.onRemoved.addListener(() => syncTabsToServer());

chrome.runtime.onConnect.addListener((port) => {
  if (port.name !== 'ge-v2-port') return;
  const tabId = port.sender?.tab?.id;
  log.info('content port connected', { tabId, url: port.sender?.url });
  if (tabId) tabPorts.set(tabId, port);

  port.onMessage.addListener((msg) => {
    connectWebSocket();
    if (msg.type !== 'DOM_DELTA') log.debug('port message', { tabId, type: msg.type, jobId: msg.jobId });
    if (msg.type === 'MODELS_FOUND') {
      chrome.storage.local.set({ scannedModels: msg.models });
      wsSend({ type: 'MODELS_SYNC', models: msg.models });
    } else if (msg.type === 'DOM_DELTA') {
      wsSend(msg);
    } else if (msg.type === 'TURN_DONE') {
      wsSend(msg);
      if (!msg.error) notePacingAnswer(msg.jobId, msg.text);
    } else if (msg.type === 'TAB_READY') {
      syncTabsToServer();
    }
  });

  port.onDisconnect.addListener(() => {
    log.info('content port disconnected', { tabId });
    if (tabId) tabPorts.delete(tabId);
    syncTabsToServer();
  });
});

chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  if (msg.type === 'LOG') {
    log.ingest(msg.entry, sender?.tab?.id);
    sendResponse({ ok: true });
    return false;
  }
  if (msg.type === 'API_DELTA') {
    wsSend({ type: 'DOM_DELTA', jobId: msg.jobId, thoughtDelta: msg.thoughtDelta || '', textDelta: msg.textDelta || '' });
    sendResponse({ ok: true });
    return false;
  }
  if (msg.type === 'GET_LOGS') {
    sendResponse({ entries: log.entries(), count: log.count(), lastError: log.lastError() });
    return false;
  }
  if (msg.type === 'POPUP_DIAG') {
    (async () => {
      const diag = await runInjectionDiagnostics('popup');
      sendResponse({
        version: chrome.runtime.getManifest().version,
        extensionId: chrome.runtime.id,
        userAgent: navigator.userAgent,
        serverUp,
        wsConnected,
        lastError,
        diag
      });
    })().catch((err) => {
      log.error('POPUP_DIAG failed', err);
      sendResponse({ error: err.message });
    });
    return true;
  }
  if (msg.type === 'POPUP_STATE') {
    (async () => {
      const cfg = await getConfig();
      const health = await fetchHealth(cfg.host);
      serverUp = Boolean(health);
      if (health && !wsConnected) connectWebSocket();
      const { geminiTarget } = await chrome.storage.local.get('geminiTarget');
      const tabs = await geminiTabs();
      sendResponse({
        ...cfg,
        transport: await getTransport(),
        pacing: await getPacing(),
        ...(await chrome.storage.local.get(['defaultModel', 'scannedModels'])),
        preflight: lastPreflight && { ok: lastPreflight.ok, failedStep: lastPreflight.failedStep, httpStatus: lastPreflight.httpStatus },
        wsConnected,
        serverUp,
        lastError: serverUp ? lastError : 'Server not running',
        homeUrl: geminiTarget?.homeUrl || '',
        geminiTabs: tabs.length
      });
    })();
    return true;
  }
  if (msg.type === 'POPUP_SAVE') {
    log.info('popup: save host/token', { host: msg.host, tokenProvided: Boolean(msg.token) });
    chrome.storage.local.set({ host: msg.host, token: msg.token }).then(() => {
      needRepair = false;
      retryDelay = RETRY_BASE_MS;
      if (ws) ws.close();
      connectWebSocket();
      sendResponse({ ok: true });
    });
    return true;
  }
  if (msg.type === 'POPUP_SET_DEFAULT_MODEL') {
    const defaultModel = typeof msg.model === 'string' && msg.model ? msg.model : 'auto';
    log.info('popup: set default model', { defaultModel });
    chrome.storage.local.set({ defaultModel }).then(() => sendResponse({ ok: true, defaultModel }));
    return true;
  }
  if (msg.type === 'POPUP_SET_PACING') {
    const pacing = GeBridgeLib.normalizePacing(msg.pacing);
    log.info('popup: set pacing', pacing);
    chrome.storage.local.set({ pacing }).then(() => sendResponse({ ok: true, pacing }));
    return true;
  }
  if (msg.type === 'POPUP_SET_TRANSPORT') {
    const transport = TRANSPORTS.includes(msg.transport) ? msg.transport : 'auto';
    log.info('popup: set transport', { transport });
    chrome.storage.local.set({ transport }).then(() => {
      syncTabsToServer();
      sendResponse({ ok: true, transport });
    });
    return true;
  }
  if (msg.type === 'POPUP_SET_GEMINI_URL') {
    const raw = String(msg.url || '').trim();
    if (!raw) {
      chrome.storage.local.remove('geminiTarget').then(() => sendResponse({ ok: true, cleared: true }));
      return true;
    }
    const target = parseGeminiUrl(raw);
    if (!target) {
      log.warn('popup: rejected Gemini URL', { url: raw });
      sendResponse({ ok: false });
      return false;
    }
    chrome.storage.local.set({ geminiTarget: target }).then(() => {
      sendTarget(target);
      syncTabsToServer();
      sendResponse({ ok: true });
    });
    return true;
  }
  if (msg.type === 'POPUP_OPEN_GEMINI') {
    chrome.storage.local.get('geminiTarget').then(({ geminiTarget }) => {
      chrome.tabs.create({ url: geminiTarget?.homeUrl || GEMINI_FALLBACK_URL });
      sendResponse({ ok: true, hasTarget: Boolean(geminiTarget) });
    });
    return true;
  }
  if (msg.type === 'POPUP_PING_SERVER') {
    pingServer().then(sendResponse);
    return true;
  }
  if (msg.type === 'POPUP_PING_GEMINI') {
    pingGemini().then(sendResponse);
    return true;
  }
  if (msg.type === 'POPUP_SCAN_MODELS') {
    geminiTabs().then((tabs) => {
      if (tabs[0]) {
        chrome.tabs.sendMessage(tabs[0].id, { type: 'SCAN_MODELS', forceOpenMenu: true })
          .catch((err) => log.warn('SCAN_MODELS delivery failed', { tabId: tabs[0].id, error: err.message }));
      }
      sendResponse({ ok: Boolean(tabs[0]) });
    });
    return true;
  }
  if (msg.type === 'POPUP_CALIBRATE') {
    geminiTabs().then((tabs) => {
      if (tabs[0]) {
        chrome.tabs.update(tabs[0].id, { active: true });
        chrome.tabs.sendMessage(tabs[0].id, { type: 'START_CALIBRATION' })
          .catch((err) => log.warn('START_CALIBRATION delivery failed', { tabId: tabs[0].id, error: err.message }));
      }
      sendResponse({ ok: Boolean(tabs[0]) });
    });
    return true;
  }
});

learnFromOpenTabs().catch((err) => log.warn('learnFromOpenTabs failed', { error: err.message }));
connectWebSocket();
runInjectionDiagnostics('sw-start');
runApiPreflight('sw-start').catch((err) => log.warn('api-sw preflight crashed', { error: err.message }));
