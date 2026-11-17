(() => {
  const noop = () => {};
  const log = typeof GeBridgeLog !== 'undefined' ? GeBridgeLog : { debug: noop, info: noop, warn: noop, error: noop };
  log.info('content script start', {
    href: location.href,
    readyState: document.readyState,
    isTopFrame: window === window.top,
    alreadyLoaded: Boolean(window.__GE_BRIDGE_V2_LOADED__),
    hasLib: typeof GeBridgeLib !== 'undefined'
  });
  if (window.__GE_BRIDGE_V2_LOADED__) {
    log.info('content script already loaded, skipping init');
    return;
  }
  window.__GE_BRIDGE_V2_LOADED__ = true;
  try {
    main();
  } catch (err) {
    log.error('content script init failed', err);
    throw err;
  }

  function main() {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  let port = null;
  // Calibration per page kind: the new-chat page and a session page host the composer under
  // different parents (ucs-chat-landing vs ucs-results), so one shadow path cannot serve both.
  let calStore = {};
  let activeModelSlug = null;
  const pageKind = () => (/\/r\/session\//.test(location.pathname) ? 'session' : 'home');
  const curCal = () => calStore[pageKind()] || null;

  chrome.storage.local.get(['calibratedV3', 'calibratedV2']).then((d) => {
    calStore = d.calibratedV3 || (d.calibratedV2 ? { home: d.calibratedV2 } : {});
    log.info('calibration loaded', { kinds: Object.keys(calStore) });
    setHud('ready');
  }).catch((err) => log.error('storage.get calibration failed', err));

  // Known Gemini Enterprise (ucs-*) elements; found by stable class/id/tag, independent of UI language.
  const KNOWN = {
    composer: (el) => el.classList?.contains('ProseMirror') && el.isContentEditable,
    sendBtn: (el) => el.classList?.contains('send-button') && el.classList.contains('submit'),
    modelBtn: (el) => el.id === 'model-selector-menu-anchor',
    modelItem: (el) => el.tagName === 'MD-MENU-ITEM' && el.getRootNode()?.host?.tagName === 'UCS-MODEL-SELECTOR',
    reply: (el) => el.tagName === 'UCS-RESPONSE-MARKDOWN'
  };
  // deepCollect reaches slotted nodes twice (as child and via <slot>), so dedupe.
  const findKnown = (key) => [...new Set(deepCollect((el) => KNOWN[key](el) && isVisible(el)))];

  // Compact Floating HUD on Gemini Enterprise
  const hud = document.createElement('div');
  hud.style.cssText = [
    'position:fixed', 'right:16px', 'bottom:14px', 'z-index:2147483647',
    'display:flex', 'align-items:center', 'gap:6px', 'padding:6px 11px',
    'border-radius:999px', 'background:rgba(9,13,22,0.92)', 'color:#e2e8f0',
    'border:1px solid rgba(59,130,246,0.4)', 'font:600 11.5px/1 "Segoe UI",system-ui,sans-serif',
    'box-shadow:0 8px 24px rgba(0,0,0,0.4)', 'backdrop-filter:blur(8px)'
  ].join(';');
  const hudText = document.createElement('span');
  const mkBtn = (txt, fn) => {
    const b = document.createElement('button');
    b.textContent = txt;
    b.style.cssText = 'background:#1e293b;color:#93c5fd;border:1px solid #334155;border-radius:999px;padding:3px 8px;font-size:11px;cursor:pointer;';
    b.onclick = fn;
    return b;
  };
  hud.append(hudText, mkBtn('🔄 Models', () => scanAvailableModels(true)), mkBtn('🎯 Calibrate', () => startCalibration()));
  document.documentElement.appendChild(hud);
  log.info('pill appended', { connected: hud.isConnected, parent: hud.parentNode && hud.parentNode.nodeName });
  setTimeout(() => {
    const r = hud.getBoundingClientRect();
    log.info('pill check after 5s', { connected: hud.isConnected, width: r.width, height: r.height });
  }, 5000);

  function setHud(mode, msg) {
    if (mode === 'busy') hudText.textContent = `⚡ ${msg}`;
    else if (mode === 'cal') hudText.textContent = `🎯 ${msg}`;
    else hudText.textContent = `🟢 GeBridge v2${activeModelSlug ? ' • ' + activeModelSlug : ''}`;
  }
  setHud('ready');

  function ensurePort() {
    try {
      port = chrome.runtime.connect({ name: 'ge-v2-port' });
      log.info('port connect');
      port.postMessage({ type: 'TAB_READY' });
      port.onDisconnect.addListener(() => {
        log.warn('port disconnected', { lastError: chrome.runtime.lastError && chrome.runtime.lastError.message });
        port = null;
        setTimeout(ensurePort, 1500);
      });
    } catch (err) {
      log.warn('port connect failed, retrying', { error: err.message });
      setTimeout(ensurePort, 2000);
    }
  }
  ensurePort();

  // Deep Shadow-DOM + <slot> Walker
  function deepCollect(pred, root = document, acc = []) {
    const walk = (node) => {
      if (!node) return;
      if (node.nodeType === Node.ELEMENT_NODE) {
        if (pred(node)) acc.push(node);
        if (node.shadowRoot) walk(node.shadowRoot);
      }
      const kids = node.tagName === 'SLOT' && node.assignedNodes
        ? node.assignedNodes({ flatten: true })
        : node.childNodes;
      if (kids) for (let i = 0; i < kids.length; i++) walk(kids[i]);
    };
    walk(root);
    return acc;
  }

  function isVisible(el) {
    if (!el?.getBoundingClientRect) return false;
    const r = el.getBoundingClientRect();
    if (r.width < 6 || r.height < 6) return false;
    const s = getComputedStyle(el);
    return s.display !== 'none' && s.visibility !== 'hidden' && Number(s.opacity) > 0.05;
  }

  const { toModelSlug, toApiModelId } = GeBridgeLib;
  const itemLabel = (el) => (el.innerText || el.textContent || '').split('\n').map((x) => x.trim()).filter(Boolean)[0] || '';

  function buildShadowPath(el) {
    const segs = [];
    let cur = el;
    while (cur) {
      let s = cur.tagName.toLowerCase();
      if (cur.id) s += `#${CSS.escape(cur.id)}`;
      else if (cur.getAttribute('aria-label')) s += `[aria-label="${CSS.escape(cur.getAttribute('aria-label'))}"]`;
      else if (cur.getAttribute('role')) s += `[role="${CSS.escape(cur.getAttribute('role'))}"]`;
      segs.unshift(s);
      const rn = cur.getRootNode();
      if (rn instanceof ShadowRoot) { segs.unshift('>>>'); cur = rn.host; } else break;
    }
    const groups = [];
    let buf = [];
    for (const x of segs) {
      if (x === '>>>') { groups.push(buf.join(' ')); buf = []; } else buf.push(x);
    }
    if (buf.length) groups.push(buf.join(' '));
    return groups.join(' >>> ');
  }

  function resolveShadowPath(pathStr) {
    if (!pathStr) return null;
    const parts = pathStr.split(' >>> ').map((s) => s.trim()).filter(Boolean);
    let roots = [document];
    for (let i = 0; i < parts.length; i++) {
      const next = [];
      for (const r of roots) {
        for (const m of r.querySelectorAll(parts[i])) {
          if (i < parts.length - 1) { if (m.shadowRoot) next.push(m.shadowRoot); }
          else next.push(m);
        }
      }
      roots = next;
    }
    const vis = roots.filter(isVisible);
    return vis[vis.length - 1] || null;
  }

  // Locate Gemini Enterprise Model Picker Trigger
  function findModelPickerButton() {
    const known = findKnown('modelBtn');
    if (known[0]) return known[0];
    if (curCal()?.modelBtn) {
      const hit = resolveShadowPath(curCal().modelBtn);
      if (hit) return hit;
    }
    const btns = deepCollect((el) => {
      if (!isVisible(el)) return false;
      if (el.tagName !== 'BUTTON' && el.getAttribute('role') !== 'button' && el.getAttribute('role') !== 'combobox') return false;
      const txt = `${el.getAttribute('aria-label') || ''} ${el.textContent || ''}`.toLowerCase();
      return /gemini|model|flash|pro|argon/i.test(txt) && txt.length < 70;
    });
    return btns[0] || null;
  }

  // Menu entries of the model picker: the known ucs-model-selector items, else any visible menu option.
  function modelMenuItems() {
    const known = findKnown('modelItem');
    if (known.length) return known;
    return deepCollect((el) => isVisible(el) && /menuitem|option|radio/i.test(el.getAttribute('role') || ''));
  }

  // The picker button drops the "· Preview" suffix its menu item shows, so ignore it when comparing.
  const sameModel = (a, b) => a === b || `${a}-preview` === b || `${b}-preview` === a;

  async function openModelMenu(picker) {
    picker.click();
    await sleep(350);
  }

  function closeMenu() {
    document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, composed: true }));
  }

  // Scan live available models from the Gemini Enterprise Model Picker UI.
  // Model ids are the API modelIds (what widgetStreamAssist's assistGenerationConfig.modelId takes).
  async function scanAvailableModels(forceOpenMenu = false) {
    const picker = findModelPickerButton();
    let opened = false;
    if (picker) {
      const label = itemLabel(picker);
      if (label) activeModelSlug = toApiModelId(label);
      if (forceOpenMenu) {
        await openModelMenu(picker);
        opened = true;
      }
    }

    const seen = new Map();
    for (const opt of modelMenuItems()) {
      const title = itemLabel(opt);
      if (!/gemini|flash|pro|argon|ultra/i.test(title)) continue;
      const id = toApiModelId((opt.innerText || opt.textContent || '').replace(/\s*\n\s*/g, ' · '));
      if (!id || id.length < 4) continue;
      const disabled = opt.getAttribute('aria-disabled') === 'true' || opt.hasAttribute('disabled') || /upgrade|locked/i.test(opt.textContent || '');
      seen.set(id, {
        id,
        display_name: title,
        available: !disabled,
        supports_thinking: !/lite/i.test(id),
        efforts: /argon|pro/i.test(id) ? ['auto', 'low', 'medium', 'high', 'max'] : ['auto', 'low', 'medium', 'high']
      });
    }
    if (opened) closeMenu();
    log.info('model scan', { opened, active: activeModelSlug, models: [...seen.keys()] });
    if (seen.size > 0 && port) port.postMessage({ type: 'MODELS_FOUND', models: Array.from(seen.values()) });
    setHud('ready');
  }

  // Switch model (targetModel = API modelId or 'auto') in the Gemini Enterprise UI before sending.
  async function ensureModelSelected(targetModel, targetEffort) {
    if (!targetModel) return;
    const picker = findModelPickerButton();
    if (!picker) {
      log.warn('model picker not found, keeping current model', { targetModel });
      return;
    }
    const current = toApiModelId(itemLabel(picker));
    if (!sameModel(current, targetModel)) {
      await openModelMenu(picker);
      const match = modelMenuItems().find((el) => {
        const id = toApiModelId((el.innerText || el.textContent || '').replace(/\s*\n\s*/g, ' · '));
        return id && sameModel(id, targetModel);
      });
      if (!match) {
        closeMenu();
        throw new Error(`Model "${targetModel}" is not in the Gemini model picker. Scan models and pick one that exists.`);
      }
      if (match.getAttribute('aria-disabled') === 'true' || match.hasAttribute('disabled')) {
        closeMenu();
        throw new Error(`Model "${targetModel}" is disabled for your enterprise account.`);
      }
      match.click();
      activeModelSlug = targetModel;
      await sleep(300);
    }

    if (targetEffort && targetEffort !== 'auto') {
      const effortBtns = deepCollect((el) => {
        if (!isVisible(el)) return false;
        const txt = `${el.getAttribute('aria-label') || ''} ${el.textContent || ''}`.trim().toLowerCase();
        return txt === targetEffort;
      });
      if (effortBtns[0]) {
        effortBtns[0].click();
        await sleep(140);
      }
    }
  }

  function findComposer() {
    const known = findKnown('composer');
    if (known.length) return known[known.length - 1];
    if (curCal()?.composer) {
      const hit = resolveShadowPath(curCal().composer);
      if (hit) return hit;
    }
    const list = deepCollect((el) => {
      if (!isVisible(el)) return false;
      return el.tagName === 'TEXTAREA' || (el.isContentEditable && (el.getAttribute('contenteditable') === 'true' || el.getAttribute('role') === 'textbox'));
    });
    list.sort((a, b) => (b.getBoundingClientRect().bottom + b.getBoundingClientRect().width) - (a.getBoundingClientRect().bottom + a.getBoundingClientRect().width));
    return list[0] || null;
  }

  function findSendButton(composer) {
    const known = findKnown('sendBtn').find((b) => !b.disabled && !b.hasAttribute('disabled'));
    if (known) return known;
    if (curCal()?.sendBtn) {
      const hit = resolveShadowPath(curCal().sendBtn);
      if (hit && !hit.disabled) return hit;
    }
    const cr = composer.getBoundingClientRect();
    const btns = deepCollect((el) => {
      if (!isVisible(el)) return false;
      if (el.tagName !== 'BUTTON' && el.getAttribute('role') !== 'button') return false;
      const lbl = `${el.getAttribute('aria-label') || ''} ${el.title || ''} ${el.textContent || ''}`.toLowerCase();
      return /send|submit|ask|arrow_upward/i.test(lbl) || el.type === 'submit';
    });
    btns.sort((a, b) => Math.hypot(a.getBoundingClientRect().left - cr.right, a.getBoundingClientRect().top - cr.top)
      - Math.hypot(b.getBoundingClientRect().left - cr.right, b.getBoundingClientRect().top - cr.top));
    return btns.find((b) => !b.disabled && b.getAttribute('aria-disabled') !== 'true') || null;
  }

  function clickNewChatIfPresent() {
    const btns = deepCollect((el) => {
      if (!isVisible(el)) return false;
      if (el.tagName !== 'BUTTON' && el.tagName !== 'A' && el.getAttribute('role') !== 'button') return false;
      const lbl = `${el.getAttribute('aria-label') || ''} ${el.title || ''} ${el.textContent || ''}`.toLowerCase();
      return /new chat|new conversation|start new/i.test(lbl);
    });
    if (btns[0]) { btns[0].click(); return true; }
    return false;
  }

  function isStillGenerating() {
    return deepCollect((el) => {
      if (!isVisible(el)) return false;
      const lbl = `${el.getAttribute('aria-label') || ''} ${el.title || ''}`.toLowerCase();
      return /stop generating|stop response|cancel/i.test(lbl) || el.getAttribute('role') === 'progressbar';
    }).length > 0;
  }

  function getReplyContainers() {
    const known = findKnown('reply');
    if (known.length) return known;
    const rc = curCal();
    if (rc?.replyTag) {
      const custom = deepCollect((el) => isVisible(el) && el.tagName.toLowerCase() === rc.replyTag);
      if (custom.length) return custom;
    }
    return deepCollect((el) => {
      if (!isVisible(el)) return false;
      const t = el.tagName.toLowerCase();
      const c = (typeof el.className === 'string' ? el.className : '').toLowerCase();
      return t.includes('markdown') || t.includes('response') || t.includes('model-turn')
        || c.includes('markdown') || c.includes('model-response') || c.includes('assistant');
    });
  }

  // Separate Gemini CoT / Thinking block from main answer Markdown
  function extractThoughtAndAnswer(container) {
    if (!container) return { thought: '', answer: '' };
    const clone = container.cloneNode(true);
    let thought = '';
    clone.querySelectorAll?.('details, [class*="thought"], [class*="thinking"], [data-test-id*="thought"]').forEach((tNode) => {
      thought += (tNode.innerText || tNode.textContent || '').trim() + '\n';
      tNode.remove();
    });
    clone.querySelectorAll?.('button, nav, [aria-hidden="true"], .citation-chip').forEach((n) => n.remove());
    return {
      thought: thought.trim(),
      answer: (clone.innerText || clone.textContent || '').trim()
    };
  }

  function fillComposer(composer, text) {
    composer.focus();
    if (composer.tagName === 'TEXTAREA') {
      const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')?.set;
      if (setter) setter.call(composer, text); else composer.value = text;
      composer.dispatchEvent(new InputEvent('input', { bubbles: true, composed: true, data: text, inputType: 'insertText' }));
      return;
    }
    const sel = window.getSelection();
    const range = document.createRange();
    range.selectNodeContents(composer);
    sel.removeAllRanges();
    sel.addRange(range);
    if (!document.execCommand('insertText', false, text)) {
      composer.textContent = text;
      composer.dispatchEvent(new InputEvent('input', { bubbles: true, composed: true, data: text, inputType: 'insertText' }));
    }
  }

  async function runTurn(job) {
    const turnStart = Date.now();
    log.info('RUN_TURN start', {
      jobId: job.jobId, targetModel: job.targetModel, effort: job.reasoningEffort,
      newChat: Boolean(job.newChat), promptChars: (job.prompt || '').length, images: (job.images || []).length
    });
    setHud('busy', `${job.targetModel || activeModelSlug || 'Running'}...`);
    try {
      if (job.newChat && clickNewChatIfPresent()) {
        await sleep(600);
      }
      if (job.targetModel || job.reasoningEffort) {
        await ensureModelSelected(job.targetModel, job.reasoningEffort);
      }

      const composer = findComposer();
      if (!composer) throw new Error('Chat input box not found. Click 🎯 Calibrate on the bottom-right pill.');

      const beforeBlocks = getReplyContainers();
      const beforeCount = beforeBlocks.length;
      const beforeLast = beforeCount ? extractThoughtAndAnswer(beforeBlocks[beforeCount - 1]).answer : '';

      fillComposer(composer, job.prompt);
      await sleep(140);

      const sendBtn = findSendButton(composer);
      if (sendBtn) sendBtn.click();
      else composer.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', code: 'Enter', keyCode: 13, bubbles: true, composed: true }));

      let sentAnswerLen = 0;
      let sentThoughtLen = 0;
      let lastAnswer = '';
      let lastThought = '';
      let stableAt = 0;
      const t0 = Date.now();

      while (Date.now() - t0 < 175000) {
        await sleep(95);
        const blocks = getReplyContainers();
        let cur = { thought: '', answer: '' };

        if (blocks.length > beforeCount) {
          cur = extractThoughtAndAnswer(blocks[blocks.length - 1]);
        } else if (blocks.length > 0) {
          const candidate = extractThoughtAndAnswer(blocks[blocks.length - 1]);
          if (candidate.answer && candidate.answer !== beforeLast && candidate.answer !== job.prompt.trim()) {
            cur = candidate;
          }
        }

        if ((cur.answer && cur.answer !== job.prompt.trim()) || cur.thought) {
          if (cur.answer !== lastAnswer || cur.thought !== lastThought) {
            const thoughtDelta = cur.thought.startsWith(lastThought) ? cur.thought.slice(sentThoughtLen) : '';
            const textDelta = cur.answer.startsWith(lastAnswer) ? cur.answer.slice(sentAnswerLen) : '';
            if ((thoughtDelta || textDelta) && port) {
              port.postMessage({ type: 'DOM_DELTA', jobId: job.jobId, thoughtDelta, textDelta });
            }
            sentThoughtLen = cur.thought.length;
            sentAnswerLen = cur.answer.length;
            lastThought = cur.thought;
            lastAnswer = cur.answer;
            stableAt = Date.now();
          } else if (lastAnswer && !isStillGenerating() && Date.now() - stableAt >= 1050) {
            break;
          }
        }
      }

      if (!lastAnswer) throw new Error('Timed out waiting for Gemini Enterprise reply bubble. Click 🎯 Calibrate once.');
      if (port) {
        port.postMessage({
          type: 'TURN_DONE',
          jobId: job.jobId,
          text: lastAnswer,
          activeModel: activeModelSlug
        });
      }
      log.info('RUN_TURN end', { jobId: job.jobId, ms: Date.now() - turnStart, answerChars: lastAnswer.length, portOpen: Boolean(port) });
    } catch (err) {
      log.error('RUN_TURN error', { jobId: job.jobId, ms: Date.now() - turnStart, error: err.message, portOpen: Boolean(port) });
      if (port) port.postMessage({ type: 'TURN_DONE', jobId: job.jobId, error: err.message });
    } finally {
      setHud('ready');
    }
  }

  // API transport from the page (same-origin cookies for getoxsrf). Deltas go to the background
  // one at a time (awaited, so they stay ordered); the final result is the message response.
  async function runApiTurnInPage(job) {
    const t0 = Date.now();
    let started = false;
    let chain = Promise.resolve();
    const sendDelta = (thoughtDelta, textDelta) => {
      started = true;
      chain = chain.then(() => chrome.runtime.sendMessage({ type: 'API_DELTA', jobId: job.jobId, thoughtDelta, textDelta }))
        .catch((err) => log.warn('API_DELTA send failed', { jobId: job.jobId, error: err.message }));
    };
    log.info('RUN_TURN_API start', { jobId: job.jobId, promptChars: (job.prompt || '').length, reusedSession: Boolean(job.session), images: (job.images || []).length });
    setHud('busy', 'API...');
    try {
      const client = GeBridgeLib.createGeminiClient({ target: job.target, origin: window.location.origin, log });
      const out = await GeBridgeLib.runApiTurn(client, job.session || null, job.prompt, {
        onThought: (d) => sendDelta(d, ''),
        onText: (d) => sendDelta('', d)
      }, { modelId: job.modelId, attachments: job.images || [] });
      await chain;
      log.info('RUN_TURN_API end', { jobId: job.jobId, ms: Date.now() - t0, answerChars: out.text.length });
      return { ok: true, text: out.text, session: out.session, started };
    } catch (err) {
      await chain;
      log.error('RUN_TURN_API error', { jobId: job.jobId, step: err.step, httpStatus: err.httpStatus, error: err.message });
      return { ok: false, error: err.message, step: err.step, httpStatus: err.httpStatus, network: Boolean(err.network), started };
    } finally {
      setHud('ready');
    }
  }

  // Visual calibrator (Composer -> Send -> Reply -> Model picker), saved per page kind (home / session).
  // Only needed when the known ucs-* selectors stop matching. Uses pointerdown because a disabled
  // button never fires click, and fills the composer so the send button (absent while empty) appears.
  const CAL_DUMMY = 'ge-bridge calibration';
  function startCalibration() {
    const kind = pageKind();
    const next = {};
    let step = 1;
    let composerEl = null;
    setHud('cal', `[${kind}] 1/4 Click the chat input (Esc = save and stop)`);

    const swallow = (e) => {
      if (hud.contains(e.target)) return;
      e.preventDefault();
      e.stopPropagation();
    };
    const onPointer = (e) => {
      if (hud.contains(e.target)) return;
      swallow(e);
      const el = e.composedPath()[0];
      if (step === 1) {
        composerEl = el.closest?.('[contenteditable="true"],textarea') || el;
        next.composer = buildShadowPath(composerEl);
        composerEl.focus();
        document.execCommand('insertText', false, CAL_DUMMY);
        step = 2;
        setHud('cal', `[${kind}] 2/4 Click the Send button (text was filled in so it shows)`);
      } else if (step === 2) {
        const btn = e.composedPath().find((n) => n.tagName === 'BUTTON' || n.getAttribute?.('role') === 'button') || el;
        next.sendBtn = buildShadowPath(btn);
        clearDummy();
        step = 3;
        setHud('cal', `[${kind}] 3/4 Click any AI reply text (Esc if this page has none)`);
      } else if (step === 3) {
        const host = e.composedPath().find((n) => n.tagName?.includes('-'));
        next.replyTag = (host || el).tagName.toLowerCase();
        step = 4;
        setHud('cal', `[${kind}] 4/4 Click the model selector (Esc to skip)`);
      } else if (step === 4) {
        const btn = e.composedPath().find((n) => n.tagName === 'BUTTON' || /button|combobox/.test(n.getAttribute?.('role') || '')) || el;
        next.modelBtn = buildShadowPath(btn);
        finish();
      }
    };
    const onKey = (e) => { if (e.key === 'Escape') finish(); };

    function clearDummy() {
      if (!composerEl) return;
      const txt = composerEl.value ?? composerEl.textContent ?? '';
      if (!txt.includes(CAL_DUMMY)) return;
      composerEl.focus();
      document.execCommand('selectAll');
      document.execCommand('delete');
    }

    function finish() {
      window.removeEventListener('pointerdown', onPointer, true);
      window.removeEventListener('mousedown', swallow, true);
      window.removeEventListener('click', swallow, true);
      window.removeEventListener('keydown', onKey, true);
      clearDummy();
      if (next.composer) {
        calStore = { ...calStore, [kind]: { ...(calStore[kind] || {}), ...next } };
        chrome.storage.local.set({ calibratedV3: calStore });
        log.info('calibration saved', { kind, keys: Object.keys(next) });
        if (port) port.postMessage({ type: 'TAB_READY' });
      }
      setHud('ready');
    }

    window.addEventListener('pointerdown', onPointer, true);
    window.addEventListener('mousedown', swallow, true);
    window.addEventListener('click', swallow, true);
    window.addEventListener('keydown', onKey, true);
  }

  chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
    log.debug('message received', { type: msg && msg.type });
    if (msg.type === 'PING_TAB') {
      sendResponse({ activeModel: activeModelSlug, calibrated: Boolean(curCal() || findKnown('composer').length) });
      return false;
    }
    if (msg.type === 'PING_GEMINI') {
      GeBridgeLib.runGeminiProbe(msg.target, { origin: window.location.origin }).then(sendResponse);
      return true;
    }
    if (msg.type === 'SCAN_MODELS') {
      scanAvailableModels(Boolean(msg.forceOpenMenu));
      sendResponse({ ok: true });
      return false;
    }
    if (msg.type === 'START_CALIBRATION') {
      startCalibration();
      sendResponse({ ok: true });
      return false;
    }
    if (msg.type === 'RUN_IMAGE_API') {
      const client = GeBridgeLib.createGeminiClient({ target: msg.target, origin: window.location.origin, log });
      GeBridgeLib.runImageGeneration(client, msg.prompt, msg.aspect, { modelId: msg.modelId })
        .then((r) => sendResponse({ ok: true, mime: r.mime, b64: r.b64, text: r.text }))
        .catch((err) => sendResponse({ ok: false, error: err.message, step: err.step, httpStatus: err.httpStatus, network: Boolean(err.network) }));
      return true;
    }
    if (msg.type === 'RUN_TURN_API') {
      runApiTurnInPage(msg).then(sendResponse);
      return true;
    }
    if (msg.type === 'RUN_TURN') {
      runTurn(msg);
      sendResponse({ accepted: true });
      return false;
    }
  });

  // Auto-discover active model badge silently after load
  setTimeout(() => scanAvailableModels(false), 1800);
  log.info('content script init complete');
  }
})();
