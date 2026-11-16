const $ = (id) => document.getElementById(id);
const hostIn = $('host');
const tokenIn = $('token');
const urlIn = $('geminiUrl');
const errEl = $('err');
const hintEl = $('hint');
const outEl = $('out');
const transportSel = $('transport');
const modelSel = $('defaultModel');
const pacingIn = { on: $('pacingOn'), wpmMin: $('wpmMin'), wpmMax: $('wpmMax'), minDelay: $('minDelay'), maxDelay: $('maxDelay') };

const log = GeBridgeLog;
log.info('popup opened');

const send = (msg) => chrome.runtime.sendMessage(msg).catch((err) => {
  log.warn('popup: message failed', { type: msg.type, error: err.message });
  return null;
});

function setStatus(prefix, cls, text) {
  $(prefix + 'Dot').className = 'dot ' + cls;
  $(prefix + 'Txt').textContent = text;
}

async function refresh() {
  const s = await send({ type: 'POPUP_STATE' });
  if (!s) return;
  if (document.activeElement !== hostIn) hostIn.value = s.host || '127.0.0.1:8787';
  if (s.token && !tokenIn.value) tokenIn.value = s.token;
  if (s.homeUrl && !urlIn.value) urlIn.value = s.homeUrl;
  if (s.transport && document.activeElement !== transportSel) transportSel.value = s.transport;
  if (document.activeElement !== modelSel) fillModels(s.scannedModels || [], s.defaultModel || 'auto');
  if (s.pacing && !Object.values(pacingIn).includes(document.activeElement)) showPacing(s.pacing);

  if (s.wsConnected) setStatus('srv', 'ok', 'Connected and paired');
  else if (s.serverUp) setStatus('srv', '', s.lastError || 'Running, connecting...');
  else setStatus('srv', 'bad', 'Server not running');

  if (s.geminiTabs && s.homeUrl) setStatus('gem', 'ok', 'Tab open, home URL learned');
  else if (s.geminiTabs) setStatus('gem', '', 'Tab open, open your home page to learn URL');
  else setStatus('gem', s.homeUrl ? '' : 'bad', s.homeUrl ? 'No tab open (URL saved)' : 'No tab open, URL unknown');
}

function formatServerPing(r) {
  if (!r?.ok) return r?.error || 'No response from extension';
  return [
    `Server OK ${r.ms} ms`,
    `version ${r.version || '?'}`,
    `WS ${r.wsState}, server sees extension: ${r.extensionConnected ? 'yes' : 'no'}`
  ].join('\n');
}

function formatGeminiPing(r) {
  if (r?.noTab) return 'No Gemini Enterprise tab open and no home URL known. Use "Open Gemini Enterprise" or paste the URL under Advanced.';
  if (!r?.steps) return r?.error || 'No response from page';
  const lines = r.steps.map((s) => {
    const status = s.httpStatus ? ` HTTP ${s.httpStatus}` : '';
    const detail = s.error ? ` - ${s.error}` : s.hasSession ? ' (session created)' : '';
    return `${s.step}: ${s.ok ? 'OK' : 'FAIL'} ${s.ms} ms${status}${detail}`;
  });
  lines.push(`Total ${r.ms} ms: ${r.ok ? 'OK' : 'FAILED'}${r.via ? ` (via ${r.via})` : ''}`);
  return lines.join('\n');
}

$('pingSrvBtn').onclick = async () => {
  log.info('popup: click Ping server');
  outEl.textContent = 'Pinging server...';
  const r = await send({ type: 'POPUP_PING_SERVER' });
  log.info('popup: Ping server result', { ok: r?.ok, ms: r?.ms, wsState: r?.wsState, error: r?.error });
  outEl.textContent = formatServerPing(r);
  refresh();
};

$('pingGemBtn').onclick = async () => {
  log.info('popup: click Ping Gemini');
  outEl.textContent = 'Pinging Gemini...';
  const r = await send({ type: 'POPUP_PING_GEMINI' });
  log.info('popup: Ping Gemini result', { ok: r?.ok, noTab: r?.noTab, error: r?.error, steps: r?.steps });
  outEl.textContent = formatGeminiPing(r);
};

