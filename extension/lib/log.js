// Structured, redacting logger shared by the service worker, content script and popup.
// Classic script (importScripts / content_scripts / <script>); safe to load more than once.
//  - service worker ("bg"): ring buffer + chrome.storage.local persistence + EXT_LOG frames to the server
//  - content script / popup: forwards each entry to the background via runtime.sendMessage({type:'LOG'})
var GeBridgeLog = globalThis.GeBridgeLog || (() => {
  const LEVELS = ['debug', 'info', 'warn', 'error'];
  const SOURCES = ['bg', 'content', 'popup'];
  const MSG_MAX = 500;
  const DATA_MAX = 4096;
  const RING_CAP = 2000;
  const FRAME_MAX = 500;
  const PENDING_CAP = 50;
  const STORE_KEY = 'geLogBuffer';
  const MAX_DEPTH = 5;
  const MAX_ITEMS = 50;

  // ---------- redaction ----------

  const URL_RE = /https?:\/\/[^\s"'<>)\]]+/g;
  const JWT_RE = /\beyJ[A-Za-z0-9_-]{4,}\.[A-Za-z0-9_-]{4,}\.[A-Za-z0-9_-]*/g;
  const SENSITIVE_KEY_RE = /token|authoriz|xsrf|cookie|secret|passw|credential|^session(name|_name|id)?$/i;

  function redactUrl(raw) {
    const m = /^([^?#]*)(?:\?([^#]*))?(?:#.*)?$/.exec(raw);
    if (!m) return '[url]';
    let base = m[1]
      .replace(/(\/cid\/)([^/\s]{4})[^/\s]*/g, '$1$2…')
      .replace(/(\/sessions\/)[^/\s]+/g, '$1[redacted]');
    const query = m[2];
    if (query === undefined) return base;
    const names = query.split('&').filter(Boolean).map((p) => {
      const i = p.indexOf('=');
      const k = i < 0 ? p : p.slice(0, i);
      const v = i < 0 ? '' : p.slice(i + 1);
      return /^csesidx$/i.test(k) ? `${k}=${v.slice(0, 3)}…` : k;
    });
    return base + (names.length ? `?${names.join('&')}` : '');
  }

  function redactString(input) {
    let s = String(input);
    s = s.replace(URL_RE, (u) => {
      const tail = (/[.,;:!]+$/.exec(u) || [''])[0];
      return redactUrl(tail ? u.slice(0, -tail.length) : u) + tail;
    });
    s = s.replace(/gemb_[A-Za-z0-9_-]*/g, 'gemb_[redacted]');
    s = s.replace(JWT_RE, '[jwt]');
    s = s.replace(/\b((?:proxy-)?authorization)(["']?\s*[:=]\s*["']?)(?:(?:bearer|basic)\s+)?[^\s"',;]+/gi, '$1$2[redacted]');
    s = s.replace(/\bBearer\s+[A-Za-z0-9._~+/=-]{8,}/g, 'Bearer [redacted]');
    s = s.replace(/\b(\w*xsrf\w*)(["']?\s*[:=]\s*["']?)[^\s"',;&]+/gi, '$1$2[redacted]');
    s = s.replace(/\b((?:set-)?cookie)(["']?\s*[:=]\s*)("[^"]*"|'[^']*'|[^\n]*)/gi, '$1$2[redacted]');
    s = s.replace(/\b(__Secure-\w+|S?APISID|HSID|SSID|SID)=[^;\s"']+/g, '$1=[redacted]');
    s = s.replace(/(\bsessions\/)[^\s/"'?,;]+/g, '$1[redacted]');
    s = s.replace(/(\bcid[=:/]\s*)([0-9a-fA-F]{4})[0-9a-fA-F-]{8,}/gi, '$1$2…');
    s = s.replace(/(\bcsesidx[=:]\s*)(\d{3})\d+/gi, '$1$2…');
    return s;
  }

  function redact(value, depth = 0) {
    if (value === null || value === undefined) return value;
    const t = typeof value;
    if (t === 'string') return redactString(value);
    if (t === 'number' || t === 'boolean') return value;
    if (t === 'bigint') return String(value);
    if (t === 'function' || t === 'symbol') return `[${t}]`;
    if (value instanceof Error || Object.prototype.toString.call(value) === '[object Error]') {
      return { name: value.name, message: redactString(value.message), stack: redactString(value.stack || '') };
    }
    if (depth >= MAX_DEPTH) return '[depth]';
    if (Array.isArray(value)) {
      const out = value.slice(0, MAX_ITEMS).map((v) => redact(v, depth + 1));
      if (value.length > MAX_ITEMS) out.push(`[+${value.length - MAX_ITEMS} more]`);
      return out;
    }
    const out = {};
    let n = 0;
    for (const k of Object.keys(value)) {
      if (n++ >= MAX_ITEMS) { out['[truncated]'] = true; break; }
      const v = value[k];
      if (typeof v === 'boolean') out[k] = v;
      else if (SENSITIVE_KEY_RE.test(k)) out[k] = '[redacted]';
      else if (/^cid$/i.test(k) && typeof v === 'string') out[k] = `${v.slice(0, 4)}…`;
      else if (/^csesidx$/i.test(k) && (typeof v === 'string' || typeof v === 'number')) out[k] = `${String(v).slice(0, 3)}…`;
      else out[k] = redact(v, depth + 1);
    }
    return out;
  }

  function capData(data) {
    if (data === undefined) return undefined;
    const red = redact(data);
    let json;
    try { json = JSON.stringify(red); } catch { return { unserializable: true }; }
    if (json === undefined) return undefined;
    if (json.length <= DATA_MAX) return red;
    return { truncated: true, size: json.length, preview: json.slice(0, 3500) };
  }

  function makeEntry({ level, src, msg, data, tabId, ts } = {}) {
    const entry = {
      ts: Number.isFinite(ts) ? ts : Date.now(),
      level: LEVELS.includes(level) ? level : 'info',
      src: SOURCES.includes(src) ? src : 'bg'
    };
    const tid = Number.isInteger(tabId) ? tabId : (data && Number.isInteger(data.tabId) ? data.tabId : undefined);
    if (tid !== undefined) entry.tabId = tid;
    entry.msg = redactString(msg === undefined ? '' : msg).slice(0, MSG_MAX);
    const d = capData(data);
    if (d !== undefined) entry.data = d;
    return entry;
  }

  // ---------- ring buffer ----------

  function createRing(cap = RING_CAP) {
    let items = [];
    return {
      push(x) {
        items.push(x);
        if (items.length > cap) items.splice(0, items.length - cap);
      },
      toArray() { return items.slice(); },
      clear() { items = []; },
      get size() { return items.length; }
    };
  }

  // ---------- logger ----------

  function createLogger(opts = {}) {
    const mode = opts.mode === 'bg' ? 'bg' : 'forward';
    const src = opts.src || (mode === 'bg' ? 'bg' : 'content');
    const cap = opts.capacity || RING_CAP;
    const storage = opts.storage || null;
    const forward = opts.forward || null;
    const echo = Boolean(opts.echo);
    const ring = createRing(cap);
    let unsent = [];
    let sender = null;
    let persistTimer = null;
    let flushTimer = null;
    let pending = [];
    let pumping = false;

    function persistSoon() {
      if (!storage || persistTimer) return;
      persistTimer = setTimeout(() => {
        persistTimer = null;
        Promise.resolve(storage.set({ [STORE_KEY]: ring.toArray() })).catch((e) => {
          if (echo) console.warn('[ge-bridge] log persist failed', e && e.message);
        });
      }, 3000);
    }

    async function pump() {
      if (pumping || !forward) return;
      pumping = true;
      let failed = false;
      try {
        while (pending.length) {
          try {
            await forward(pending[0]);
          } catch {
            failed = true;
            break;
          }
          pending.shift();
        }
      } finally {
        pumping = false;
        if (failed && pending.length) setTimeout(pump, 2000);
      }
    }

    function record(entry) {
      if (mode === 'bg') {
        ring.push(entry);
        unsent.push(entry);
        if (unsent.length > cap) unsent.splice(0, unsent.length - cap);
        persistSoon();
      } else {
        pending.push(entry);
        if (pending.length > PENDING_CAP) pending.splice(0, pending.length - PENDING_CAP);
        pump();
      }
      return entry;
    }

    function log(level, msg, data) {
      let entry;
      try {
        entry = makeEntry({ level, src, msg, data, ts: opts.now ? opts.now() : undefined });
      } catch (e) {
        entry = { ts: Date.now(), level: 'error', src, msg: `log failure: ${String(e && e.message).slice(0, 200)}` };
      }
      if (echo && typeof console !== 'undefined') {
        const fn = console[level] || console.log;
        // Stringify: chrome://extensions renders raw objects as "[object Object]"
        let shown = '';
        if (entry.data !== undefined) {
          try { shown = JSON.stringify(entry.data); } catch { shown = String(entry.data); }
        }
        fn.call(console, `[ge-bridge ${src}]`, entry.msg, shown);
      }
      return record(entry);
    }

    // Accept an entry produced in another context (content/popup) into the bg buffer.
    function ingest(raw, tabId) {
      if (mode !== 'bg' || !raw || typeof raw !== 'object') return null;
      const entry = makeEntry({
        level: raw.level,
        src: raw.src === 'popup' ? 'popup' : 'content',
        msg: typeof raw.msg === 'string' ? raw.msg : '',
        data: raw.data,
        tabId: Number.isInteger(tabId) ? tabId : raw.tabId,
        ts: raw.ts
      });
      return record(entry);
    }

    function flush() {
      if (mode !== 'bg' || !sender) return 0;
      let sent = 0;
      while (unsent.length) {
        const batch = unsent.slice(0, FRAME_MAX);
        let ok = false;
        try { ok = sender({ type: 'EXT_LOG', entries: batch }); } catch { ok = false; }
        if (!ok) break;
        unsent.splice(0, batch.length);
        sent += batch.length;
      }
      return sent;
    }

    async function restore() {
      if (!storage) return 0;
      let prior = [];
      try {
        const d = await storage.get(STORE_KEY);
        if (Array.isArray(d && d[STORE_KEY])) prior = d[STORE_KEY];
      } catch { return 0; }
      const cur = ring.toArray();
      ring.clear();
      for (const e of prior.slice(-cap)) {
        if (e && typeof e === 'object' && typeof e.msg === 'string') ring.push(e);
      }
      for (const e of cur) ring.push(e);
      return prior.length;
    }

    function start(intervalMs = 2000) {
      if (mode === 'bg' && !flushTimer) flushTimer = setInterval(flush, intervalMs);
    }

    function lastError() {
      const all = ring.toArray();
      for (let i = all.length - 1; i >= 0; i--) {
        if (all[i].level === 'error') return { ts: all[i].ts, msg: all[i].msg };
      }
      return null;
    }

    function installGlobalHandlers(target) {
      if (!target || typeof target.addEventListener !== 'function') return false;
      target.addEventListener('error', (e) => {
        log('error', 'uncaught error', {
          message: e.message, file: e.filename, line: e.lineno, col: e.colno, stack: e.error && e.error.stack
        });
      });
      target.addEventListener('unhandledrejection', (e) => {
        const r = e.reason;
        log('error', 'unhandled rejection', r instanceof Error ? r : { reason: String(r) });
      });
      return true;
    }

    const api = {
      debug: (m, d) => log('debug', m, d),
      info: (m, d) => log('info', m, d),
      warn: (m, d) => log('warn', m, d),
      error: (m, d) => log('error', m, d),
      log,
      ingest,
      flush,
      start,
      restore,
      lastError,
      installGlobalHandlers,
      attachSender(fn) { sender = fn; },
      entries: () => ring.toArray(),
      count: () => ring.size,
      unsentCount: () => unsent.length,
      toJsonl: () => ring.toArray().map((e) => JSON.stringify(e)).join('\n') + '\n',
      clear() { ring.clear(); unsent = []; }
    };
    return api;
  }

  // ---------- default instance for the current context ----------

  const hasChrome = typeof chrome !== 'undefined' && chrome.runtime;
  const isWorker = typeof importScripts === 'function' && typeof window === 'undefined';
  const isPopup = !isWorker && typeof location !== 'undefined' && location.protocol === 'chrome-extension:';
  const instance = createLogger(isWorker
    ? { mode: 'bg', src: 'bg', echo: true, storage: hasChrome && chrome.storage ? chrome.storage.local : null }
    : {
      mode: 'forward',
      src: isPopup ? 'popup' : 'content',
      echo: hasChrome,
      forward: hasChrome ? (entry) => chrome.runtime.sendMessage({ type: 'LOG', entry }) : null
    });
  if (hasChrome) instance.installGlobalHandlers(globalThis);

  return {
    ...instance,
    redact, redactString, redactUrl, makeEntry, createLogger, createRing,
    LEVELS, RING_CAP, FRAME_MAX, MSG_MAX, DATA_MAX
  };
})();
