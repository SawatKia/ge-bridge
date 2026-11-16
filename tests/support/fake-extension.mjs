export const FAKE_PNG_B64 = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x66, 0x61, 0x6b, 0x65]).toString('base64');
export const REPLY = 'Hello from the fake extension.';
export const TOOL_CHUNKS = [
  'Let me check. <tool_ca',
  'll>{"name":"get_weather","arguments":{"city":"Bangkok"}}</tool_call>',
];

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const MODELS = [
  { id: 'gemini-test-flash', display_name: 'Gemini Test Flash', available: true, supports_thinking: true, efforts: ['auto', 'low', 'medium', 'high'] },
  { id: 'gemini-test-pro', display_name: 'Gemini Test Pro', available: true, supports_thinking: false, efforts: [] },
  { id: 'gemini-test-locked', display_name: 'Gemini Test Locked', available: false, supports_thinking: false, efforts: [] },
];

export const makeTabs = (ids) => ids.map((tabId) => ({
  tabId,
  url: 'https://vertexaisearch.cloud.google.com/',
  title: 'Gemini Enterprise',
  activeModel: 'gemini-test-flash',
  calibrated: true,
}));

// `gate(runTurnMessage)` may return a promise that delays the answer.
export async function startFakeExtension({ base, token, tabIds = [101, 102], name = 'ext', delayMs = 20, gate } = {}) {
  const ws = new WebSocket(`${base.replace('http', 'ws')}/ws/extension?token=${token}`);
  const runTurns = [];
  const received = [];
  const send = (o) => ws.send(JSON.stringify(o));

  async function answer(m) {
    const prompt = m.prompt ?? '';
    const d = (o) => send({ type: 'DOM_DELTA', jobId: m.jobId, ...o });
    if (gate) await gate(m);
    // A recovery retry's prompt carries [USER REQUEST]; the *_ONCE markers only bite the first try.
    const firstTry = !prompt.includes('[USER REQUEST]');
    if (prompt.includes('[[BLOCK_ONCE]]') && firstTry) {
      await sleep(delayMs);
      return send({ type: 'TURN_DONE', jobId: m.jobId, error: JSON.stringify({ error: { code: 400, message: 'Request blocked by policy' } }, null, 2) });
    }
    if (prompt.startsWith('Write one prompt for an AI image generator')) {
      d({ textDelta: 'Prompt: "A red apple on a white table"' });
      await sleep(delayMs);
      return send({ type: 'TURN_DONE', jobId: m.jobId, text: 'Prompt: "A red apple on a white table"', activeModel: 'gemini-test-flash' });
    }
    if (prompt.includes('[[BRIEF]]') && !prompt.includes('[TOOL_RESULT')) {
      // Gemini's other way of not drawing: a design brief with a prompt, no refusal words, no tool call.
      const brief = '## Sticker concept\n\n' + 'A cute character in pastel colours, bold outlines, waving. '.repeat(8) + '\n\nPrompt: chibi girl sticker';
      d({ textDelta: brief });
      await sleep(delayMs);
      return send({ type: 'TURN_DONE', jobId: m.jobId, text: brief, activeModel: 'gemini-test-flash' });
    }
    if (prompt.includes('[[IMAGE_REFUSE]]')) {
      const refusal = "I can't generate images here, but here is a prompt you can use elsewhere: a red apple.";
      d({ textDelta: refusal });
      await sleep(delayMs);
      return send({ type: 'TURN_DONE', jobId: m.jobId, text: refusal, activeModel: 'gemini-test-flash' });
    }
    if (prompt.includes('[[REFUSE_ONCE]]') && firstTry) {
      const refusal = 'Sorry, I could not respond to this request.';
      d({ textDelta: refusal });
      await sleep(delayMs);
      return send({ type: 'TURN_DONE', jobId: m.jobId, text: refusal, activeModel: 'gemini-test-flash' });
    }
    if (prompt.includes('[[FAIL]]')) {
      await sleep(delayMs);
      return send({ type: 'TURN_DONE', jobId: m.jobId, error: 'fake failure requested' });
    }
    // [[CALL]]{"name":..,"arguments":{..}}[[/CALL]] (one or more): answer with <tool_call> text until a tool result is in the prompt.
    const calls = [...prompt.matchAll(/\[\[CALL\]\]([\s\S]*?)\[\[\/CALL\]\]/g)].map((x) => x[1].trim());
    if (calls.length && !prompt.includes('[TOOL_RESULT id=')) {
      const out = calls.map((c) => `<tool_call>${c}</tool_call>`).join('\n');
      d({ textDelta: out });
      await sleep(delayMs);
      return send({ type: 'TURN_DONE', jobId: m.jobId, text: out, activeModel: 'gemini-test-flash' });
    }
    d({ thoughtDelta: 'Thinking about it. ' });
    await sleep(delayMs);
    const chunks = prompt.includes('[[TOOL]]') ? TOOL_CHUNKS : ['Hello ', 'from the ', 'fake ', 'extension.'];
    for (const c of chunks) {
      d({ textDelta: c });
      await sleep(delayMs);
    }
    send({ type: 'TURN_DONE', jobId: m.jobId, text: chunks.join(''), activeModel: 'gemini-test-flash' });
  }

  ws.addEventListener('message', (ev) => {
    const m = JSON.parse(ev.data);
    received.push(m);
    if (m.type === 'REQUEST_MODELS_SYNC') send({ type: 'MODELS_SYNC', models: MODELS });
    else if (m.type === 'RUN_IMAGE') {
      setTimeout(() => send(m.prompt.includes('[[FAIL]]')
        ? { type: 'IMAGE_DONE', jobId: m.jobId, error: 'Gemini returned no image' }
        : { type: 'IMAGE_DONE', jobId: m.jobId, mime: 'image/png', b64: FAKE_PNG_B64 }), delayMs);
    }
    else if (m.type === 'RUN_TURN') {
      runTurns.push(m);
      answer(m);
    }
  });

  await new Promise((res, rej) => {
    ws.addEventListener('open', res, { once: true });
    ws.addEventListener('error', () => rej(new Error(`${name}: ws connect failed`)), { once: true });
  });
  send({ type: 'TABS_SYNC', tabs: makeTabs(tabIds) });
  send({ type: 'MODELS_SYNC', models: MODELS });

  const closed = new Promise((r) => ws.addEventListener('close', r, { once: true }));
  return { name, ws, runTurns, received, send, closed, close: () => ws.close() };
}

export async function waitFor(cond, what, timeoutMs = 5000) {
  const end = Date.now() + timeoutMs;
  for (;;) {
    const v = await cond();
    if (v) return v;
    if (Date.now() > end) throw new Error(`timeout waiting for: ${what}`);
    await sleep(25);
  }
}