function fillModels(models, selected) {
  const opts = [['auto', 'Auto (Gemini chooses)']];
  for (const m of models) if (m && m.id && m.available !== false) opts.push([m.id, `${m.display_name || m.id} (${m.id})`]);
  if (!opts.some(([id]) => id === selected)) opts.push([selected, `${selected} (not in last scan)`]);
  modelSel.replaceChildren(...opts.map(([id, label]) => new Option(label, id, false, id === selected)));
}

modelSel.onchange = async () => {
  log.info('popup: change default model', { model: modelSel.value });
  const r = await send({ type: 'POPUP_SET_DEFAULT_MODEL', model: modelSel.value });
  errEl.textContent = r?.ok ? `Default model: ${r.defaultModel}.` : 'Could not save default model.';
};

function showPacing(p) {
  pacingIn.on.checked = p.enabled;
  pacingIn.wpmMin.value = p.wpmMin;
  pacingIn.wpmMax.value = p.wpmMax;
  pacingIn.minDelay.value = p.minDelayMs / 1000;
  pacingIn.maxDelay.value = p.maxDelayMs / 1000;
}

async function savePacing() {
  const pacing = {
    enabled: pacingIn.on.checked,
    wpmMin: Number(pacingIn.wpmMin.value),
    wpmMax: Number(pacingIn.wpmMax.value),
    minDelayMs: Math.round(Number(pacingIn.minDelay.value) * 1000),
    maxDelayMs: Math.round(Number(pacingIn.maxDelay.value) * 1000)
  };
  const r = await send({ type: 'POPUP_SET_PACING', pacing });
  if (!r?.ok) {
    errEl.textContent = 'Could not save pacing.';
    return;
  }
  showPacing(r.pacing);
  const p = r.pacing;
  errEl.textContent = p.enabled
    ? `Pacing on: ${p.wpmMin}-${p.wpmMax} wpm, wait ${p.minDelayMs / 1000}-${p.maxDelayMs / 1000} s.`
    : 'Pacing off: questions are sent right away.';
}

for (const el of Object.values(pacingIn)) el.onchange = savePacing;

transportSel.onchange = async () => {
  log.info('popup: change transport', { transport: transportSel.value });
  const r = await send({ type: 'POPUP_SET_TRANSPORT', transport: transportSel.value });
  errEl.textContent = r?.ok ? `Transport set to ${r.transport}.` : 'Could not save transport.';
};

$('openBtn').onclick = async () => {
  log.info('popup: click Open Gemini');
  const r = await send({ type: 'POPUP_OPEN_GEMINI' });
  log.info('popup: Open Gemini result', { ok: r?.ok, hasTarget: r?.hasTarget });
  hintEl.textContent = r && !r.hasTarget
    ? 'Your home URL is not known yet. Sign in, open your Gemini Enterprise home page and it will be learned, or paste it under Advanced.'
    : '';
};

$('saveUrlBtn').onclick = async () => {
  log.info('popup: click Save Gemini URL');
  const r = await send({ type: 'POPUP_SET_GEMINI_URL', url: urlIn.value });
  log.info('popup: Save Gemini URL result', { ok: r?.ok, cleared: r?.cleared });
  errEl.textContent = r?.ok
    ? (r.cleared ? 'Gemini URL cleared.' : 'Gemini URL saved.')
    : 'Not a Gemini Enterprise home URL (needs /home/cid/<id>?csesidx=<digits>).';
  refresh();
};

$('saveBtn').onclick = async () => {
  log.info('popup: click Save & Connect');
  const r = await send({ type: 'POPUP_SAVE', host: hostIn.value.trim(), token: tokenIn.value.trim() });
  log.info('popup: Save & Connect result', { ok: r?.ok });
  setTimeout(refresh, 400);
};

$('scanBtn').onclick = async () => {
  log.info('popup: click Scan models');
  const r = await send({ type: 'POPUP_SCAN_MODELS' });
  log.info('popup: Scan models result', { ok: r?.ok });
  errEl.textContent = r?.ok ? 'Scanning Gemini Model Picker...' : 'Open Gemini Enterprise tab first.';
};

$('dashBtn').onclick = () => {
  log.info('popup: click Open Control Center');
  chrome.tabs.create({ url: 'http://' + (hostIn.value.trim() || '127.0.0.1:8787') });
};

$('calBtn').onclick = async () => {
  log.info('popup: click Calibrate');
  const r = await send({ type: 'POPUP_CALIBRATE' });
  log.info('popup: Calibrate result', { ok: r?.ok });
  await new Promise((res) => setTimeout(res, 150));
  window.close();
};

async function refreshLogs() {
  const r = await send({ type: 'GET_LOGS' });
  if (!r) { $('logCount').textContent = 'Entries: unavailable'; return; }
  $('logCount').textContent = `Entries buffered: ${r.count}`;
  $('logLast').textContent = r.lastError
    ? `Last error: ${new Date(r.lastError.ts).toLocaleTimeString()} ${GeBridgeLog.redactString(r.lastError.msg)}`
    : 'No errors logged.';
}

function stamp() {
  return new Date().toISOString().replace(/[:.]/g, '-');
}

$('dlLogBtn').onclick = async () => {
  log.info('popup: click Download log');
  const r = await send({ type: 'GET_LOGS' });
  if (!r) { errEl.textContent = 'Could not read log from background.'; return; }
  const body = r.entries.map((e) => JSON.stringify(e)).join('\n') + '\n';
  const url = URL.createObjectURL(new Blob([body], { type: 'application/x-ndjson' }));
  const a = document.createElement('a');
  a.href = url;
  a.download = `ge-bridge-extension-${stamp()}.jsonl`;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 5000);
  log.info('popup: log downloaded', { entries: r.entries.length });
};

function formatDiagnostics(d) {
  const lines = [
    `GeBridge ${d.version || '?'} (extension id ${d.extensionId || '?'})`,
    `UA: ${d.userAgent || '?'}`,
    `Server up: ${d.serverUp}, WS connected: ${d.wsConnected}`,
    `Last error: ${d.lastError || 'none'}`
  ];
  const g = d.diag;
  if (g) {
    lines.push(`Tabs matching patterns: ${g.matched}, candidate gemini/vertexaisearch tabs: ${g.candidates}`);
    if (g.mismatch) lines.push('MATCH PATTERN MISMATCH: candidate tabs exist that the manifest patterns missed');
    for (const t of g.tabs || []) {
      lines.push(`tab ${t.tabId}: status=${t.status} discarded=${t.discarded} permission=${t.permission}`
        + (t.probe ? ` loaded=${t.probe.loaded} readyState=${t.probe.readyState} hasLib=${t.probe.hasLib}` : '')
        + (t.probeError ? ` PROBE ERROR: ${t.probeError}` : '')
        + (t.injected !== undefined ? ` manualInject=${t.injected}${t.injectError ? ' ' + t.injectError : ''}` : ''));
    }
    if (g.crash) lines.push(`diagnostics crashed: ${g.crash}`);
  }
  return GeBridgeLog.redactString(lines.join('\n'));
}

$('copyDiagBtn').onclick = async () => {
  log.info('popup: click Copy diagnostics');
  outEl.textContent = 'Collecting diagnostics...';
  const d = await send({ type: 'POPUP_DIAG' });
  const text = d && !d.error ? formatDiagnostics(d) : `Diagnostics failed: ${d?.error || 'no response'}`;
  outEl.textContent = text;
  try {
    await navigator.clipboard.writeText(text);
    errEl.textContent = 'Diagnostics copied to clipboard.';
  } catch (err) {
    log.warn('popup: clipboard write failed', { error: err.message });
    errEl.textContent = 'Clipboard blocked; select the text above and copy it manually.';
  }
};

$('logsBox').addEventListener('toggle', () => { if ($('logsBox').open) refreshLogs(); });

refresh();
refreshLogs();
