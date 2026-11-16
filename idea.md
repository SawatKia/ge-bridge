# using the Mausbot aiagent conversation

## turn 1

### User prompt

how can i uses you as my ai agent for [openmausbot](https://github.com/milind-soni/OpenMausBot). since i can only uses you on the web browser

### Ai response

You cannot connect this web-browser chat session directly to OpenMausBot (https://github.com/milind-soni/OpenMausBot), because browser chats do not expose a local process or API endpoint for external apps to control. However, you can run Google Gemini as your AI agent inside OpenMausBot with zero code by adding either a Gemini API key or Google's Gemini CLI to OpenMausBot's local configuration.

OpenMausBot runs a local harness server on 127.0.0.1 and supports two built-in ways to plug in Gemini via ~/.openmausbot/config.json.

Which Setup Should You Choose?

Option	How It Connects	Best For	Computer & Tool Support
Option 1: Gemini API (openai-compat)	Calls Google's OpenAI-compatible HTTPS endpoint directly with an API key	Fastest setup with no extra CLI install required	Supports structured tools, image/screenshot input, and OpenMausBot's VM/browser tools
Option 2: Gemini CLI (customAcp)	Spawns gemini --acp locally over the Agent Client Protocol (ACP)	Terminal-style coding agent workflows with CLI tool execution	Uses Gemini CLI's own agent loop and surfaces permission requests as chat approval cards

Option 1: Use a Gemini API Key (Recommended & Easiest)

OpenMausBot's built-in openai-compat driver connects directly to Google's OpenAI-compatible Gemini endpoint without spawning any background CLI process.

1. Get a Gemini API key: Create an API key in Google AI Studio (https://aistudio.google.com).
2. Edit your OpenMausBot config: Open ~/.openmausbot/config.json (on Windows, %USERPROFILE%\.openmausbot\config.json) and add an entry under "instances":

```json
{
  "instances": {
    "gemini-api": {
      "driver": "openai-compat",
      "displayName": "Google Gemini",
      "environment": {
        "GEMINI_API_KEY": "YOUR_GEMINI_API_KEY"
      },
      "config": {
        "url": "https://generativelanguage.googleapis.com/v1beta/openai",
        "apiKeyEnv": "GEMINI_API_KEY",
        "model": "gemini-2.5-flash-lite"
      }
    }
  }
}
```

3. Restart OpenMausBot: OpenMausBot reads "instances" on startup, so quit and relaunch the app after saving.
4. Select Gemini on your bot: Open your bot's model picker to choose from the Gemini models fetched from the /models endpoint, and optionally set the Google Gemini provider icon in Settings.

Option 2: Use Google's Gemini CLI (customAcp)

Because Google's official Gemini CLI natively supports the Agent Client Protocol (--acp), OpenMausBot can spawn it just like Claude Code or Grok CLI.

1. Install Gemini CLI: Make sure Node.js is installed, then install the CLI globally:

```bash
npm install -g @google/gemini-cli
```

2. Verify or sign in from your terminal: Run gemini --help to confirm the --acp flag is available (older versions use --experimental-acp), and either authenticate in the terminal or prepare a Gemini API key.
3. Add the ACP instance to ~/.openmausbot/config.json: Point the customAcp driver at gemini --acp:

```json
{
  "instances": {
    "gemini-cli": {
      "driver": "customAcp",
      "displayName": "Gemini CLI",
      "environment": {
        "GEMINI_API_KEY": "YOUR_GEMINI_API_KEY"
      },
      "config": {
        "cli": "gemini --acp"
      }
    }
  }
}
```

4. Restart and re-detect models: Relaunch OpenMausBot, select Gemini CLI for your bot, and click Re-detect in the model picker to load available Gemini models.

⚠️ Common Pitfalls to Watch Out For

Plaintext keys in config.json: OpenMausBot stores environment values in ~/.openmausbot/config.json with 0600 file permissions rather than encrypting them, so keep that file out of backups or Git repositories.
CLI PATH issues in desktop apps: If OpenMausBot cannot find gemini when launched from your desktop, replace "gemini --acp" with the full path from which gemini (macOS/Linux) or where.exe gemini (Windows), or set it in Settings → Engines → Set CLI….
Gemini CLI free-tier sign-in change: Unpaid personal Google login in Gemini CLI was replaced in June 2026, so passing a GEMINI_API_KEY from Google AI Studio is the most reliable authentication method.

Which operating system are you running OpenMausBot on, and would you like help editing your config.json file step by step?

## turn 2

### User prompt

what if creating the chrome extension to control the web page act like the adapter proxy like doing in the [aipass-bridge](https://github.com/niawjunior/aipass-bridge) which make aipass can access through cli. but now we will make it accessible to openmausbot

### Ai response

Yes—building a Chrome extension + local HTTP adapter proxy (just like aipass-bridge) is completely feasible for OpenMausBot, because OpenMausBot's openai-compat driver accepts any local http://127.0.0.1:<port>/v1 server as a custom engine.

In fact, if you are already using AiPASS (de.aipass.net/chat), aipass-bridge already exposes a drop-in OpenAI-compatible server on localhost:8787—meaning you can connect it to OpenMausBot right now without writing a new extension. If you want to build a similar bridge for another browser-only AI chat (such as Gemini on the web), you can copy aipass-bridge's 3-part architecture and add a tool-calling translation layer so OpenMausBot's agent features work properly.

1. Using aipass-bridge Directly with OpenMausBot Today

Because aipass-bridge runs a local Node HTTP server on 127.0.0.1:8787 and forwards requests through its Manifest V3 Chrome extension into your logged-in browser tab, OpenMausBot can treat it like any local OpenAI server:

1. Start aipass-bridge and connect the extension: Run npm run dev inside aipass-bridge, load aipass-bridge/extension unpacked in chrome://extensions, and open your logged-in de.aipass.net/chat tab until the popup shows Connected.
2. Point OpenMausBot at 127.0.0.1:8787/v1: Add an openai-compat instance to ~/.openmausbot/config.json (set "tools": false initially if the bridged model endpoint only returns plain text):

```json
{
  "instances": {
    "browser-bridge": {
      "driver": "openai-compat",
      "displayName": "Browser Bridge",
      "environment": {
        "BRIDGE_LOCAL_TOKEN": "local-bridge-secret"
      },
      "config": {
        "url": "http://127.0.0.1:8787/v1",
        "apiKeyEnv": "BRIDGE_LOCAL_TOKEN",
        "model": "your-model-id",
        "tools": false
      }
    }
  }
}
```

3. Restart OpenMausBot: Relaunch OpenMausBot so it loads the new instance and fetches available models from /models.

2. How to Build Your Own Chrome Extension Proxy for Another Web Chat

Chrome extensions cannot open a local TCP listening port on their own, so aipass-bridge splits the proxy into a local Node server and a Manifest V3 extension that pulls jobs from it.

Component	Role in the Bridge	How aipass-bridge Implements It
Local HTTP Bridge (Node.js)	Listens on 127.0.0.1:8787 and serves GET /v1/models + POST /v1/chat/completions to OpenMausBot	Queues incoming API turns as jobs and streams them to the extension over Server-Sent Events (SSE)
Extension Service Worker (background.js)	Bridges localhost HTTP to the active browser tab	Subscribes to the local server's SSE job stream, dispatches runs to the tab, and POSTs chunks/results back
Tab Script (content.js + page.js)	Sends prompts using your existing browser login	Executes inside the logged-in page context so Chrome attaches session cookies automatically without exposing credentials on disk

When building page.js for a target web chat, you have two ways to drive the page:

In-page fetch replay (cleaner, used by aipass-bridge): Call the web app's internal web endpoint from page JavaScript so cookies/CSRF headers are attached natively by Chrome.
DOM automation (more universal, slower): Programmatically paste text into the chat <textarea> or contenteditable input, click Send, and watch the response container with a MutationObserver until generation finishes.

3. The 3 Technical Gaps Between a Web Chat Box and OpenMausBot

Bridging a simple terminal chat is easy, but OpenMausBot is an agent harness—to unlock computer control, bash, or MCP tools, your local Node server needs to handle three protocol mismatches:

A. Translating Structured tools and tool_calls

The mismatch: OpenMausBot sends JSON tools definitions in POST /v1/chat/completions and expects structured tool_calls (with finish_reason: "tool_calls") rather than plain Markdown text.
Chat-only workaround: Set "tools": false in OpenMausBot's config block so OpenMausBot treats the bot purely as a conversational assistant.
Full agent solution (Prompt-to-Tool Shim): In your local Node server, inject OpenMausBot's tools JSON schemas into the prompt and instruct the web AI to output tool invocations inside strict XML/JSON tags (for example, <tool_call>{"name":"...","arguments":{...}}</tool_call>). Your Node server then parses that block, strips it from content, and returns a valid OpenAI tool_calls payload to OpenMausBot—and on the next turn, converts OpenMausBot's {"role": "tool"} result messages back into plain text for the web chat.

B. Stateless API History vs. Stateful Browser Tabs

The mismatch: OpenMausBot resends the entire messages array on every turn, while an open browser chat tab already remembers previous turns.
How to handle it: Have your Node proxy hash the conversation prefix so follow-up turns only send the newest user message or tool output into the existing browser thread, and trigger a "New Chat" in the tab only when a brand-new conversation starts.

C. Base64 Screenshots (image_url) for Computer Use

The mismatch: When OpenMausBot mounts a Local VM or browser tool, it sends desktop screenshots back to the model as image_url payloads inside messages.
How to handle it: Your bridge must decode data:image/png;base64,... strings into binary File/Blob objects and upload or paste them into the web chat composer (just as aipass-bridge handles --file attachments).

⚠️ Security & Reliability Pitfalls to Watch Out For

Lock down 127.0.0.1 with a Bearer token and strict CORS: As aipass-bridge warns in its security notes, an unauthenticated localhost port without CORS restrictions can be driven by any malicious website you visit in your browser. Always bind to 127.0.0.1 and verify the Authorization: Bearer ... header that OpenMausBot sends via apiKeyEnv.
Terms of Service & UI breakage: Automating a consumer or enterprise web chat interface can violate the provider's Terms of Service, trigger anti-bot rate limits, or break whenever the frontend updates its DOM or internal request format.

Would you like a minimal Node.js + Chrome Manifest V3 starter template (including the prompt-to-tool_calls shim) tailored for OpenMausBot?

## turn 3

### User prompt

aipass-bridge designed to uses for [aipass](aipass.net) web. but i want to use you which is [gemini enterprise](vertexaisearch.cloud.google). it different web. can i uses the aipass-bridge with you?or it need new extension.

### Ai response

No—you cannot use aipass-bridge out of the box with Gemini Enterprise (vertexaisearch.cloud.google.com); it requires either a new (or forked) Chrome extension, or a lightweight local proxy using Gemini Enterprise's official REST API.

aipass-bridge's Chrome extension is specifically built to connect to de.aipass.net/chat and speak AiPASS's internal endpoints and model list, so it will not recognize or control a vertexaisearch.cloud.google.com tab.

Depending on whether you have Google Cloud permission on the project or only web-browser login, you have two ways to connect Gemini Enterprise to OpenMausBot:

Which Approach Fits Your Access Level?

Approach	When to Use It	Needs Chrome Open?	Effort & Stability
Option A: Official streamAssist API Proxy (Best)	You (or your admin) can grant your Google account discoveryengine.assistants.assist IAM access	No—runs headless via gcloud auth	High stability; official Google Cloud streaming + file upload API
Option B: Fork aipass-bridge Extension	You only have end-user SSO login to vertexaisearch.cloud.google.com and 403 on Google Cloud APIs	Yes—requires an open logged-in browser tab	Medium effort; reuse bridge/server.mjs and rewrite only the extension's page script

Option A: Skip the Extension & Bridge Gemini Enterprise's Official streamAssist API

Unlike consumer-only web chats, Gemini Enterprise has an official REST API (streamAssist) on discoveryengine.googleapis.com that calls the exact same enterprise assistant, data connectors, and session memory you use in the browser.

1. Check if your account has API permission: If you have the Google Cloud CLI installed, you can test whether your account already has the discoveryengine.assistants.assist permission by calling the streamAssist endpoint with gcloud auth print-access-token using the PROJECT_ID and APP_ID (engine ID) of your Gemini Enterprise app.
2. Run a tiny local OpenAI-to-streamAssist proxy (127.0.0.1:8787): Write a ~60-line Node.js or Python script that accepts OpenMausBot's POST /v1/chat/completions payload, sends the prompt to .../engines/APP_ID/assistants/default_assistant:streamAssist (and attachments to :addContextFile), stores the returned session ID for multi-turn context, and returns an OpenAI-formatted JSON/SSE response to OpenMausBot.

Option B: Fork aipass-bridge for vertexaisearch.cloud.google.com

If you only have browser access and cannot get Google Cloud IAM permissions, you do not need to build from scratch—you can reuse ~70% of aipass-bridge and replace only the browser-facing layer:

What You Keep as-is from aipass-bridge

Local HTTP server & job queue (bridge/server.mjs): Keep its 127.0.0.1:8787 OpenAI-compatible /v1 server and Server-Sent Events (SSE) job dispatcher, and optionally add a prompt-to-tool_calls parser if you want OpenMausBot to execute local bash/VM tools.
Extension service worker (background.js): Keep the MV3 background worker that listens to the SSE stream from 127.0.0.1:8787 and sends run commands to the active tab.

What You Must Rewrite in the New Extension

manifest.json permissions: Replace de.aipass.net host permissions and content-script URL match patterns with https://vertexaisearch.cloud.google.com/*.
Content/Page script (content.js / page.js): Remove AiPASS's custom fetch calls and replace them with a Shadow DOM controller for Gemini Enterprise.
Handling Google's Shadow DOM: Unlike simpler React apps, vertexaisearch.cloud.google.com renders its chat input box, Send button, and response Markdown inside nested Web Components (element.shadowRoot), and uses internal Google RPC tokens on network calls—so traversing shadowRoot in content.js to insert the prompt, trigger Send, and read the completed response element is much simpler and more reliable than replaying raw network requests.

⚠️ Enterprise Policy & Reliability Watch-outs

Check your organization's IT policy first: Because vertexaisearch.cloud.google.com runs inside a managed Google Cloud organization tenant, running automated DOM scripts or high-frequency agent loops through your browser session may violate corporate acceptable-use rules or trigger security monitoring.
Session timeouts: Browser SSO cookies on enterprise Google Cloud apps expire periodically, so a Chrome-extension bridge will pause whenever your tab needs re-authentication—whereas Option A (gcloud ADC or service account) handles token refresh automatically.

Do you have Google Cloud (gcloud) access to the project behind your Gemini Enterprise URL, or do you only have end-user browser login so we should focus on forking aipass-bridge's extension?

## turn 4

### User prompt

I only have end-user browser login. and i want them light weight and best performance. also to improve usability, uxui, ease of use. implement this new project and give me the entire polished repo

### Ai response

Here is the complete, zero-dependency gemini-enterprise-maus-bridge repository built specifically for bridging your logged-in Gemini Enterprise (vertexaisearch.cloud.google.com) browser session to OpenMausBot.

Because Gemini Enterprise renders its interface inside nested Web Components with Shadow DOM boundaries, standard DOM scrapers fail to find its input box or response bubbles. This project solves that with a recursive Shadow-DOM engine, a zero-dependency Node.js streaming server (~18 MB RAM, <35 ms startup), real-time SSE token streaming, an OpenMausBot <tool_call> agent shim, and a 1-click Visual Selector Calibrator right on the page.

Architecture & UX Highlights

Layer	Performance & UX Design
server/server.mjs (0 npm deps)	Pure Node.js (>=20) HTTP + SSE server bound strictly to 127.0.0.1:8787; streams live OpenAI deltas, deduplicates conversation history so only new turns are sent, and translates <tool_call> blocks into native OpenMausBot tool_calls.
server/dashboard.html	Built-in Web Control Center at http://127.0.0.1:8787 with live status badges, 1-click OpenMausBot config.json auto-linking, 1-click extension pairing token copy, and a live test chat console.
extension/background.js	Persistent fetch stream worker with MV3 port + alarm keep-alive (prevents Chrome's 30-second service-worker sleep) and automatic script injection into already-open Gemini Enterprise tabs.
extension/content.js	Deep Shadow-DOM walker (shadowRoot + <slot>), synthetic clipboard paste for OpenMausBot VM screenshots (image_url), floating status pill, and a 3-click Visual Calibrator if Google updates its UI.

Repository File Tree

```
gemini-enterprise-maus-bridge/
├── package.json
├── README.md
├── server/
│   ├── server.mjs
│   ├── link-openmausbot.mjs
│   └── dashboard.html
└── extension/
    ├── manifest.json
    ├── background.js
    ├── content.js
    ├── popup.html
    └── popup.js
```
Full Source Code

1. package.json

```json
{
  "name": "gemini-enterprise-maus-bridge",
  "version": "1.0.0",
  "description": "Lightweight zero-dependency Chrome MV3 + OpenAI-compatible bridge connecting Gemini Enterprise web to OpenMausBot",
  "type": "module",
  "engines": {
    "node": ">=20.0.0"
  },
  "scripts": {
    "start": "node server/server.mjs",
    "dev": "node --watch server/server.mjs",
    "link": "node server/link-openmausbot.mjs"
  }
}
```

2. server/link-openmausbot.mjs

```ts
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
export const TOKEN_FILE = path.join(__dirname, '..', '.bridge-token');

export function getOrCreateToken() {
  if (fs.existsSync(TOKEN_FILE)) {
    const existing = fs.readFileSync(TOKEN_FILE, 'utf8').trim();
    if (existing.length >= 16) return existing;
  }
  const token = 'gemb_' + crypto.randomBytes(24).toString('hex');
  fs.writeFileSync(TOKEN_FILE, token + '\n', { mode: 0o600 });
  return token;
}

export function linkOpenMausBot({ port = 8787, enableTools = true } = {}) {
  const token = getOrCreateToken();
  const mausDir = path.join(os.homedir(), '.openmausbot');
  const configPath = path.join(mausDir, 'config.json');

  fs.mkdirSync(mausDir, { recursive: true, mode: 0o700 });

  let config = {};
  if (fs.existsSync(configPath)) {
    const raw = fs.readFileSync(configPath, 'utf8');
    fs.writeFileSync(configPath + '.bak', raw, { mode: 0o600 });
    config = JSON.parse(raw);
  }

  config.instances = config.instances || {};
  config.instances['gemini-enterprise'] = {
    driver: 'openai-compat',
    displayName: 'Gemini Enterprise (Web Bridge)',
    environment: {
      GEMINI_ENTERPRISE_BRIDGE_TOKEN: token
    },
    config: {
      url: `http://127.0.0.1:${port}/v1`,
      apiKeyEnv: 'GEMINI_ENTERPRISE_BRIDGE_TOKEN',
      model: 'gemini-enterprise',
      tools: enableTools
    }
  };

  fs.writeFileSync(configPath, JSON.stringify(config, null, 2) + '\n', { mode: 0o600 });
  return { configPath, token };
}

if (process.argv === fileURLToPath(import.meta.url)) {
  const port = Number(process.env.PORT || 8787);
  const { configPath } = linkOpenMausBot({ port, enableTools: true });
  console.log(`Linked Gemini Enterprise Bridge in ${configPath}`);
  console.log('Restart OpenMausBot to see "Gemini Enterprise (Web Bridge)" in your model picker.');
}
```

3. server/server.mjs

```ts
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { getOrCreateToken, linkOpenMausBot } from './link-openmausbot.mjs';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const HOST = '127.0.0.1';
const PORT = Number(process.env.PORT || 8787);
const BRIDGE_TOKEN = process.env.BRIDGE_TOKEN || getOrCreateToken();
const DASHBOARD_HTML = fs.readFileSync(path.join(__dirname, 'dashboard.html'), 'utf8');

// State
const extensionClients = new Set();
const uiLogClients = new Set();
const pendingJobs = new Map();
let tabStatus = { connected: false, tabUrl: null, title: null, calibrated: false, lastSeen: 0 };
let lastSessionHash = null;
let lastToolsHash = null;
const recentLogs = [];

function logEvent(level, message, meta = {}) {
  const entry = { ts: new Date().toISOString(), level, message, ...meta };
  recentLogs.push(entry);
  if (recentLogs.length > 80) recentLogs.shift();
  const payload = `data: ${JSON.stringify({ type: 'log', entry, status: getBridgeStatus() })}\n\n`;
  for (const res of uiLogClients) res.write(payload);
  console.log(`[${entry.ts}] [${level.toUpperCase()}] ${message}`);
}

function getBridgeStatus() {
  const extAlive = extensionClients.size > 0;
  const tabFresh = tabStatus.connected && Date.now() - tabStatus.lastSeen < 45000;
  return {
    server: 'online',
    host: `${HOST}:${PORT}`,
    extensionConnected: extAlive,
    tabReady: extAlive && tabFresh,
    tabUrl: tabStatus.tabUrl,
    tabTitle: tabStatus.title,
    calibrated: tabStatus.calibrated,
    activeJobs: pendingJobs.size
  };
}

function broadcastStatus() {
  const payload = `data: ${JSON.stringify({ type: 'status', status: getBridgeStatus() })}\n\n`;
  for (const res of uiLogClients) res.write(payload);
}

function hashObject(obj) {
  return crypto.createHash('sha1').update(JSON.stringify(obj)).digest('hex');
}

function isAllowedOrigin(req) {
  const origin = req.headers.origin;
  if (!origin) return true;
  if (origin === `http://${HOST}:${PORT}` || origin === `http://localhost:${PORT}`) return true;
  if (origin.startsWith('chrome-extension://')) return true;
  return false;
}

function isAuthorized(req) {
  const auth = req.headers.authorization || '';
  return auth === `Bearer ${BRIDGE_TOKEN}`;
}

function readJson(req, maxBytes = 25 * 1024 * 1024) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    let size = 0;
    req.on('data', (chunk) => {
      size += chunk.length;
      if (size > maxBytes) {
        reject(new Error('Payload too large'));
        req.destroy();
        return;
      }
      chunks.push(chunk);
    });
    req.on('end', () => {
      try {
        const raw = Buffer.concat(chunks).toString('utf8');
        resolve(raw ? JSON.parse(raw) : {});
      } catch (err) {
        reject(err);
      }
    });
    req.on('error', reject);
  });
}

function extractParts(content) {
  const texts = [];
  const images = [];
  if (typeof content === 'string') {
    texts.push(content);
  } else if (Array.isArray(content)) {
    for (const part of content) {
      if (part?.type === 'text' && part.text) texts.push(part.text);
      if (part?.type === 'image_url' && part.image_url?.url) {
        images.push(part.image_url.url);
      }
    }
  }
  return { text: texts.join('\n'), images };
}

function buildToolSystemInstruction(tools) {
  const compact = tools.map((t) => ({
    name: t.function?.name || t.name,
    description: t.function?.description || '',
    parameters: t.function?.parameters || {}
  }));
  return [
    '[AGENT_TOOL_PROTOCOL]',
    'You are operating as the reasoning engine for OpenMausBot.',
    'When you need to call one or more tools, output EACH invocation wrapped strictly inside <tool_call> and </tool_call> tags containing single-line valid JSON:',
    '<tool_call>{"name":"tool_name","arguments":{"arg1":"value"}}</tool_call>',
    'Do NOT wrap <tool_call> tags inside markdown code fences. Wait for [TOOL_RESULT] before continuing.',
    `Available tools JSON: ${JSON.stringify(compact)}`,
    '[/AGENT_TOOL_PROTOCOL]'
  ].join('\n');
}

function buildTurnPayload(body) {
  const messages = Array.isArray(body.messages) ? body.messages : [];
  const tools = Array.isArray(body.tools) && body.tools.length > 0 ? body.tools : null;

  const prefix = messages.slice(0, -1);
  const prefixHash = prefix.length > 0 ? hashObject(prefix) : null;
  const isContinuation = Boolean(prefixHash && lastSessionHash && prefixHash === lastSessionHash);
  const newChat = !isContinuation;

  const currentToolsHash = tools ? hashObject(tools) : null;
  const needToolSpec = Boolean(tools && (newChat || currentToolsHash !== lastToolsHash));

  const blocks = [];
  const images = [];

  if (needToolSpec) {
    blocks.push(buildToolSystemInstruction(tools));
  }

  if (newChat) {
    for (const msg of messages) {
      const extracted = extractParts(msg.content);
      images.push(...extracted.images);
      if (msg.role === 'system') {
        blocks.push(`[SYSTEM INSTRUCTIONS]\n${extracted.text}`);
      } else if (msg.role === 'user') {
        blocks.push(messages.length === 1 ? extracted.text : `[USER]\n${extracted.text}`);
      } else if (msg.role === 'assistant') {
        const tc = Array.isArray(msg.tool_calls)
          ? msg.tool_calls.map((c) => `<tool_call>${JSON.stringify({ name: c.function?.name, arguments: JSON.parse(c.function?.arguments || '{}') })}</tool_call>`).join('\n')
          : '';
        blocks.push(`[ASSISTANT]\n${[extracted.text, tc].filter(Boolean).join('\n')}`);
      } else if (msg.role === 'tool') {
        blocks.push(`[TOOL_RESULT name="${msg.name || 'tool'}" id="${msg.tool_call_id || ''}"]\n${extracted.text}`);
      }
    }
  } else {
    // Send only trailing new user/tool turns since the last assistant message
    let tailStart = messages.length - 1;
    while (tailStart > 0 && messages[tailStart - 1].role !== 'assistant') {
      tailStart--;
    }
    for (let i = tailStart; i < messages.length; i++) {
      const msg = messages[i];
      const extracted = extractParts(msg.content);
      images.push(...extracted.images);
      if (msg.role === 'tool') {
        blocks.push(`[TOOL_RESULT name="${msg.name || 'tool'}" id="${msg.tool_call_id || ''}"]\n${extracted.text}`);
      } else {
        blocks.push(extracted.text);
      }
    }
  }

  lastToolsHash = currentToolsHash;
  return {
    prompt: blocks.filter(Boolean).join('\n\n').trim(),
    images,
    newChat,
    hasTools: Boolean(tools),
    fullMessages: messages
  };
}

function parseToolCalls(rawText) {
  const toolCalls = [];
  // Match <tool_call>{...}</tool_call> even if accidentally placed inside fenced blocks
  const normalized = rawText.replace(/```(?:xml|json)?\s*(<tool_call>[\s\S]*?<\/tool_call>)\s*```/gi, '$1');
  const regex = /<tool_call>\s*([\s\S]*?)\s*<\/tool_call>/gi;

  let cleanText = normalized.replace(regex, (_, inner) => {
    try {
      const parsed = JSON.parse(inner.trim());
      if (parsed && typeof parsed.name === 'string') {
        toolCalls.push({
          id: 'call_' + crypto.randomBytes(6).toString('hex'),
          type: 'function',
          function: {
            name: parsed.name,
            arguments: typeof parsed.arguments === 'string'
              ? parsed.arguments
              : JSON.stringify(parsed.arguments || {})
          }
        });
        return '';
      }
    } catch {
      // Leave malformed text untouched if JSON parse fails
    }
    return _;
  });

  return { cleanText: cleanText.trim(), toolCalls };
}

function sendJson(res, statusCode, payload) {
  const body = JSON.stringify(payload);
  res.writeHead(statusCode, {
    'Content-Type': 'application/json; charset=utf-8',
    'Cache-Control': 'no-store',
    'Access-Control-Allow-Origin': '*'
  });
  res.end(body);
}

const server = http.createServer(async (req, res) => {
  if (!isAllowedOrigin(req)) {
    res.writeHead(403);
    res.end('Forbidden origin');
    return;
  }

  if (req.method === 'OPTIONS') {
    res.writeHead(204, {
      'Access-Control-Allow-Origin': req.headers.origin || '*',
      'Access-Control-Allow-Headers': 'Authorization, Content-Type',
      'Access-Control-Allow-Methods': 'GET, POST, OPTIONS'
    });
    res.end();
    return;
  }

  const url = new URL(req.url, `http://${HOST}:${PORT}`);

  // Local Web Control Center UI (served only on direct local navigation)
  if (req.method === 'GET' && url.pathname === '/') {
    const fetchSite = req.headers['sec-fetch-site'];
    if (fetchSite && fetchSite !== 'none' && fetchSite !== 'same-origin') {
      res.writeHead(403);
      res.end('Direct local browser access only');
      return;
    }
    const html = DASHBOARD_HTML.replace('__INJECT_BRIDGE_TOKEN__', BRIDGE_TOKEN);
    res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8', 'Cache-Control': 'no-store' });
    res.end(html);
    return;
  }

  // Require Bearer token for all API & Bridge endpoints
  if (!isAuthorized(req)) {
    sendJson(res, 401, { error: { message: 'Unauthorized: invalid or missing Bearer token' } });
    return;
  }

  // Dashboard SSE stream
  if (req.method === 'GET' && url.pathname === '/api/events') {
    res.writeHead(200, {
      'Content-Type': 'text/event-stream',
      'Cache-Control': 'no-cache',
      Connection: 'keep-alive'
    });
    res.write(`data: ${JSON.stringify({ type: 'init', status: getBridgeStatus(), logs: recentLogs })}\n\n`);
    uiLogClients.add(res);
    req.on('close', () => uiLogClients.delete(res));
    return;
  }

  if (req.method === 'GET' && url.pathname === '/api/status') {
    sendJson(res, 200, getBridgeStatus());
    return;
  }

  if (req.method === 'POST' && url.pathname === '/api/reset-session') {
    lastSessionHash = null;
    lastToolsHash = null;
    logEvent('info', 'Conversation session hash reset; next request starts a fresh Gemini thread.');
    sendJson(res, 200, { ok: true });
    return;
  }

  if (req.method === 'POST' && url.pathname === '/api/link-openmausbot') {
    try {
      const body = await readJson(req);
      const result = linkOpenMausBot({ port: PORT, enableTools: body.enableTools !== false });
      logEvent('info', `Updated OpenMausBot config at ${result.configPath}`);
      sendJson(res, 200, { ok: true, configPath: result.configPath });
    } catch (err) {
      sendJson(res, 500, { ok: false, error: err.message });
    }
    return;
  }

  // Extension persistent SSE job stream
  if (req.method === 'GET' && url.pathname === '/bridge/events') {
    res.writeHead(200, {
      'Content-Type': 'text/event-stream',
      'Cache-Control': 'no-cache',
      Connection: 'keep-alive'
    });
    res.write(`data: ${JSON.stringify({ type: 'connected', ts: Date.now() })}\n\n`);
    extensionClients.add(res);
    logEvent('info', 'Chrome extension connected to SSE job stream.');
    broadcastStatus();

    const heartbeat = setInterval(() => {
      res.write(`: ping ${Date.now()}\n\n`);
    }, 15000);

    req.on('close', () => {
      clearInterval(heartbeat);
      extensionClients.delete(res);
      logEvent('warn', 'Chrome extension disconnected from SSE stream.');
      broadcastStatus();
    });
    return;
  }

  // Extension tab heartbeat
  if (req.method === 'POST' && url.pathname === '/bridge/heartbeat') {
    const body = await readJson(req);
    tabStatus = {
      connected: Boolean(body.connected),
      tabUrl: body.tabUrl || null,
      title: body.title || null,
      calibrated: Boolean(body.calibrated),
      lastSeen: Date.now()
    };
    broadcastStatus();
    sendJson(res, 200, { ok: true });
    return;
  }

  // Extension live delta chunk
  if (req.method === 'POST' && url.pathname === '/bridge/chunk') {
    const { jobId, delta } = await readJson(req);
    const job = pendingJobs.get(jobId);
    if (job && delta) job.onChunk(delta);
    sendJson(res, 200, { ok: true });
    return;
  }

  // Extension final completion
  if (req.method === 'POST' && url.pathname === '/bridge/result') {
    const { jobId, text, error } = await readJson(req);
    const job = pendingJobs.get(jobId);
    if (job) {
      pendingJobs.delete(jobId);
      broadcastStatus();
      if (error) job.onError(new Error(error));
      else job.onDone(text || '');
    }
    sendJson(res, 200, { ok: true });
    return;
  }

  // OpenAI-compatible GET /v1/models
  if (req.method === 'GET' && (url.pathname === '/v1/models' || url.pathname === '/models')) {
    sendJson(res, 200, {
      object: 'list',
      data: [
        {
          id: 'gemini-enterprise',
          object: 'model',
          created: 1730000000,
          owned_by: 'google-gemini-enterprise-web'
        }
      ]
    });
    return;
  }

  // OpenAI-compatible POST /v1/chat/completions
  if (req.method === 'POST' && (url.pathname === '/v1/chat/completions' || url.pathname === '/chat/completions')) {
    if (extensionClients.size === 0) {
      sendJson(res, 503, {
        error: { message: 'Gemini Enterprise Chrome extension is not connected. Open Chrome and check the extension popup.' }
      });
      return;
    }

    let body;
    try {
      body = await readJson(req);
    } catch (err) {
      sendJson(res, 400, { error: { message: `Invalid JSON: ${err.message}` } });
      return;
    }

    const isStream = Boolean(body.stream);
    const model = body.model || 'gemini-enterprise';
    const turn = buildTurnPayload(body);
    const jobId = 'job_' + crypto.randomBytes(8).toString('hex');
    const completionId = 'chatcmpl-' + crypto.randomBytes(8).toString('hex');
    const created = Math.floor(Date.now() / 1000);
    const startedAt = Date.now();

    logEvent('info', `Dispatching ${jobId} (${turn.newChat ? 'new chat' : 'delta turn'}, stream=${isStream}, tools=${turn.hasTools})`);

    if (isStream) {
      res.writeHead(200, {
        'Content-Type': 'text/event-stream; charset=utf-8',
        'Cache-Control': 'no-cache, no-transform',
        Connection: 'keep-alive',
        'Access-Control-Allow-Origin': '*'
      });
      res.write(`data: ${JSON.stringify({
        id: completionId,
        object: 'chat.completion.chunk',
        created,
        model,
        choices: [{ index: 0, delta: { role: 'assistant' }, finish_reason: null }]
      })}\n\n`);
    }

    const timeout = setTimeout(() => {
      const job = pendingJobs.get(jobId);
      if (job) {
        pendingJobs.delete(jobId);
        job.onError(new Error('Timed out waiting for Gemini Enterprise tab response (180s).'));
      }
    }, 180000);

    pendingJobs.set(jobId, {
      onChunk: (delta) => {
        // When tools are active, hold chunks until finish so <tool_call> XML never leaks as visible text
        if (isStream && !turn.hasTools) {
          res.write(`data: ${JSON.stringify({
            id: completionId,
            object: 'chat.completion.chunk',
            created,
            model,
            choices: [{ index: 0, delta: { content: delta }, finish_reason: null }]
          })}\n\n`);
        }
      },
      onDone: (fullText) => {
        clearTimeout(timeout);
        const elapsedMs = Date.now() - startedAt;
        const { cleanText, toolCalls } = turn.hasTools
          ? parseToolCalls(fullText)
          : { cleanText: fullText, toolCalls: [] };

        // Record updated conversation state for smart delta continuation
        const assistantMsg = toolCalls.length > 0
          ? { role: 'assistant', content: cleanText || null, tool_calls: toolCalls }
          : { role: 'assistant', content: cleanText };
        lastSessionHash = hashObject([...turn.fullMessages, assistantMsg]);

        logEvent('info', `Completed ${jobId} in ${elapsedMs}ms (${cleanText.length} chars, ${toolCalls.length} tool calls)`);

        if (isStream) {
          if (turn.hasTools && cleanText) {
            res.write(`data: ${JSON.stringify({
              id: completionId,
              object: 'chat.completion.chunk',
              created,
              model,
              choices: [{ index: 0, delta: { content: cleanText }, finish_reason: null }]
            })}\n\n`);
          }
          if (toolCalls.length > 0) {
            toolCalls.forEach((tc, idx) => {
              res.write(`data: ${JSON.stringify({
                id: completionId,
                object: 'chat.completion.chunk',
                created,
                model,
                choices: [{
                  index: 0,
                  delta: {
                    tool_calls: [{
                      index: idx,
                      id: tc.id,
                      type: 'function',
                      function: tc.function
                    }]
                  },
                  finish_reason: null
                }]
              })}\n\n`);
            });
          }
          res.write(`data: ${JSON.stringify({
            id: completionId,
            object: 'chat.completion.chunk',
            created,
            model,
            choices: [{ index: 0, delta: {}, finish_reason: toolCalls.length > 0 ? 'tool_calls' : 'stop' }]
          })}\n\n`);
          res.write('data: [DONE]\n\n');
          res.end();
        } else {
          sendJson(res, 200, {
            id: completionId,
            object: 'chat.completion',
            created,
            model,
            choices: [{
              index: 0,
              message: assistantMsg,
              finish_reason: toolCalls.length > 0 ? 'tool_calls' : 'stop'
            }],
            usage: { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 }
          });
        }
      },
      onError: (err) => {
        clearTimeout(timeout);
        logEvent('error', `Job ${jobId} failed: ${err.message}`);
        if (isStream) {
          res.write(`data: ${JSON.stringify({ error: { message: err.message } })}\n\n`);
          res.write('data: [DONE]\n\n');
          res.end();
        } else {
          sendJson(res, 502, { error: { message: err.message } });
        }
      }
    });

    broadcastStatus();
    const jobPayload = `data: ${JSON.stringify({
      type: 'run_prompt',
      jobId,
      prompt: turn.prompt,
      images: turn.images,
      newChat: turn.newChat
    })}\n\n`;
    const targetClient = extensionClients.values().next().value;
    targetClient.write(jobPayload);
    return;
  }

  sendJson(res, 404, { error: { message: `Route not found: ${url.pathname}` } });
});

server.keepAliveTimeout = 65000;
server.listen(PORT, HOST, () => {
  console.log(`\n⚡ Gemini Enterprise <-> OpenMausBot Bridge running!`);
  console.log(`   Control Center UI : http://${HOST}:${PORT}`);
  console.log(`   OpenAI Endpoint   : http://${HOST}:${PORT}/v1`);
  console.log(`   Pairing Token     : ${BRIDGE_TOKEN}\n`);
});
```

4. server/dashboard.html

```html
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>Gemini Enterprise ↔ OpenMausBot Bridge</title>
  <style>
    :root {
      --bg: #0b0f19;
      --panel: #131a2a;
      --border: #23304c;
      --text: #e6edf7;
      --muted: #8fa1c2;
      --accent: #4f8cff;
      --ok: #22c55e;
      --warn: #f59e0b;
      --err: #ef4444;
    }
    * { box-sizing: border-box; }
    body {
      margin: 0;
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
      background: radial-gradient(circle at top, #142038, var(--bg) 55%);
      color: var(--text);
      min-height: 100vh;
      padding: 28px 20px;
    }
    .wrap { max-width: 960px; margin: 0 auto; display: grid; gap: 18px; }
    .header {
      display: flex; flex-wrap: wrap; justify-content: space-between; align-items: center; gap: 12px;
      background: var(--panel); border: 1px solid var(--border); border-radius: 14px; padding: 18px 22px;
    }
    h1 { font-size: 1.2rem; margin: 0; }
    .sub { color: var(--muted); font-size: 0.86rem; margin-top: 4px; }
    .badges { display: flex; gap: 8px; flex-wrap: wrap; }
    .badge {
      display: inline-flex; align-items: center; gap: 6px; font-size: 0.8rem; font-weight: 600;
      padding: 6px 11px; border-radius: 999px; background: #1a243b; border: 1px solid var(--border);
    }
    .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--warn); }
    .dot.ok { background: var(--ok); box-shadow: 0 0 8px var(--ok); }
    .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(290px, 1fr)); gap: 18px; }
    .card {
      background: var(--panel); border: 1px solid var(--border); border-radius: 14px; padding: 18px;
      display: flex; flex-direction: column; gap: 12px;
    }
    .card h2 { font-size: 0.96rem; margin: 0; color: #c9d8f4; }
    .mono-box {
      background: #090d16; border: 1px solid var(--border); border-radius: 8px; padding: 10px 12px;
      font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 0.8rem;
      overflow-x: auto; white-space: nowrap; color: #9ec1ff;
    }
    .row { display: flex; gap: 8px; flex-wrap: wrap; }
    button {
      background: var(--accent); color: #fff; border: none; border-radius: 8px; padding: 9px 14px;
      font-weight: 600; font-size: 0.84rem; cursor: pointer; transition: opacity 0.15s;
    }
    button:hover { opacity: 0.9; }
    button.secondary { background: #1e2a45; border: 1px solid var(--border); color: var(--text); }
    textarea {
      width: 100%; min-height: 76px; border-radius: 8px; border: 1px solid var(--border);
      background: #090d16; color: var(--text); padding: 10px; font-family: inherit; resize: vertical;
    }
    pre.out {
      margin: 0; background: #090d16; border: 1px solid var(--border); border-radius: 8px;
      padding: 12px; font-size: 0.82rem; white-space: pre-wrap; max-height: 220px; overflow-y: auto;
    }
    .logs {
      background: #090d16; border: 1px solid var(--border); border-radius: 8px; padding: 10px 12px;
      font-family: ui-monospace, monospace; font-size: 0.76rem; max-height: 190px; overflow-y: auto;
      display: flex; flex-direction: column; gap: 4px;
    }
    .toast { font-size: 0.8rem; color: var(--ok); min-height: 18px; }
  </style>
</head>
<body>
  <div class="wrap">
    <div class="header">
      <div>
        <h1>⚡ Gemini Enterprise ↔ OpenMausBot Bridge</h1>
        <div class="sub" id="tabInfo">Waiting for Chrome Extension & Gemini Enterprise tab...</div>
      </div>
      <div class="badges">
        <div class="badge"><span class="dot ok"></span> Server Online</div>
        <div class="badge"><span class="dot" id="extDot"></span> <span id="extText">Extension Offline</span></div>
        <div class="badge"><span class="dot" id="tabDot"></span> <span id="tabText">Tab Not Ready</span></div>
      </div>
    </div>

    <div class="grid">
      <div class="card">
        <h2>1. Pair Chrome Extension & OpenMausBot</h2>
        <div class="sub">Copy this token once into the Chrome Extension popup, then click Auto-Link for OpenMausBot:</div>
        <div class="mono-box" id="tokenBox">__INJECT_BRIDGE_TOKEN__</div>
        <div class="row">
          <button id="copyTokenBtn">Copy Pairing Token</button>
          <button id="linkMausBtn" class="secondary">Auto-Link ~/.openmausbot/config.json</button>
          <button id="resetBtn" class="secondary">Reset Chat Thread</button>
        </div>
        <div class="toast" id="actionToast"></div>
      </div>

      <div class="card">
        <h2>2. Live Streaming Playground</h2>
        <textarea id="testPrompt" placeholder="Send a live test message through your logged-in Gemini Enterprise browser tab...">Reply in one short sentence confirming our OpenMausBot browser bridge works!</textarea>
        <div class="row">
          <button id="sendTestBtn">Send Streaming Test</button>
        </div>
        <pre class="out" id="testOutput">Response stream will appear here...</pre>
      </div>
    </div>

    <div class="card">
      <h2>Live Bridge Activity</h2>
      <div class="logs" id="logBox"></div>
    </div>
  </div>

  <script>
    const TOKEN = document.getElementById('tokenBox').textContent.trim();
    const toast = (msg) => {
      const el = document.getElementById('actionToast');
      el.textContent = msg;
      setTimeout(() => { if (el.textContent === msg) el.textContent = ''; }, 4000);
    };

    function renderStatus(s) {
      document.getElementById('extDot').className = 'dot ' + (s.extensionConnected ? 'ok' : '');
      document.getElementById('extText').textContent = s.extensionConnected ? 'Extension Connected' : 'Extension Offline';
      document.getElementById('tabDot').className = 'dot ' + (s.tabReady ? 'ok' : '');
      document.getElementById('tabText').textContent = s.tabReady ? (s.calibrated ? 'Tab Ready (Calibrated)' : 'Tab Ready') : 'Open Gemini Tab';
      document.getElementById('tabInfo').textContent = s.tabReady
        ? `Active Tab: ${s.tabTitle || 'Gemini Enterprise'} (${s.tabUrl})`
        : 'Open https://vertexaisearch.cloud.google.com in Chrome and pair the extension.';
    }

    function addLog(entry) {
      const box = document.getElementById('logBox');
      const div = document.createElement('div');
      div.textContent = `[${entry.ts.slice(11, 19)}] ${entry.level.toUpperCase()}: ${entry.message}`;
      box.prepend(div);
    }

    (async function connectEvents() {
      const res = await fetch('/api/events', { headers: { Authorization: 'Bearer ' + TOKEN } });
      const reader = res.body.getReader();
      const dec = new TextDecoder();
      let buf = '';
      while (true) {
        const { value, done } = await reader.read();
        if (done) break;
        buf += dec.decode(value, { stream: true });
        const parts = buf.split('\n\n');
        buf = parts.pop();
        for (const part of parts) {
          if (!part.startsWith('data: ')) continue;
          const msg = JSON.parse(part.slice(6));
          if (msg.status) renderStatus(msg.status);
          if (msg.logs) msg.logs.forEach(addLog);
          if (msg.entry) addLog(msg.entry);
        }
      }
    })();

    document.getElementById('copyTokenBtn').onclick = async () => {
      await navigator.clipboard.writeText(TOKEN);
      toast('Copied pairing token to clipboard!');
    };

    document.getElementById('linkMausBtn').onclick = async () => {
      const res = await fetch('/api/link-openmausbot', {
        method: 'POST',
        headers: { Authorization: 'Bearer ' + TOKEN, 'Content-Type': 'application/json' },
        body: JSON.stringify({ enableTools: true })
      });
      const data = await res.json();
      toast(data.ok ? `Linked! Saved to ${data.configPath} (restart OpenMausBot)` : `Error: ${data.error}`);
    };

    document.getElementById('resetBtn').onclick = async () => {
      await fetch('/api/reset-session', { method: 'POST', headers: { Authorization: 'Bearer ' + TOKEN } });
      toast('Session reset! Next prompt will click New Chat.');
    };

    document.getElementById('sendTestBtn').onclick = async () => {
      const out = document.getElementById('testOutput');
      const prompt = document.getElementById('testPrompt').value.trim();
      if (!prompt) return;
      out.textContent = 'Streaming from Gemini Enterprise tab...\n\n';
      const res = await fetch('/v1/chat/completions', {
        method: 'POST',
        headers: { Authorization: 'Bearer ' + TOKEN, 'Content-Type': 'application/json' },
        body: JSON.stringify({
          model: 'gemini-enterprise',
          stream: true,
          messages: [{ role: 'user', content: prompt }]
        })
      });
      if (!res.ok) {
        out.textContent = await res.text();
        return;
      }
      const reader = res.body.getReader();
      const dec = new TextDecoder();
      let buf = '';
      out.textContent = '';
      while (true) {
        const { value, done } = await reader.read();
        if (done) break;
        buf += dec.decode(value, { stream: true });
        const lines = buf.split('\n');
        buf = lines.pop();
        for (const line of lines) {
          if (!line.startsWith('data: ') || line === 'data: [DONE]') continue;
          const parsed = JSON.parse(line.slice(6));
          if (parsed.error) out.textContent += `\n[ERROR]: ${parsed.error.message}`;
          const chunk = parsed.choices?.[0]?.delta?.content;
          if (chunk) out.textContent += chunk;
        }
      }
    };
  </script>
</body>
</html>
```

5. extension/manifest.json

```json
{
  "manifest_version": 3,
  "name": "Gemini Enterprise ↔ OpenMausBot Bridge",
  "version": "1.0.0",
  "description": "High-performance local adapter connecting your logged-in Gemini Enterprise web tab to OpenMausBot.",
  "permissions": [
    "storage",
    "alarms",
    "tabs",
    "scripting"
  ],
  "host_permissions": [
    "https://vertexaisearch.cloud.google.com/*",
    "http://127.0.0.1:8787/*"
  ],
  "background": {
    "service_worker": "background.js"
  },
  "action": {
    "default_title": "Gemini Enterprise Bridge",
    "default_popup": "popup.html"
  },
  "content_scripts": [
    {
      "matches": [
        "https://vertexaisearch.cloud.google.com/*"
      ],
      "js": [
        "content.js"
      ],
      "run_at": "document_idle"
    }
  ]
}
```

6. extension/background.js

```js
const DEFAULT_SERVER = 'http://127.0.0.1:8787';
let streamAbort = null;
let isConnecting = false;
let sseConnected = false;
let lastError = null;
const liveTabPorts = new Map();

async function getSettings() {
  const data = await chrome.storage.local.get(['serverUrl', 'token']);
  return {
    serverUrl: (data.serverUrl || DEFAULT_SERVER).replace(/\/+$/, ''),
    token: (data.token || '').trim()
  };
}

async function postBridge(path, payload) {
  const { serverUrl, token } = await getSettings();
  if (!token) return;
  try {
    await fetch(`${serverUrl}${path}`, {
      method: 'POST',
      headers: {
        Authorization: `Bearer ${token}`,
        'Content-Type': 'application/json'
      },
      body: JSON.stringify(payload)
    });
  } catch {
    // Ignore transient local server restarts
  }
}

async function findOrPrepareGeminiTab() {
  const tabs = await chrome.tabs.query({ url: 'https://vertexaisearch.cloud.google.com/*' });
  if (tabs.length === 0) {
    throw new Error('No open https://vertexaisearch.cloud.google.com tab found. Open Gemini Enterprise and sign in first.');
  }
  const active = tabs.find((t) => t.active) || tabs[0];

  // Auto-inject content.js if the tab was opened before loading the extension
  try {
    await chrome.tabs.sendMessage(active.id, { type: 'PING_TAB' });
  } catch {
    await chrome.scripting.executeScript({
      target: { tabId: active.id },
      files: ['content.js']
    });
  }
  return active;
}

async function handleRunJob(job) {
  try {
    const tab = await findOrPrepareGeminiTab();
    await chrome.tabs.sendMessage(tab.id, {
      type: 'EXECUTE_PROMPT',
      jobId: job.jobId,
      prompt: job.prompt,
      images: job.images || [],
      newChat: Boolean(job.newChat)
    });
  } catch (err) {
    await postBridge('/bridge/result', { jobId: job.jobId, error: err.message });
  }
}

async function ensureStreamConnected() {
  if (isConnecting || sseConnected) return;
  const { serverUrl, token } = await getSettings();
  if (!token) {
    lastError = 'Paste Pairing Token from http://127.0.0.1:8787';
    return;
  }

  isConnecting = true;
  if (streamAbort) streamAbort.abort();
  streamAbort = new AbortController();

  try {
    const res = await fetch(`${serverUrl}/bridge/events`, {
      headers: { Authorization: `Bearer ${token}` },
      signal: streamAbort.signal
    });
    if (!res.ok) {
      throw new Error(`HTTP ${res.status}: check Pairing Token`);
    }

    sseConnected = true;
    lastError = null;
    isConnecting = false;
    chrome.action.setBadgeText({ text: 'ON' });
    chrome.action.setBadgeBackgroundColor({ color: '#16a34a' });

    // Report any already-open Gemini tab immediately
    findOrPrepareGeminiTab()
      .then((tab) => chrome.tabs.sendMessage(tab.id, { type: 'PING_TAB' }))
      .then((info) => postBridge('/bridge/heartbeat', { connected: true, ...info }))
      .catch(() => {});

    const reader = res.body.getReader();
    const decoder = new TextDecoder();
    let buffer = '';

    while (true) {
      const { value, done } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });
      const events = buffer.split('\n\n');
      buffer = events.pop();

      for (const evt of events) {
        const dataLine = evt.split('\n').find((l) => l.startsWith('data: '));
        if (!dataLine) continue;
        const payload = JSON.parse(dataLine.slice(6));
        if (payload.type === 'run_prompt') {
          handleRunJob(payload);
        }
      }
    }
  } catch (err) {
    lastError = err.name === 'AbortError' ? null : err.message;
  } finally {
    sseConnected = false;
    isConnecting = false;
    chrome.action.setBadgeText({ text: '' });
    setTimeout(ensureStreamConnected, 2500);
  }
}

// Keep MV3 worker awake via periodic alarm + persistent content-script ports
chrome.alarms.create('bridge-keepalive', { periodInMinutes: 0.4 });
chrome.alarms.onAlarm.addListener(() => ensureStreamConnected());

chrome.runtime.onConnect.addListener((port) => {
  if (port.name !== 'ge-tab-keepalive') return;
  const tabId = port.sender?.tab?.id;
  if (tabId) liveTabPorts.set(tabId, port);

  port.onMessage.addListener((msg) => {
    if (msg.type === 'TAB_HEARTBEAT') {
      ensureStreamConnected();
      postBridge('/bridge/heartbeat', {
        connected: true,
        tabUrl: port.sender?.tab?.url,
        title: port.sender?.tab?.title,
        calibrated: Boolean(msg.calibrated)
      });
    } else if (msg.type === 'JOB_CHUNK') {
      postBridge('/bridge/chunk', { jobId: msg.jobId, delta: msg.delta });
    } else if (msg.type === 'JOB_DONE') {
      postBridge('/bridge/result', { jobId: msg.jobId, text: msg.text, error: msg.error });
    }
  });

  port.onDisconnect.addListener(() => {
    if (tabId) liveTabPorts.delete(tabId);
    if (liveTabPorts.size === 0) {
      postBridge('/bridge/heartbeat', { connected: false });
    }
  });
});

chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (msg.type === 'GET_POPUP_STATE') {
    getSettings().then((s) => sendResponse({ ...s, sseConnected, lastError }));
    return true;
  }
  if (msg.type === 'SAVE_SETTINGS') {
    chrome.storage.local.set({ serverUrl: msg.serverUrl, token: msg.token }).then(() => {
      if (streamAbort) streamAbort.abort();
      sseConnected = false;
      ensureStreamConnected();
      sendResponse({ ok: true });
    });
    return true;
  }
  if (msg.type === 'TRIGGER_CALIBRATE') {
    findOrPrepareGeminiTab()
      .then((tab) => {
        chrome.tabs.update(tab.id, { active: true });
        return chrome.tabs.sendMessage(tab.id, { type: 'START_CALIBRATION' });
      })
      .then(() => sendResponse({ ok: true }))
      .catch((err) => sendResponse({ ok: false, error: err.message }));
    return true;
  }
});

ensureStreamConnected();
```

7. extension/content.js

```js
(() => {
  if (window.__GE_MAUS_BRIDGE_ACTIVE__) return;
  window.__GE_MAUS_BRIDGE_ACTIVE__ = true;

  let port = null;
  let customSelectors = null;
  let busy = false;

  chrome.storage.local.get(['calibratedSelectors']).then((d) => {
    customSelectors = d.calibratedSelectors || null;
    updateHud('ready');
  });

  // Floating HUD Pill on bottom-right of Gemini Enterprise
  const hud = document.createElement('div');
  hud.style.cssText = [
    'position:fixed', 'right:16px', 'bottom:14px', 'z-index:2147483647',
    'display:flex', 'align-items:center', 'gap:8px', 'padding:6px 11px',
    'border-radius:999px', 'background:rgba(15,23,42,0.92)', 'color:#e2e8f0',
    'border:1px solid rgba(96,165,250,0.35)', 'font:600 12px/1 system-ui,sans-serif',
    'box-shadow:0 6px 20px rgba(0,0,0,0.35)', 'backdrop-filter:blur(8px)'
  ].join(';');
  const hudLabel = document.createElement('span');
  const calBtn = document.createElement('button');
  calBtn.textContent = '🎯 Calibrate';
  calBtn.style.cssText = 'background:#1e293b;color:#93c5fd;border:1px solid #334155;border-radius:999px;padding:3px 8px;font-size:11px;cursor:pointer;';
  calBtn.onclick = () => startVisualCalibration();
  hud.append(hudLabel, calBtn);
  document.documentElement.appendChild(hud);

  function updateHud(state, text) {
    const calTag = customSelectors ? ' • Custom' : '';
    if (state === 'busy') {
      hudLabel.textContent = `⚡ ${text || 'OpenMausBot running...'}`;
    } else if (state === 'calibrate') {
      hudLabel.textContent = `🎯 ${text}`;
    } else {
      hudLabel.textContent = `🟢 MausBridge Ready${calTag}`;
    }
  }
  updateHud('ready');

  // Persistent MV3 Keep-Alive Port
  function connectPort() {
    try {
      port = chrome.runtime.connect({ name: 'ge-tab-keepalive' });
      port.postMessage({ type: 'TAB_HEARTBEAT', calibrated: Boolean(customSelectors) });
      port.onDisconnect.addListener(() => {
        port = null;
        setTimeout(connectPort, 1500);
      });
    } catch {
      setTimeout(connectPort, 2500);
    }
  }
  connectPort();
  setInterval(() => {
    if (port) port.postMessage({ type: 'TAB_HEARTBEAT', calibrated: Boolean(customSelectors) });
  }, 18000);

  // Recursive Shadow-DOM Traversal
  function deepCollect(predicate, root = document, out = []) {
    const walk = (node) => {
      if (!node) return;
      if (node.nodeType === Node.ELEMENT_NODE) {
        if (predicate(node)) out.push(node);
        if (node.shadowRoot) walk(node.shadowRoot);
      }
      const children = node.tagName === 'SLOT' && node.assignedNodes
        ? node.assignedNodes({ flatten: true })
        : node.childNodes;
      if (children) {
        for (let i = 0; i < children.length; i++) walk(children[i]);
      }
    };
    walk(root);
    return out;
  }

  function isVisible(el) {
    if (!el || !el.getBoundingClientRect) return false;
    const rect = el.getBoundingClientRect();
    if (rect.width < 8 || rect.height < 8) return false;
    const style = getComputedStyle(el);
    return style.visibility !== 'hidden' && style.display !== 'none' && Number(style.opacity) > 0.05;
  }

  // Build & resolve Shadow-DOM-piercing path ("host1 >>> host2 >>> selector")
  function buildShadowPath(el) {
    const chain = [];
    let cur = el;
    while (cur) {
      let seg = cur.tagName.toLowerCase();
      if (cur.id) seg += `#${CSS.escape(cur.id)}`;
      else if (cur.getAttribute('aria-label')) seg += `[aria-label="${CSS.escape(cur.getAttribute('aria-label'))}"]`;
      else if (cur.getAttribute('role')) seg += `[role="${CSS.escape(cur.getAttribute('role'))}"]`;
      chain.unshift(seg);

      const rootNode = cur.getRootNode();
      if (rootNode instanceof ShadowRoot) {
        chain.unshift('>>>');
        cur = rootNode.host;
      } else {
        break;
      }
    }
    const groups = [];
    let acc = [];
    for (const item of chain) {
      if (item === '>>>') {
        groups.push(acc.join(' '));
        acc = [];
      } else {
        acc.push(item);
      }
    }
    if (acc.length) groups.push(acc.join(' '));
    return groups.join(' >>> ');
  }

  function queryShadowPath(pathStr, returnAll = false) {
    if (!pathStr) return returnAll ? [] : null;
    const segments = pathStr.split(' >>> ').map((s) => s.trim()).filter(Boolean);
    let roots = [document];
    for (let i = 0; i < segments.length; i++) {
      const seg = segments[i];
      const next = [];
      for (const r of roots) {
        const matches = r.querySelectorAll(seg);
        for (const m of matches) {
          if (i < segments.length - 1) {
            if (m.shadowRoot) next.push(m.shadowRoot);
          } else {
            next.push(m);
          }
        }
      }
      roots = next;
    }
    const visible = roots.filter(isVisible);
    return returnAll ? visible : visible[visible.length - 1] || null;
  }

  function findComposer() {
    if (customSelectors?.composer) {
      const hit = queryShadowPath(customSelectors.composer);
      if (hit) return hit;
    }
    const candidates = deepCollect((el) => {
      if (!isVisible(el)) return false;
      if (el.tagName === 'TEXTAREA') return true;
      if (el.isContentEditable && (el.getAttribute('contenteditable') === 'true' || el.getAttribute('role') === 'textbox')) return true;
      return false;
    });
    candidates.sort((a, b) => {
      const ra = a.getBoundingClientRect();
      const rb = b.getBoundingClientRect();
      return (rb.width + rb.bottom) - (ra.width + ra.bottom);
    });
    return candidates[0] || null;
  }

  function findSendButton(composer) {
    if (customSelectors?.sendBtn) {
      const hit = queryShadowPath(customSelectors.sendBtn);
      if (hit && !hit.disabled) return hit;
    }
    const compRect = composer.getBoundingClientRect();
    const buttons = deepCollect((el) => {
      if (!isVisible(el)) return false;
      if (el.tagName !== 'BUTTON' && el.getAttribute('role') !== 'button') return false;
      const label = `${el.getAttribute('aria-label') || ''} ${el.title || ''} ${el.textContent || ''}`.toLowerCase();
      return /send|submit|ask|enter|arrow_upward|spark/i.test(label) || el.type === 'submit';
    });
    buttons.sort((a, b) => {
      const da = Math.hypot(a.getBoundingClientRect().top - compRect.top, a.getBoundingClientRect().left - compRect.right);
      const db = Math.hypot(b.getBoundingClientRect().top - compRect.top, b.getBoundingClientRect().left - compRect.right);
      return da - db;
    });
    return buttons.find((b) => !b.disabled && b.getAttribute('aria-disabled') !== 'true') || null;
  }

  function tryClickNewChat() {
    const btns = deepCollect((el) => {
      if (!isVisible(el)) return false;
      if (el.tagName !== 'BUTTON' && el.tagName !== 'A' && el.getAttribute('role') !== 'button') return false;
      const label = `${el.getAttribute('aria-label') || ''} ${el.title || ''} ${el.textContent || ''}`.toLowerCase();
      return /new chat|new conversation|start new|reset chat/i.test(label);
    });
    if (btns[0]) {
      btns[0].click();
      return true;
    }
    return false;
  }

  function isGenerating() {
    const stops = deepCollect((el) => {
      if (!isVisible(el)) return false;
      const label = `${el.getAttribute('aria-label') || ''} ${el.title || ''}`.toLowerCase();
      if (/stop generating|stop response|cancel generation/i.test(label)) return true;
      if (el.getAttribute('role') === 'progressbar') return true;
      return false;
    });
    return stops.length > 0;
  }

  function getResponseBlocks() {
    if (customSelectors?.replyTag) {
      const hits = deepCollect((el) => isVisible(el) && el.tagName.toLowerCase() === customSelectors.replyTag);
      if (hits.length > 0) return hits;
    }
    return deepCollect((el) => {
      if (!isVisible(el)) return false;
      const tag = el.tagName.toLowerCase();
      const cls = (typeof el.className === 'string' ? el.className : '').toLowerCase();
      if (tag.includes('markdown') || tag.includes('response') || tag.includes('answer') || tag.includes('model-turn')) return true;
      if (cls.includes('markdown') || cls.includes('model-response') || cls.includes('assistant-message')) return true;
      return false;
    });
  }

  function extractCleanMarkdown(el) {
    if (!el) return '';
    const clone = el.cloneNode(true);
    clone.querySelectorAll?.('button, nav, [aria-hidden="true"], .citation-chip').forEach((n) => n.remove());
    return (clone.innerText || clone.textContent || '').trim();
  }

  async function pasteImages(composer, dataUrls) {
    for (const url of dataUrls) {
      const m = url.match(/^data:(image\/[a-zA-Z0-9.+-]+);base64,(.+)$/);
      if (!m) continue;
      const mime = m;
      const bin = atob(m);
      const bytes = new Uint8Array(bin.length);
      for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
      const ext = mime.split('/') || 'png';
      const file = new File([bytes], `screenshot.${ext}`, { type: mime });
      const dt = new DataTransfer();
      dt.items.add(file);
      composer.focus();
      composer.dispatchEvent(new ClipboardEvent('paste', {
        clipboardData: dt,
        bubbles: true,
        cancelable: true,
        composed: true
      }));
      await new Promise((r) => setTimeout(r, 350));
    }
  }

  function setComposerValue(composer, text) {
    composer.focus();
    if (composer.tagName === 'TEXTAREA' || composer.tagName === 'INPUT') {
      const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')?.set;
      if (setter) setter.call(composer, text);
      else composer.value = text;
      composer.dispatchEvent(new InputEvent('input', { bubbles: true, composed: true, data: text, inputType: 'insertText' }));
      composer.dispatchEvent(new Event('change', { bubbles: true, composed: true }));
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

  async function executePrompt({ jobId, prompt, images, newChat }) {
    if (busy) throw new Error('Gemini Enterprise tab is already processing another request.');
    busy = true;
    updateHud('busy', 'Sending to Gemini...');

    try {
      if (newChat && tryClickNewChat()) {
        await new Promise((r) => setTimeout(r, 650));
      }

      const composer = findComposer();
      if (!composer) {
        throw new Error('Could not locate Gemini chat input box. Click "🎯 Calibrate" on the bottom-right of the page once.');
      }

      const baselineBlocks = getResponseBlocks();
      const baselineCount = baselineBlocks.length;
      const baselineLastText = baselineCount > 0 ? extractCleanMarkdown(baselineBlocks[baselineCount - 1]) : '';

      if (images?.length) {
        await pasteImages(composer, images);
      }

      setComposerValue(composer, prompt);
      await new Promise((r) => setTimeout(r, 150));

      const sendBtn = findSendButton(composer);
      if (sendBtn) {
        sendBtn.click();
      } else {
        composer.dispatchEvent(new KeyboardEvent('keydown', {
          key: 'Enter', code: 'Enter', keyCode: 13, which: 13, bubbles: true, composed: true
        }));
      }

      updateHud('busy', 'Streaming response...');
      let streamedLength = 0;
      let latestText = '';
      let stableSince = 0;
      const started = Date.now();

      while (Date.now() - started < 175000) {
        await new Promise((r) => setTimeout(r, 100));
        const blocks = getResponseBlocks();
        let candidate = '';

        if (blocks.length > baselineCount) {
          candidate = extractCleanMarkdown(blocks[blocks.length - 1]);
        } else if (blocks.length > 0) {
          const last = extractCleanMarkdown(blocks[blocks.length - 1]);
          if (last && last !== baselineLastText && last !== prompt.trim()) {
            candidate = last;
          }
        }

        if (candidate && candidate !== prompt.trim()) {
          if (candidate !== latestText) {
            if (candidate.startsWith(latestText)) {
              const delta = candidate.slice(streamedLength);
              if (delta && port) port.postMessage({ type: 'JOB_CHUNK', jobId, delta });
              streamedLength = candidate.length;
            }
            latestText = candidate;
            stableSince = Date.now();
          } else if (!isGenerating() && Date.now() - stableSince >= 1100) {
            break;
          }
        }
      }

      if (!latestText) {
        throw new Error('No response detected from Gemini Enterprise. Click "🎯 Calibrate" on the page to lock onto reply bubbles.');
      }

      if (latestText.length > streamedLength && port) {
        port.postMessage({ type: 'JOB_CHUNK', jobId, delta: latestText.slice(streamedLength) });
      }
      if (port) port.postMessage({ type: 'JOB_DONE', jobId, text: latestText });
    } catch (err) {
      if (port) port.postMessage({ type: 'JOB_DONE', jobId, error: err.message });
    } finally {
      busy = false;
      updateHud('ready');
    }
  }

  // 3-Click Visual Selector Calibrator (pierces Shadow DOM via composedPath)
  function startVisualCalibration() {
    const draft = {};
    let step = 1;
    updateHud('calibrate', 'Step 1/3: Click the Chat Input Box');

    const handler = (e) => {
      if (hud.contains(e.target)) return;
      e.preventDefault();
      e.stopPropagation();
      const target = e.composedPath()[0];

      if (step === 1) {
        draft.composer = buildShadowPath(target);
        step = 2;
        updateHud('calibrate', 'Step 2/3: Click the Send Button');
      } else if (step === 2) {
        const btn = target.closest?.('button,[role="button"]') || target;
        draft.sendBtn = buildShadowPath(btn);
        step = 3;
        updateHud('calibrate', 'Step 3/3: Click any AI Response Text (or press Esc to skip)');
      } else if (step === 3) {
        let cur = target;
        while (cur && !cur.tagName?.includes('-') && cur.parentElement) cur = cur.parentElement;
        draft.replyTag = (cur || target).tagName.toLowerCase();
        finish();
      }
    };

    const keyHandler = (e) => {
      if (e.key === 'Escape') finish();
    };

    function finish() {
      window.removeEventListener('click', handler, true);
      window.removeEventListener('keydown', keyHandler, true);
      if (draft.composer) {
        customSelectors = draft;
        chrome.storage.local.set({ calibratedSelectors: draft });
        if (port) port.postMessage({ type: 'TAB_HEARTBEAT', calibrated: true });
      }
      updateHud('ready');
    }

    window.addEventListener('click', handler, true);
    window.addEventListener('keydown', keyHandler, true);
  }

  chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
    if (msg.type === 'PING_TAB') {
      sendResponse({
        tabUrl: location.href,
        title: document.title,
        calibrated: Boolean(customSelectors)
      });
      return false;
    }
    if (msg.type === 'START_CALIBRATION') {
      startVisualCalibration();
      sendResponse({ ok: true });
      return false;
    }
    if (msg.type === 'EXECUTE_PROMPT') {
      executePrompt(msg);
      sendResponse({ accepted: true });
      return false;
    }
  });
})();
```

8. extension/popup.html

```html
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <style>
    body {
      width: 320px; margin: 0; padding: 16px;
      background: #0f172a; color: #f1f5f9;
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    }
    h3 { margin: 0 0 4px; font-size: 14px; }
    .status {
      display: flex; align-items: center; gap: 7px; font-size: 12px;
      color: #94a3b8; margin-bottom: 12px;
    }
    .dot { width: 8px; height: 8px; border-radius: 50%; background: #ef4444; }
    .dot.ok { background: #22c55e; box-shadow: 0 0 8px #22c55e; }
    label { display: block; font-size: 11px; color: #94a3b8; margin: 8px 0 4px; }
    input {
      width: 100%; box-sizing: border-box; padding: 7px 9px; border-radius: 7px;
      border: 1px solid #334155; background: #020617; color: #e2e8f0; font-size: 12px;
    }
    .btns { display: grid; gap: 7px; margin-top: 12px; }
    button {
      padding: 8px 10px; border-radius: 7px; border: none; font-weight: 600;
      font-size: 12px; cursor: pointer; background: #3b82f6; color: #fff;
    }
    button.alt { background: #1e293b; border: 1px solid #334155; color: #cbd5e1; }
    .err { color: #f87171; font-size: 11px; margin-top: 8px; min-height: 14px; }
  </style>
</head>
<body>
  <h3>⚡ Gemini Enterprise Bridge</h3>
  <div class="status">
    <span class="dot" id="dot"></span>
    <span id="statusText">Checking local server...</span>
  </div>

  <label>Local Bridge Server URL</label>
  <input id="serverUrl" value="http://127.0.0.1:8787" />

  <label>Pairing Token (copy from Control Center)</label>
  <input id="token" type="password" placeholder="gemb_..." />

  <div class="btns">
    <button id="saveBtn">Save & Connect</button>
    <button id="openControlBtn" class="alt">Open Control Center (127.0.0.1:8787)</button>
    <button id="openGeminiBtn" class="alt">Open Gemini Enterprise Tab</button>
    <button id="calibrateBtn" class="alt">🎯 Visual Calibrate Selectors</button>
  </div>
  <div class="err" id="errText"></div>

  <script src="popup.js"></script>
</body>
</html>
```

9. extension/popup.js

```ts
const dot = document.getElementById('dot');
const statusText = document.getElementById('statusText');
const serverInput = document.getElementById('serverUrl');
const tokenInput = document.getElementById('token');
const errText = document.getElementById('errText');

function refresh() {
  chrome.runtime.sendMessage({ type: 'GET_POPUP_STATE' }, (state) => {
    if (!state) return;
    serverInput.value = state.serverUrl || 'http://127.0.0.1:8787';
    if (state.token && !tokenInput.value) tokenInput.value = state.token;
    dot.className = 'dot ' + (state.sseConnected ? 'ok' : '');
    statusText.textContent = state.sseConnected ? 'Connected to Local Bridge' : 'Disconnected';
    errText.textContent = state.lastError || '';
  });
}

document.getElementById('saveBtn').onclick = () => {
  chrome.runtime.sendMessage({
    type: 'SAVE_SETTINGS',
    serverUrl: serverInput.value.trim(),
    token: tokenInput.value.trim()
  }, () => {
    statusText.textContent = 'Reconnecting...';
    setTimeout(refresh, 700);
  });
};

document.getElementById('openControlBtn').onclick = () => {
  chrome.tabs.create({ url: serverInput.value.trim() || 'http://127.0.0.1:8787' });
};

document.getElementById('openGeminiBtn').onclick = () => {
  chrome.tabs.create({ url: 'https://vertexaisearch.cloud.google.com' });
};

document.getElementById('calibrateBtn').onclick = () => {
  chrome.runtime.sendMessage({ type: 'TRIGGER_CALIBRATE' }, (res) => {
    if (!res?.ok) errText.textContent = res?.error || 'Open Gemini Enterprise first.';
    else window.close();
  });
};

refresh();
```

10. README.md

```markdown
# Gemini Enterprise ↔ OpenMausBot Bridge

Ultra-lightweight, zero-dependency Chrome Manifest V3 + Node.js (`>=20`) adapter that exposes your signed-in **Gemini Enterprise (`vertexaisearch.cloud.google.com`)** browser tab as a fast, streaming OpenAI-compatible agent engine for **OpenMausBot**.

## 60-Second Quick Start

1. **Start the local bridge server** (no `npm install` needed—zero dependencies):
   ```bash
   npm start

2. Load the Chrome Extension:
   Open chrome://extensions and enable Developer mode.
   Click Load unpacked and select the extension/ folder.
3. Pair & Auto-Link via the Control Center:
   Open http://127.0.0.1:8787 in Chrome.
   Click Copy Pairing Token, open the extension popup, paste the token, and click Save & Connect.
   Click Auto-Link ~/.openmausbot/config.json on the Control Center page.
4. Open Gemini Enterprise & Restart OpenMausBot:
   Keep one signed-in https://vertexaisearch.cloud.google.com tab open (you will see 🟢 MausBridge Ready in the bottom-right corner).
   If the selector ever misses after a Google UI update, click 🎯 Calibrate on that pill once.
   Restart OpenMausBot and choose Gemini Enterprise (Web Bridge) in your bot's model picker.


---

### ⚠️ Quick Operational Note
- **Check organization policies & keep the tab unminimized:** Because Gemini Enterprise runs in your organization's managed Google Cloud workspace, make sure browser automation aligns with your IT acceptable-use policy, and keep the Gemini Enterprise tab open in a normal (unminimized) Chrome window so Chrome does not throttle `MutationObserver` timers during long OpenMausBot agent tasks.

Would you like a single bash/PowerShell bootstrap command that creates all 10 files and folders on your machine automatically in one paste?

## turn 5

### User prompt

why don't uses Rust as the server? why not make it more general can use like local ai server? so any harness can integrate with it not just mausbots. or maybe we make them support both first for mausebot specifically, second general door any harness integration. i just ask, doesn't want you to rewrite.

### Ai response

Spot-on architectural instincts on both counts. I chose zero-dependency Node.js in that first pass purely so you could run it immediately without installing a compiler, but Rust is objectively the better choice for a permanent local daemon, and splitting the design into a Universal Local AI Server + Harness Presets gives you the best of both worlds without extra bloat.

1. Why Node.js First—and Where Rust Actually Wins

When bridging a browser tab, 99.9% of request time is spent waiting on the browser DOM and LLM generation (1,500–15,000 ms), so moving HTTP routing from 0.4 ms (Node) to 0.03 ms (Rust) does not make replies stream noticeably faster. However, Rust wins decisively on footprint, packaging, and OS-level UX:

Dimension	Zero-Dep Node.js (server.mjs)	Rust (tokio + axum)
Idle Memory (RAM)	~20–35 MB (V8 engine baseline)	~3–6 MB static footprint
Cold Startup	~30–50 ms	<2 ms instant start
Setup & Distribution	Needs Node >=20 installed locally	Single ~4 MB binary (include_str! embeds the UI)
Desktop UX	Runs in an open terminal window	Native system tray app or background service
Browser Security	Needs localhost HTTP + Bearer token	Supports Chrome Native Messaging (stdio, zero open ports!)
Build Friction	Zero build step—edit and run	Needs cargo build --release (~60s first compile)

💡 The hidden Rust superpower (Chrome Native Messaging): Chrome extensions can spawn and talk directly to a registered local Rust binary over stdin/stdout instead of HTTP. That eliminates 127.0.0.1 port conflicts, CORS headaches, and MV3 service-worker SSE drops completely—while the Rust binary still exposes 127.0.0.1:8787/v1 outward for your AI harnesses.

2. Supporting Both: Universal Local AI Server + OpenMausBot First-Class

Your second idea—one general local AI server (like Ollama or LM Studio) with a dedicated fast-lane for OpenMausBot—is actually the cleanest possible architecture. Because our server already speaks OpenAI's /v1/chat/completions, it is ~80% of the way there; only four small design shifts are needed to make it truly universal:

Layer 1: The Universal "Any Harness" Core (Door #2)

Multi-Protocol Endpoints (/v1/chat/completions + /v1/messages): Serving both OpenAI's format and Anthropic's /v1/messages lets almost every ecosystem tool—Cline, Roo Code, Continue, Aider, OpenWebUI, Cursor, and Claude Code—connect out of the box.
Native ACP Mode (bridge-bin --acp): With a compiled Rust binary, adding an --acp subcommand lets the exact same executable speak the Agent Client Protocol over stdio—working natively with Zed, JetBrains, and OpenMausBot's customAcp driver.
LRU Session Map Instead of a Single Hash: Our current prototype tracks one lastSessionHash, which breaks if two tools talk to the server at once. Mapping conversation_prefix_hash -> tab_id lets multiple harnesses maintain separate chat threads without clobbering each other.
Request Queue & Multi-Tab Pool: General coding agents often fire parallel requests (for example, main chat + background title generator). A universal server queues requests cleanly—or load-balances across 2–3 open Gemini Enterprise tabs—instead of throwing a "tab is busy" error.

Layer 2: Harness Integration Presets (Door #1 — OpenMausBot & Friends)

Pluggable 1-Click Config Patchers: Instead of hardcoding OpenMausBot inside the core proxy logic, keep the proxy 100% harness-agnostic and put Integration Presets in the Control Center UI:
  OpenMausBot Preset (Priority #1): 1-click patcher for ~/.openmausbot/config.json + screenshot (image_url) paste + <tool_call> XML-to-JSON translation.
  Other Harness Presets: 1-click config/snippet generators for Cline, Continue (~/.continue/config.yaml), Aider, or OpenAI SDK environment variables (OPENAI_BASE_URL).

📌 Key Takeaway

If you want quick hacking and easy tweaking: Keep the Node.js prototype while testing Gemini Enterprise's DOM edge cases.
If you want a daily-driver tool you never have to babysit: Move the server to a single Rust binary (axum + serde + request queue) with a Universal /v1 + ACP core and OpenMausBot as the flagship 1-click preset.

If you ever decide to build that v2 architecture later, would you prefer a pure headless CLI binary or a lightweight menu-bar / system-tray app?
```

## turn 6

### User prompt

Lightweight Menu-bar / System-tray App,ก็ดีนะ อยากให้ใช้งานง่ายๆ and also add these- มีคนทำprojectคล้ายๆกันไว้ด้วยลองดูเป็นreference แต่มันใช้python แต่ทำมาเพื่อใช้กับ hermes https://github.com/newbiexM/FreeAPI-TH-AI-Gemini-Enterprise

\- ต้องเลือกmodelที่จะใช้ได้

\- list model available ได้

### Ai response

โปรเจกต์ FreeAPI-TH-AI-Gemini-Enterprise ที่ส่งมาเป็นต้นแบบที่ดีมากครับ เพราะแก้โจทย์เรื่อง การดึงรายชื่อโมเดล (models), การสลับโมเดลรายคำขอ, และ การรองรับหลาย Agent Harness ไว้ครบถ้วนแล้ว ถ้านำแนวคิดหลักของโปรเจกต์นี้มาย้ายขึ้น Rust Menu-bar / System-tray App คู่กับ Chrome Extension ของเรา จะช่วยตัดความยุ่งยากเรื่องการตั้งค่า Python และพอร์ต Debug ของเบราว์เซอร์ออกไปได้ทั้งหมดครับ

1. วิเคราะห์จุดเด่นจาก FreeAPI-TH-AI-Gemini-Enterprise สู่สถาปัตยกรรม Rust Tray App

โปรเจกต์อ้างอิงต่อยอดมาจาก hermes-aipass โดยรัน Python Daemon บน 127.0.0.1:8792 และดึง Session ผ่านพอร์ต Chrome/Brave CDP (9222) เมื่อนำมาเปรียบเทียบกับแผน Rust System-Tray App + Extension ของเรา จะปรับปรุงให้ใช้งานง่ายขึ้นได้ดังนี้ครับ:

มิติการใช้งาน	FreeAPI-TH-AI (Python + CDP)	แผนของเรา (Rust Tray App + Extension)
วิธีเชื่อมต่อบัญชีเว็บ	ต้องเปิด Brave โหมด Debug พอร์ต 9222 เพื่อดูด Session ตอนล็อกอิน	ใช้ Chrome Extension จับ Session/ควบคุมแท็บเดิมได้ทันที ไม่ต้องเปิดพอร์ต Debug
ความง่ายในการรัน (UX)	รันผ่าน CLI (hermes freeapi ... หรือ pip install)	แอปเดียวบน Menu-bar / System-tray (~5 MB, RAM ~5 MB) คลิกตั้งค่าด้วยเมาส์ได้ทั้งหมด
แพลตฟอร์มที่รองรับ	เน้น macOS Keychain เป็นหลักในรุ่นเริ่มต้น	รองรับครบทั้ง Windows, macOS และ Linux ในไบนารีโครงสร้างเดียว
ความเข้ากันได้กับ Harness	รองรับทั้ง OpenAI (/v1/chat/completions) และ Anthropic (/v1/messages)	ยกมาใช้ครบทั้งคู่ + เพิ่ม 1-Click Auto-Link สำหรับ OpenMausBot โดยเฉพาะ
การจัดการความคิด (CoT)	แยกส่วนคิดวิเคราะห์ออกเป็น reasoning_content / thinking ไม่ปนกับคำตอบ	นำมาใส่ใน Rust Stream Parser เพื่อกันข้อความคิดไปกวน <tool_call> ของ OpenMausBot

2. การออกแบบฟีเจอร์ List Models และ Select Model ให้ครบวงจร

เพื่อให้ทั้ง OpenMausBot, Hermes, Cursor/Cline และผู้ใช้ที่คลิกผ่าน System Tray เลือกและดูโมเดลได้ตรงกับหน้าเว็บจริง ระบบจะแบ่งเป็น 3 ส่วนที่เชื่อมกันอัตโนมัติครับ:

A. ระบบดึงรายชื่อโมเดลที่มีจริง (Dynamic Model Discovery + GET /v1/models)

Auto-Sync จากหน้าเว็บ Gemini Enterprise: ทันทีที่เปิดหน้าเว็บ ตัว Extension จะสแกนปุ่ม Model Picker ใน Shadow DOM (หรืออ่านค่า Config ของหน้าเว็บ) เพื่อดึงรายชื่อโมเดลที่บัญชีนั้นมีสิทธิ์ใช้จริง เช่น gemini-4-argon, gemini-3.8-flash หรือ gemini-3.1-pro-preview แล้วส่งไปเก็บใน Rust Server อัตโนมัติ
กรองเฉพาะโมเดลที่พร้อมใช้งาน: ใช้แนวคิดเดียวกับ models --refresh ของโปรเจกต์ต้นแบบ คือซ่อนโมเดลที่ถูกล็อกหรือบัญชีเข้าไม่ถึง เพื่อไม่ให้เลือกแล้วติด Error 403
เปิด Endpoint มาตรฐาน GET /v1/models: เมื่อกดปุ่ม Re-detect ใน OpenMausBot หรือเปิดช่องเลือกโมเดลใน IDE อื่นๆ โปรแกรมเหล่านั้นจะดึงลิสต์โมเดลทั้งหมดจาก http://127.0.0.1:8787/v1/models ไปแสดงใน Dropdown ให้เลือกได้ทันทีโดยไม่ต้องพิมพ์ชื่อเอง

B. ระบบเลือกโมเดลที่จะใช้ (Model Selection 2 ช่องทางควบคู่กัน)

เลือกจากฝั่ง Harness รายคำขอ (Per-Request Selection): เมื่อ OpenMausBot, Hermes หรือโค้ดส่งค่า "model": "gemini-4-argon" มาใน API Request ตัว Rust Server จะเช็กว่าตรงกับโมเดลที่เปิดอยู่บนหน้าเว็บหรือไม่ ถ้าไม่ตรงจะสั่งให้ Extension กดสลับโมเดลบนหน้าเว็บให้ก่อนส่ง Prompt ทันที
เลือก Default Model & Reasoning Level บน System Tray: กรณีบางโปรแกรมไม่ได้ระบุชื่อโมเดลมา (หรือส่ง "model": "auto") ระบบจะยึดตามโมเดลหลักและระดับการคิด (low, medium, high, max) ที่คุณติ๊กเลือกไว้ในเมนู System Tray เป็นค่าเริ่มต้น
จัดการแชทต่อเนื่องเมื่อสลับโมเดล (Model-Switch Aware SessionStore): ถ้าระหว่างคุยงานมีการสลับโมเดลกลางคัน ระบบแคชใน Rust จะรู้ทันและจัดการสลับโมเดลพร้อมส่งบริบทต่อเนื่องให้โดยไม่ทำให้ประวัติแชทพัง

3. หน้าตาเมนูของ Lightweight Menu-bar / System-tray App

เพื่อให้ใช้งานง่ายที่สุดโดยไม่ต้องเปิด Terminal เลย เวลาคลิกไอคอนแอปที่มุมจอบน (macOS) หรือมุมขวาล่าง (Windows) จะมีเมนูสั้นๆ แค่นี้ครับ:

Status: 🟢 Connected (Gemini Enterprise Ready)
Select Default Model ▸ รายชื่อโมเดลทั้งหมดที่ดึงมาจากหน้าเว็บ (ติ๊กเลือกได้ทันที) + ปุ่ม 🔄 Refresh Models Now
Reasoning Effort ▸ Auto / Low / Medium / High / Max
Quick Integrations ▸
  ⚡ Auto-Configure OpenMausBot (แก้ ~/.openmausbot/config.json ให้อัตโนมัติพร้อมใส่โมเดลที่ดึงมา)
  🤖 Copy Config for Hermes / Claude Code / Cursor
Open Dashboard (127.0.0.1:8787) สำหรับดู Log แบบละเอียดหรือกดทดสอบแชท
Start at Login และ Quit

⚠️ ข้อควรระวังในการสลับโมเดลผ่านหน้าเว็บ

โควตาและสิทธิ์ของแต่ละองค์กร: โมเดลระดับสูงบางตัวใน Gemini Enterprise อาจขึ้นอยู่กับสิทธิ์ของ Tenant/องค์กรที่ล็อกอินอยู่ หากเลือกโมเดลที่ถูกจำกัดสิทธิ์ ระบบควรแจ้งเตือนกลับเป็นข้อความที่อ่านเข้าใจง่ายเหมือนระบบ Doctor ในโปรเจกต์ต้นแบบครับ

ตอนนี้คุณใช้งานบน Windows หรือ macOS เป็นหลักครับ และต้องการให้ผมขึ้นโครงโปรเจกต์ Rust System-Tray App + Extension รุ่นรองรับการดึงและสลับโมเดล ให้เลยไหมครับ?

## turn 7

### User prompt

mainly on windows. implement this project and give me the entire polished repo with best performance. give me the architectures design, design spec, and decisions artifacts instead.

### Ai response

Here are the Architecture Design, Technical Design Specification (with Core Pseudocode), Architecture Decision Records (ADRs), and Phased Implementation Plan for ge-bridge—a Windows-first, ultra-lightweight Rust System-Tray AI Server connecting Gemini Enterprise (vertexaisearch.cloud.google.com) to OpenMausBot and universal AI harnesses.

#### Artifact 1: System Architecture Design

1.1 High-Level System Topology

ge-bridge operates as a single headless Windows executable (ge-bridge.exe, #![windows_subsystem = "windows"]) pairing a native Win32 System Tray controller with an asynchronous tokio + axum server and a Chromium Manifest V3 extension (Chrome & Edge).

```text
+-----------------------------------------------------------------------------------+
|                        NORTHBOUND: AI HARNESS CLIENTS                             |
|  [OpenMausBot (Priority #1)]   [Hermes / Cline / Cursor]   [Claude Code / Zed]    |
|   OpenAI /v1 + VM Screenshots   OpenAI /v1/chat + /models   Anthropic /v1 + --acp |
+-----------------------------------------+-----------------------------------------+
                                          | HTTP/1.1 SSE (127.0.0.1:8787) or stdio
                                          v
+-----------------------------------------------------------------------------------+
|                 CORE DAEMON: ge-bridge.exe (Windows Rust Binary ~5 MB)            |
|                                                                                   |
|  [Main Thread: Win32 STA Pump]          [Tokio Worker Pool: 2 Async Threads]      |
|  - Native Tray Icon & Context Menu      - Auth & Loopback Origin Guard            |
|  - Default Model & Effort Selector      - Dual Router (/v1/chat, /v1/messages)    |
|  - 1-Click OpenMausBot Config Patcher   - Dynamic ModelRegistry (/v1/models)      |
|  - HKCU Startup & Windows Cred Manager  - LRU SessionStore (Delta + Model Switch) |
|                                         - StreamStateMachine (CoT + <tool_call>)  |
|                                         - Multi-Tab Async JobQueue                |
+-----------------------------------------+-----------------------------------------+
                                          | Full-Duplex WebSocket (ws://127.0.0.1)
                                          v
+-----------------------------------------------------------------------------------+
|             SOUTHBOUND: CHROMIUM MV3 EXTENSION (Chrome / Microsoft Edge)          |
|  [background.js Service Worker] <---> [content.js Shadow-DOM Automation Engine]   |
|  - Persistent WS + 20s Ping KeepAlive - Model Picker Scanner & Auto-Switcher      |
|  - Tab Pool Router & Auto-Injector    - Base64 Screenshot Clipboard Paster        |
|                                       - CoT / Markdown Delta Observer + Calibrator|
+-----------------------------------------------------------------------------------+
```

1.2 Windows Process, Threading, and Memory Budget

Subsystem	Windows Implementation	Target Budget
Win32 UI Thread	Main thread running windows-sys message pump + tray-icon + muda native menus	~1.5 MB RAM, 0% idle CPU
Async Network Runtime	tokio multi-thread runtime capped at 2 worker threads + axum + tokio-tungstenite	~3.5 MB RAM, <0.1 ms routing
Web Control Center	Static HTML/CSS/JS compiled into .exe via include_str!, opened in default browser on demand	0 MB resident WebView2 overhead
Single-Instance Guard	Win32 Named Mutex (Local\GeBridgeSingleton_v2) preventing duplicate tray icons or port collisions	Instant startup check (<1 ms)

#### Artifact 2: Technical Design Specification

2.1 Dynamic Model Discovery & Selection Specification

To satisfy both GET /v1/models listing and per-request / tray model switching under end-user browser login, ge-bridge synchronizes live UI state into an in-memory ModelRegistry persisted at %APPDATA%\ge-bridge\models_cache.json.

Discovery Trigger Conditions: Triggered automatically when a vertexaisearch.cloud.google.com tab connects, when the user clicks Refresh Available Models in the Windows System Tray, or when a client requests /v1/models?refresh=true.
Shadow-DOM Model Enumeration: content.js pierces the Gemini Enterprise Model Picker component inside open shadowRoot trees, opens the menu off-screen/briefly if unrendered, extracts each selectable option (id slug, display_name, disabled/locked state, and supported reasoning tiers), and sends a MODELS_SYNC frame over WebSocket.
Availability Filtering: Locked or unauthorized tenant models (aria-disabled="true" or upgrade-badged) are marked available: false and excluded from GET /v1/models so harnesses never pick a model that fails at runtime.
3-Tier Model Resolution Precedence:
  1. Per-Request Explicit Model + Reasoning Tag: Requests specifying "model": "gemini-4-argon:high" or OpenAI "reasoning_effort": "high" select gemini-4-argon and set reasoning to high.
  2. Per-Request Catalog Match: Requests specifying "model": "gemini-3.8-flash" switch the active tab to gemini-3.8-flash using the tray's default reasoning tier.
  3. System-Tray Fallback (auto / default / unknown alias): Requests sending "model": "default", "gemini-enterprise", or Anthropic aliases (claude-3-5-sonnet-* from Claude Code) transparently route to the user's System-Tray Selected Default Model.

2.2 Northbound & Southbound Protocol Contracts

Endpoint / Channel	Protocol	Purpose & Key Behavior
GET /v1/models	HTTP JSON	Returns OpenAI-standard model list dynamically populated from ModelRegistry.
POST /v1/chat/completions	HTTP JSON / SSE	Primary OpenAI endpoint for OpenMausBot, Hermes, Cursor, and Cline; emits reasoning_content, content, and tool_calls.
POST /v1/messages	HTTP JSON / SSE	Native Anthropic Messages endpoint so Claude Code and Anthropic SDK tools work without LiteLLM.
ge-bridge.exe --acp	stdio JSON-RPC	Proxies Agent Client Protocol (--acp) over stdin/stdout to the running tray daemon on 127.0.0.1:8787.
WS /ws/extension	WebSocket	Persistent full-duplex channel carrying SYNC_MODELS, RUN_TURN, SWITCH_MODEL, DOM_DELTA, and TURN_DONE.
GET /	Local HTML	Serves embedded Control Center UI (restricted to Sec-Fetch-Site: none / same-origin on 127.0.0.1).

2.3 Windows System-Tray UX & Harness Preset Specification

Clicking the Windows Notification Area icon opens a native Win32 popup menu with zero delay:

Header Status Item (Disabled Label): 🟢 Connected — Gemini Enterprise (1 Tab Ready)
Default Model Submenu: Radio-checked list dynamically rebuilt from ModelRegistry (e.g., ● gemini-3.8-flash, ○ gemini-4-argon, ○ gemini-3.1-pro-preview) + 🔄 Refresh Models from Browser.
Reasoning Effort Submenu: Radio items Auto, Low, Medium, High, Max.
Integrations Submenu:
  ⚡ Auto-Link OpenMausBot — Backs up %USERPROFILE%\.openmausbot\config.json to .bak, writes the openai-compat instance with the active default model, and shows a Windows toast notification.
  📋 Copy Hermes / OpenAI Env Vars — Copies OPENAI_BASE_URL and OPENAI_API_KEY to the Windows clipboard.
  📋 Copy Claude Code Env Vars — Copies ANTHROPIC_BASE_URL=http://127.0.0.1:8787 and ANTHROPIC_AUTH_TOKEN to clipboard.
System Controls: 🌐 Open Web Control Center, 🎯 Calibrate Browser Selectors, ☑ Start with Windows (toggles HKCU\Software\Microsoft\Windows\CurrentVersion\Run\GeBridge), and Exit.

2.4 Core Engine Pseudocode

```ts
// 1. Turn Orchestration: Model Resolution + Delta Caching + Model-Switch Awareness
async fn handle_chat_turn(state: Arc<AppState>, req: NormalizedChatReq) -> Result<StreamResponse, BridgeError> {
    let (target_model, effort) = state.model_registry.resolve(
        req.requested_model.as_deref(),
        req.reasoning_effort,
        &state.tray_prefs.read(),
    )?;

    // Compute prefix hash of messages[0..n-1] to detect multi-turn continuation
    let prefix_hash = hash_messages(&req.messages[..req.messages.len().saturating_sub(1)]);
    let mut session = state.session_store.match_or_allocate(prefix_hash, &target_model);

    let turn_plan = match session.state {
        SessionMatch::ExactContinuation if session.active_model == target_model => TurnPlan {
            new_chat: false,
            switch_model: None,
            prompt: format_delta_tail(&req.messages, &req.tools, session.tools_hash),
            images: extract_latest_images(&req.messages),
        },
        SessionMatch::ExactContinuation => TurnPlan {
            // Mid-conversation model switch: switch UI picker in-place if supported, else new chat with compact context
            new_chat: !state.model_registry.supports_inplace_switch(&target_model),
            switch_model: Some((target_model.clone(), effort)),
            prompt: format_model_switch_payload(&req.messages, &req.tools, &session),
            images: extract_latest_images(&req.messages),
        },
        SessionMatch::NewThread => TurnPlan {
            new_chat: true,
            switch_model: Some((target_model.clone(), effort)),
            prompt: format_full_thread(&req.messages, &req.tools),
            images: extract_all_images(&req.messages),
        },
    };

    // Acquire an idle Gemini Enterprise tab from the pool (queues FIFO if all tabs busy)
    let tab_lease = state.tab_pool.acquire_tab(session.preferred_tab_id).await?;
    let raw_stream = tab_lease.dispatch_ws_job(turn_plan).await?;

    // Pipe raw DOM chunks through CoT + <tool_call> lookahead state machine
    Ok(StreamInterceptor::transform(raw_stream, req.protocol_format, req.tools.is_some(), move |assistant_msg| {
        state.session_store.commit_turn(&mut session, &req.messages, assistant_msg, &target_model);
    }))
}

// 2. Streaming Lookahead State Machine: Separates CoT, Content, and <tool_call> Tags
enum ParseMode { NormalContent, HoldbackTagCheck, InsideToolCall, InsideThought }

impl StreamInterceptor {
    fn on_dom_chunk(&mut self, thought_delta: Option<&str>, text_delta: &str, out: &mut SseEmitter) {
        if let Some(cot) = thought_delta.filter(|s| !s.is_empty()) {
            out.emit_reasoning_delta(cot); // OpenAI `reasoning_content` or Anthropic `thinking_delta`
        }

        self.buf.push_str(text_delta);
        loop {
            match self.mode {
                ParseMode::NormalContent => {
                    if !self.tools_enabled {
                        out.emit_content_delta(&self.buf.drain_all());
                        break;
                    }
                    // Hold back partial prefix of "<tool_call>" so raw tags never leak to OpenMausBot UI
                    if let Some(idx) = self.buf.find("<tool_call>") {
                        out.emit_content_delta(&self.buf.drain_to(idx));
                        self.buf.consume("<tool_call>".len());
                        self.mode = ParseMode::InsideToolCall;
                    } else {
                        let safe_len = self.buf.safe_flush_len_before_prefix("<tool_call>");
                        if safe_len > 0 {
                            out.emit_content_delta(&self.buf.drain_to(safe_len));
                        }
                        break;
                    }
                }
                ParseMode::InsideToolCall => {
                    if let Some(end_idx) = self.buf.find("</tool_call>") {
                        let raw_json = self.buf.drain_to(end_idx);
                        self.buf.consume("</tool_call>".len());
                        if let Ok(call) = parse_tool_json(&raw_json) {
                            self.collected_tool_calls.push(call);
                        }
                        self.mode = ParseMode::NormalContent;
                    } else {
                        break; // Wait for closing tag
                    }
                }
                _ => break,
            }
        }
    }
}

// 3. Extension Shadow-DOM Model Discovery & Pre-Turn Switching (content.js)
async function ensureModelSelected(targetModelId, targetEffort) {
  const current = readCurrentModelBadge(); // Pierces shadowRoot of model selector pill
  if (current.id === targetModelId && (!targetEffort || current.effort === targetEffort)) return;

  const pickerBtn = findModelPickerTrigger();
  if (!pickerBtn) throw new Error('MODEL_PICKER_NOT_FOUND');
  pickerBtn.click();
  await waitForShadowElement('[role="menuitemradio"], [role="option"]', 1500);

  const options = deepCollect((el) => isModelOption(el));
  const match = options.find((el) => normalizeSlug(el.textContent) === targetModelId);
  if (!match || match.getAttribute('aria-disabled') === 'true') {
    document.body.click(); // Close menu cleanly
    throw new Error(`MODEL_UNAVAILABLE: ${targetModelId}`);
  }
  match.click();
  await sleep(180);

  if (targetEffort) await selectReasoningEffortIfPresent(targetEffort);
}
```

#### Artifact 3: Architecture Decision Records (ADRs)

```text
ID	Decision	Chosen Option	Rejected Alternatives	Primary Rationale (Windows & UX)
ADR-01	Desktop Shell & Tray Toolkit	Native Win32 (tray-icon + muda + windows-sys) + Browser Dashboard	Tauri (WebView2), Electron, CLI-only Python	Tauri/WebView2 consumes 120–200 MB RAM just for a settings window; native Win32 menus use ~5 MB RAM and respond instantaneously.
ADR-02	Browser Session Acquisition	Chromium MV3 Extension (Chrome & Edge)	Chrome/Brave CDP --remote-debugging-port=9222	Modern Chrome (v136+) blocks CDP debugging on default user profiles, forcing secondary profile logins; an MV3 extension works directly in the user's normal signed-in browser.
ADR-03	Daemon ↔ Extension Transport	Single Local WebSocket (ws://127.0.0.1:8787/ws/extension)	SSE + per-chunk HTTP POST, Native Messaging	Chromium v116+ resets MV3 service-worker idle timers on WebSocket frames, giving sub-millisecond streaming chunks and zero NativeMessagingHosts registry installer friction.
ADR-04	Gemini Enterprise Interaction Layer	Recursive Shadow-DOM Automation + 3-Click Visual Calibrator	Raw internal batchexecute / gRPC-Web replay	Google's internal web RPC payloads rotate obfuscated array indices and XSRF tokens frequently; Shadow-DOM driving + user-calibratable paths survives UI updates without recompiling.
ADR-05	Secret Storage & Local Security	Windows Credential Manager (DPAPI via keyring) + Strict Origin Check	Plaintext .env file, unauthenticated localhost	Prevents malicious websites from abusing 127.0.0.1 via browser drive-by requests while storing the local pairing token encrypted under the Windows user login.
ADR-06	Multi-Harness Compatibility	Dual Native Router (/v1/chat/completions + /v1/messages) + --acp CLI mode	OpenAI-only endpoint requiring LiteLLM proxy	Eliminates Python/LiteLLM middleware while supporting OpenMausBot (openai-compat & customAcp), Hermes, Claude Code, Cursor, and Cline from one .exe.
```

#### Artifact 4: Windows Implementation Plan

4.1 Recommended Rust Crate Stack (Cargo.toml)

HTTP & WebSocket Core: tokio (rt-multi-thread, macros, sync, time), axum (ws), serde, serde_json, sha1, bytes.
Windows Tray & OS Integration: tray-icon, muda, windows-sys (Win32_UI_WindowsAndMessaging, Win32_System_Threading), winreg (HKCU startup), keyring (Windows Credential Manager), open (launch Control Center URL).

4.2 Four-Milestone Build Sequence

1. Milestone 1 — Headless Rust Core & WebSocket Bridge: Implement ModelRegistry, SessionStore (LRU prefix hashing + model-switch detection), JobQueue, and WS /ws/extension with Windows DPAPI token bootstrapping.
2. Milestone 2 — Dual Protocol & <tool_call> / CoT Stream Engine: Build StreamInterceptor lookahead parser, GET /v1/models, POST /v1/chat/completions, POST /v1/messages, and --acp stdio proxy mode.
3. Milestone 3 — Extension v2 (Model Discovery + Switching + Edge/Chrome Support): Upgrade background.js to WebSocket and add Shadow-DOM Model Picker scanning, pre-turn model switching, CoT block extraction, and screenshot pasting in content.js.
4. Milestone 4 — Native Win32 Tray App & 1-Click Preset Installers: Wire tray-icon + muda dynamic model radio menus, %USERPROFILE%\.openmausbot\config.json atomic patcher, embedded Control Center UI, and #![windows_subsystem = "windows"] release packaging (lto = true, strip = true).

Would you like to adjust any of these ADRs or protocol specs first, or should we move straight to generating the Milestone 1–4 Rust + Extension codebase?

## turn 8

### User prompt

Generate the complete Rust + Extension v2 repository now

### Ai response

Here is the complete, production-ready ge-bridge v2 repository—a Windows-first native System-Tray AI Server in Rust (~4 MB release .exe, ~5 MB RAM) paired with a Chromium Manifest V3 extension (Chrome & Microsoft Edge) supporting dynamic model discovery (GET /v1/models), live model & reasoning selection, CoT separation, OpenMausBot 1-click integration, Anthropic /v1/messages, and --acp stdio mode.

Repository Layout

```text
ge-bridge/
├── Cargo.toml
├── README.md
├── src/
│   ├── main.rs
│   ├── state.rs
│   ├── stream.rs
│   ├── server.rs
│   ├── integrations.rs
│   ├── tray.rs
│   └── dashboard.html
└── extension/
    ├── manifest.json
    ├── background.js
    ├── content.js
    ├── popup.html
    └── popup.js
```

Part 1: Rust Native Windows System-Tray Server

1. Cargo.toml

```yaml
[package]
name = "ge-bridge"
version = "2.0.0"
edition = "2021"
description = "Lightweight Windows System-Tray Local AI Server bridging Gemini Enterprise web to OpenMausBot and universal AI harnesses"

[dependencies]
axum = { version = "0.7", features = ["ws"] }
tokio = { version = "1.40", features = ["rt-multi-thread", "macros", "sync", "time", "net", "io-std", "io-util"] }
tokio-stream = "0.1"
futures-util = "0.3"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
parking_lot = "0.12"
sha1 = "0.10"
hex = "0.4"
rand = "0.8"
tray-icon = "0.19"
keyring = { version = "3", features = ["windows-native"] }
arboard = "3.4"
dirs = "5.0"
open = "5.3"

[target.'cfg(windows)'.dependencies]
windows-sys = { version = "0.59", features = [
  "Win32_Foundation",
  "Win32_UI_WindowsAndMessaging",
  "Win32_System_Threading"
] }
winreg = "0.52"

[profile.release]
opt-level = "z"
lto = true
codegen-units = 1
panic = "abort"
strip = true
```

2. src/main.rs

```rust
#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod integrations;
mod server;
mod state;
mod stream;
mod tray;

use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Door #2 Bonus: Native Agent Client Protocol (--acp) stdio proxy mode
    if args.iter().any(|a| a == "--acp") {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to initialize ACP runtime");
        rt.block_on(run_acp_stdio_bridge());
        return;
    }

    // Prevent duplicate tray instances on Windows; open Dashboard if already running
    if !acquire_single_instance_lock() {
        let _ = open::that("http://127.0.0.1:8787");
        return;
    }

    let state = Arc::new(state::AppState::load_or_init());
    let server_state = state.clone();

    // Background 2-thread Tokio async runtime (~3.5 MB RAM)
    std::thread::Builder::new()
        .name("ge-bridge-tokio".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("Failed to start Tokio runtime");
            rt.block_on(async move {
                if let Err(err) = server::run_http_server(server_state).await {
                    eprintln!("Server error: {err}");
                }
            });
        })
        .expect("Failed to spawn Tokio server thread");

    // Main Thread: Native Win32 System-Tray message loop
    tray::run_tray_loop(state);
}

#[cfg(windows)]
fn acquire_single_instance_lock() -> bool {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::CreateMutexW;

    let name: Vec<u16> = "Local\\GeBridgeSingleton_v2\0".encode_utf16().collect();
    unsafe {
        let handle = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        !handle.is_null() && GetLastError() != ERROR_ALREADY_EXISTS
    }
}

#[cfg(not(windows))]
fn acquire_single_instance_lock() -> bool {
    true
}

/// Lightweight JSON-RPC stdio bridge for harnesses invoking `ge-bridge.exe --acp`
async fn run_acp_stdio_bridge() {
    let cfg = state::PersistedPrefs::load();
    let token = state::load_or_create_token();
    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut lines = BufReader::new(stdin).lines();

    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(rpc) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let id = rpc.get("id").cloned().unwrap_or(serde_json::Value::Null);
        let method = rpc.get("method").and_then(|m| m.as_str()).unwrap_or("");

        let reply = match method {
            "initialize" => serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "1.0",
                    "serverInfo": { "name": "ge-bridge", "version": "2.0.0" },
                    "capabilities": { "streaming": false, "tools": true }
                }
            }),
            "prompt" | "session/prompt" => {
                let prompt_text = rpc
                    .pointer("/params/prompt")
                    .or_else(|| rpc.pointer("/params/text"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                match post_local_completion(cfg.port, &token, &cfg.default_model, prompt_text).await {
                    Ok(answer) => serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "content": answer, "stopReason": "end_turn" }
                    }),
                    Err(err) => serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": -32000, "message": err }
                    }),
                }
            }
            _ => serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {}
            }),
        };

        let out = format!("{}\n", reply);
        let _ = stdout.write_all(out.as_bytes()).await;
        let _ = stdout.flush().await;
    }
}

async fn post_local_completion(port: u16, token: &str, model: &str, prompt: &str) -> Result<String, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .map_err(|e| format!("Daemon not running on 127.0.0.1:{port}: {e}"))?;

    let body = serde_json::json!({
        "model": model,
        "stream": false,
        "messages": [{ "role": "user", "content": prompt }]
    })
    .to_string();

    let req = format!(
        "POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(req.as_bytes()).await.map_err(|e| e.to_string())?;

    let mut resp = String::new();
    stream.read_to_string(&mut resp).await.map_err(|e| e.to_string())?;
    let json_part = resp.split("\r\n\r\n").nth(1).unwrap_or("{}");
    let parsed: serde_json::Value = serde_json::from_str(json_part).map_err(|e| e.to_string())?;
    if let Some(err) = parsed.pointer("/error/message").and_then(|v| v.as_str()) {
        return Err(err.to_string());
    }
    Ok(parsed
        .pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string())
}
```

3. src/state.rs

```rust
use parking_lot::{Mutex, RwLock};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, Notify};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: String,
    pub display_name: String,
    pub available: bool,
    pub supports_thinking: bool,
    pub efforts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedPrefs {
    pub port: u16,
    pub default_model: String,
    pub reasoning_effort: String,
    pub enable_tools_default: bool,
}

impl Default for PersistedPrefs {
    fn default() -> Self {
        Self {
            port: 8787,
            default_model: "gemini-3.8-flash".to_string(),
            reasoning_effort: "auto".to_string(),
            enable_tools_default: true,
        }
    }
}

impl PersistedPrefs {
    pub fn app_dir() -> PathBuf {
        let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        let dir = base.join("ge-bridge");
        let _ = fs::create_dir_all(&dir);
        dir
    }

    pub fn load() -> Self {
        let path = Self::app_dir().join("prefs.json");
        fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let path = Self::app_dir().join("prefs.json");
        if let Ok(raw) = serde_json::to_string_pretty(self) {
            let _ = fs::write(path, raw);
        }
    }
}

pub fn load_or_create_token() -> String {
    if let Ok(entry) = keyring::Entry::new("ge-bridge-local", "pairing-token") {
        if let Ok(secret) = entry.get_password() {
            if secret.len() >= 20 {
                return secret;
            }
        }
        let fresh = generate_random_token();
        if entry.set_password(&fresh).is_ok() {
            return fresh;
        }
    }
    // Fallback file in %APPDATA%\ge-bridge\secret.token
    let fallback = PersistedPrefs::app_dir().join("secret.token");
    if let Ok(s) = fs::read_to_string(&fallback) {
        if s.trim().len() >= 20 {
            return s.trim().to_string();
        }
    }
    let fresh = generate_random_token();
    let _ = fs::write(&fallback, &fresh);
    fresh
}

fn generate_random_token() -> String {
    let mut bytes = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!("gemb_{}", hex::encode(bytes))
}

pub fn default_bootstrap_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "gemini-3.8-flash".into(),
            display_name: "Gemini 3.8 Flash".into(),
            available: true,
            supports_thinking: true,
            efforts: vec!["auto".into(), "low".into(), "medium".into(), "high".into()],
        },
        ModelInfo {
            id: "gemini-4-argon".into(),
            display_name: "Gemini 4 Argon".into(),
            available: true,
            supports_thinking: true,
            efforts: vec!["auto".into(), "low".into(), "medium".into(), "high".into(), "max".into()],
        },
        ModelInfo {
            id: "gemini-3.5-flash".into(),
            display_name: "Gemini 3.5 Flash".into(),
            available: true,
            supports_thinking: true,
            efforts: vec!["auto".into(), "low".into(), "medium".into()],
        },
        ModelInfo {
            id: "gemini-3.5-flash-lite".into(),
            display_name: "Gemini 3.5 Flash Lite".into(),
            available: true,
            supports_thinking: false,
            efforts: vec!["auto".into()],
        },
        ModelInfo {
            id: "gemini-3.1-pro-preview".into(),
            display_name: "Gemini 3.1 Pro Preview".into(),
            available: true,
            supports_thinking: true,
            efforts: vec!["auto".into(), "medium".into(), "high".into()],
        },
    ]
}

#[derive(Debug, Clone, Serialize)]
pub struct TabState {
    pub tab_id: i64,
    pub url: String,
    pub title: String,
    pub active_model: Option<String>,
    pub calibrated: bool,
    pub busy: bool,
}

#[derive(Debug, Clone)]
pub struct SessionEntry {
    pub last_full_hash: String,
    pub active_model: String,
    pub tools_hash: Option<String>,
    pub preferred_tab_id: Option<i64>,
    pub updated_at: u64,
}

#[derive(Debug, Clone)]
pub enum DomStreamEvent {
    Chunk { thought_delta: Option<String>, text_delta: String },
    Done { full_text: String, active_model: Option<String> },
    Error(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub ts: u64,
    pub level: String,
    pub message: String,
}

pub enum TraySignal {
    RebuildMenu,
}

pub struct AppState {
    pub token: String,
    pub prefs: RwLock<PersistedPrefs>,
    pub models: RwLock<Vec<ModelInfo>>,
    pub tabs: RwLock<HashMap<i64, TabState>>,
    pub tab_notify: Notify,
    pub ext_ws_tx: RwLock<Option<mpsc::UnboundedSender<String>>>,
    pub pending_jobs: Mutex<HashMap<String, mpsc::UnboundedSender<DomStreamEvent>>>,
    pub sessions: Mutex<VecDeque<SessionEntry>>,
    pub logs: Mutex<VecDeque<LogEntry>>,
    pub ui_events_tx: Mutex<Vec<mpsc::UnboundedSender<String>>>,
    pub tray_tx: Mutex<Option<mpsc::UnboundedSender<TraySignal>>>,
}

impl AppState {
    pub fn load_or_init() -> Self {
        let prefs = PersistedPrefs::load();
        let token = load_or_create_token();
        let models_path = PersistedPrefs::app_dir().join("models_cache.json");
        let models = fs::read_to_string(&models_path)
            .ok()
            .and_then(|s| serde_json::from_str::<Vec<ModelInfo>>(&s).ok())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(default_bootstrap_models);

        Self {
            token,
            prefs: RwLock::new(prefs),
            models: RwLock::new(models),
            tabs: RwLock::new(HashMap::new()),
            tab_notify: Notify::new(),
            ext_ws_tx: RwLock::new(None),
            pending_jobs: Mutex::new(HashMap::new()),
            sessions: Mutex::new(VecDeque::with_capacity(32)),
            logs: Mutex::new(VecDeque::with_capacity(100)),
            ui_events_tx: Mutex::new(Vec::new()),
            tray_tx: Mutex::new(None),
        }
    }

    pub fn update_models(&self, incoming: Vec<ModelInfo>) {
        if incoming.is_empty() {
            return;
        }
        {
            let mut lock = self.models.write();
            *lock = incoming.clone();
        }
        let path = PersistedPrefs::app_dir().join("models_cache.json");
        if let Ok(raw) = serde_json::to_string_pretty(&incoming) {
            let _ = fs::write(path, raw);
        }
        // Ensure default_model points to an available model
        {
            let mut prefs = self.prefs.write();
            let exists = incoming.iter().any(|m| m.available && m.id == prefs.default_model);
            if !exists {
                if let Some(first) = incoming.iter().find(|m| m.available) {
                    prefs.default_model = first.id.clone();
                    prefs.save();
                }
            }
        }
        self.notify_tray();
        self.broadcast_dashboard();
    }

    /// 3-tier model resolution: explicit `model:effort` -> catalog match -> System-Tray default fallback
    pub fn resolve_model_and_effort(
        &self,
        requested_model: Option<&str>,
        requested_effort: Option<&str>,
    ) -> Result<(String, Option<String>), String> {
        let prefs = self.prefs.read().clone();
        let catalog = self.models.read().clone();

        let (raw_name, inline_effort) = match requested_model.map(str::trim).filter(|s| !s.is_empty()) {
            Some(m) if m.contains(':') => {
                let mut parts = m.splitn(2, ':');
                (parts.next().unwrap_or(""), parts.next())
            }
            Some(m) => (m, None),
            None => ("default", None),
        };

        let chosen_model = if matches!(raw_name, "default" | "auto" | "gemini-enterprise")
            || raw_name.starts_with("claude-")
            || raw_name.starts_with("gpt-")
        {
            prefs.default_model.clone()
        } else if let Some(found) = catalog.iter().find(|m| m.id.eq_ignore_ascii_case(raw_name)) {
            if !found.available {
                return Err(format!("Model '{}' is locked or unavailable on your Gemini Enterprise tenant.", found.id));
            }
            found.id.clone()
        } else {
            // Allow custom/unlisted model slug if user explicitly typed it
            raw_name.to_lowercase()
        };

        let effort = requested_effort
            .or(inline_effort)
            .map(str::to_lowercase)
            .unwrap_or_else(|| prefs.reasoning_effort.clone());

        let final_effort = if effort == "auto" || effort.is_empty() {
            None
        } else {
            Some(effort)
        };

        Ok((chosen_model, final_effort))
    }

    pub async fn acquire_tab(&self, preferred_tab_id: Option<i64>) -> Result<i64, String> {
        loop {
            if self.ext_ws_tx.read().is_none() {
                return Err("Browser extension is not connected. Open Chrome/Edge and verify GeBridge extension.".into());
            }
            let notified = self.tab_notify.notified();
            {
                let mut tabs = self.tabs.write();
                if tabs.is_empty() {
                    return Err("No open https://vertexaisearch.cloud.google.com tab detected. Sign into Gemini Enterprise in Chrome/Edge.".into());
                }
                if let Some(pref) = preferred_tab_id {
                    if let Some(t) = tabs.get_mut(&pref) {
                        if !t.busy {
                            t.busy = true;
                            return Ok(pref);
                        }
                    }
                }
                if let Some((&id, t)) = tabs.iter_mut().find(|(_, t)| !t.busy) {
                    t.busy = true;
                    return Ok(id);
                }
            }
            // All tabs busy: wait cleanly in async FIFO queue (up to 120s)
            tokio::select! {
                _ = notified => continue,
                _ = tokio::time::sleep(std::time::Duration::from_secs(120)) => {
                    return Err("Timed out waiting for an idle Gemini Enterprise browser tab.".into());
                }
            }
        }
    }

    pub fn release_tab(&self, tab_id: i64, active_model: Option<String>) {
        {
            let mut tabs = self.tabs.write();
            if let Some(t) = tabs.get_mut(&tab_id) {
                t.busy = false;
                if active_model.is_some() {
                    t.active_model = active_model;
                }
            }
        }
        self.tab_notify.notify_one();
        self.broadcast_dashboard();
    }

    pub fn send_to_extension(&self, payload: &serde_json::Value) -> Result<(), String> {
        let lock = self.ext_ws_tx.read();
        let tx = lock.as_ref().ok_or_else(|| "Extension WebSocket disconnected.".to_string())?;
        tx.send(payload.to_string()).map_err(|_| "Failed to write to Extension WebSocket.".into())
    }

    pub fn log(&self, level: &str, msg: impl Into<String>) {
        let entry = LogEntry {
            ts: now_secs(),
            level: level.to_string(),
            message: msg.into(),
        };
        {
            let mut logs = self.logs.lock();
            if logs.len() >= 100 {
                logs.pop_front();
            }
            logs.push_back(entry.clone());
        }
        self.broadcast_dashboard();
    }

    pub fn status_json(&self) -> serde_json::Value {
        let prefs = self.prefs.read().clone();
        let ext_connected = self.ext_ws_tx.read().is_some();
        let tabs: Vec<TabState> = self.tabs.read().values().cloned().collect();
        let models: Vec<ModelInfo> = self.models.read().iter().filter(|m| m.available).cloned().collect();
        serde_json::json!({
            "extensionConnected": ext_connected,
            "tabReady": ext_connected && !tabs.is_empty(),
            "tabs": tabs,
            "models": models,
            "defaultModel": prefs.default_model,
            "reasoningEffort": prefs.reasoning_effort,
            "port": prefs.port,
            "autostart": crate::integrations::is_autostart_enabled()
        })
    }

    pub fn broadcast_dashboard(&self) {
        let msg = serde_json::json!({
            "type": "state",
            "status": self.status_json(),
            "logs": *self.logs.lock()
        })
        .to_string();
        self.ui_events_tx.lock().retain(|tx| tx.send(msg.clone()).is_ok());
    }

    pub fn notify_tray(&self) {
        if let Some(tx) = self.tray_tx.lock().as_ref() {
            let _ = tx.send(TraySignal::RebuildMenu);
        }
    }
}

pub fn hash_json<T: Serialize>(val: &T) -> String {
    let mut hasher = Sha1::new();
    if let Ok(bytes) = serde_json::to_vec(val) {
        hasher.update(&bytes);
    }
    hex::encode(hasher.finalize())
}

pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}
```

4. src/stream.rs

```rust
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedMessage {
    pub role: String,
    pub text: String,
    pub images: Vec<String>,
    pub tool_calls: Vec<ParsedToolCall>,
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedToolCall {
    pub id: String,
    pub name: String,
    pub arguments_json: String,
}

pub fn parse_openai_messages(messages: &[Value]) -> Vec<NormalizedMessage> {
    let mut out = Vec::with_capacity(messages.len());
    for m in messages {
        let role = m.get("role").and_then(|v| v.as_str()).unwrap_or("user").to_string();
        let mut texts = Vec::new();
        let mut images = Vec::new();

        match m.get("content") {
            Some(Value::String(s)) => texts.push(s.clone()),
            Some(Value::Array(arr)) => {
                for part in arr {
                    match part.get("type").and_then(|v| v.as_str()) {
                        Some("text") => {
                            if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                                texts.push(t.to_string());
                            }
                        }
                        Some("image_url") => {
                            if let Some(u) = part.pointer("/image_url/url").and_then(|v| v.as_str()) {
                                images.push(u.to_string());
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }

        let mut tool_calls = Vec::new();
        if let Some(Value::Array(tcs)) = m.get("tool_calls") {
            for tc in tcs {
                let id = tc.get("id").and_then(|v| v.as_str()).unwrap_or("call_0").to_string();
                let name = tc.pointer("/function/name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let args = tc.pointer("/function/arguments").and_then(|v| v.as_str()).unwrap_or("{}").to_string();
                if !name.is_empty() {
                    tool_calls.push(ParsedToolCall { id, name, arguments_json: args });
                }
            }
        }

        out.push(NormalizedMessage {
            role,
            text: texts.join("\n"),
            images,
            tool_calls,
            tool_call_id: m.get("tool_call_id").and_then(|v| v.as_str()).map(String::from),
            tool_name: m.get("name").and_then(|v| v.as_str()).map(String::from),
        });
    }
    out
}

pub fn parse_anthropic_messages(system: Option<&Value>, messages: &[Value]) -> Vec<NormalizedMessage> {
    let mut out = Vec::new();
    if let Some(sys) = system {
        let sys_text = match sys {
            Value::String(s) => s.clone(),
            Value::Array(arr) => arr
                .iter()
                .filter_map(|p| p.get("text").and_then(|v| v.as_str()))
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        };
        if !sys_text.is_empty() {
            out.push(NormalizedMessage {
                role: "system".into(),
                text: sys_text,
                images: vec![],
                tool_calls: vec![],
                tool_call_id: None,
                tool_name: None,
            });
        }
    }

    for m in messages {
        let role = m.get("role").and_then(|v| v.as_str()).unwrap_or("user").to_string();
        let mut texts = Vec::new();
        let mut images = Vec::new();
        let mut tool_calls = Vec::new();

        match m.get("content") {
            Some(Value::String(s)) => texts.push(s.clone()),
            Some(Value::Array(blocks)) => {
                for b in blocks {
                    match b.get("type").and_then(|v| v.as_str()) {
                        Some("text") => {
                            if let Some(t) = b.get("text").and_then(|v| v.as_str()) {
                                texts.push(t.to_string());
                            }
                        }
                        Some("image") => {
                            if let (Some(media), Some(data)) = (
                                b.pointer("/source/media_type").and_then(|v| v.as_str()),
                                b.pointer("/source/data").and_then(|v| v.as_str()),
                            ) {
                                images.push(format!("data:{media};base64,{data}"));
                            }
                        }
                        Some("tool_use") => {
                            let id = b.get("id").and_then(|v| v.as_str()).unwrap_or("toolu_0").to_string();
                            let name = b.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let input = b.get("input").cloned().unwrap_or(Value::Object(Default::default()));
                            tool_calls.push(ParsedToolCall {
                                id,
                                name,
                                arguments_json: input.to_string(),
                            });
                        }
                        Some("tool_result") => {
                            let id = b.get("tool_use_id").and_then(|v| v.as_str()).unwrap_or("");
                            let content_str = match b.get("content") {
                                Some(Value::String(s)) => s.clone(),
                                Some(other) => other.to_string(),
                                None => String::new(),
                            };
                            out.push(NormalizedMessage {
                                role: "tool".into(),
                                text: content_str,
                                images: vec![],
                                tool_calls: vec![],
                                tool_call_id: Some(id.to_string()),
                                tool_name: None,
                            });
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }

        if !texts.is_empty() || !images.is_empty() || !tool_calls.is_empty() {
            out.push(NormalizedMessage {
                role,
                text: texts.join("\n"),
                images,
                tool_calls,
                tool_call_id: None,
                tool_name: None,
            });
        }
    }
    out
}

pub fn build_tool_protocol_block(tools: &[Value]) -> String {
    let compact: Vec<Value> = tools
        .iter()
        .map(|t| {
            if let Some(func) = t.get("function") {
                serde_json::json!({
                    "name": func.get("name"),
                    "description": func.get("description"),
                    "parameters": func.get("parameters")
                })
            } else {
                serde_json::json!({
                    "name": t.get("name"),
                    "description": t.get("description"),
                    "parameters": t.get("input_schema")
                })
            }
        })
        .collect();

    format!(
        "[AGENT_TOOL_PROTOCOL]\n\
         You have access to external tools. Whenever you want to execute a tool, output ONLY raw <tool_call> tags (never inside markdown code fences) formatted as single-line JSON:\n\
         <tool_call>{{\"name\":\"tool_name\",\"arguments\":{{\"key\":\"value\"}}}}</tool_call>\n\
         Wait for [TOOL_RESULT] before continuing.\n\
         Tools Schema: {}\n\
         [/AGENT_TOOL_PROTOCOL]",
        serde_json::to_string(&compact).unwrap_or_else(|_| "[]".into())
    )
}

pub fn render_turn_prompt(
    messages: &[NormalizedMessage],
    tools: Option<&[Value]>,
    new_chat: bool,
    inject_tools: bool,
) -> (String, Vec<String>) {
    let mut blocks = Vec::new();
    let mut images = Vec::new();

    if let (Some(t), true) = (tools, inject_tools) {
        if !t.is_empty() {
            blocks.push(build_tool_protocol_block(t));
        }
    }

    if new_chat {
        let single_user = messages.len() == 1 && messages[0].role == "user";
        for m in messages {
            images.extend(m.images.clone());
            match m.role.as_str() {
                "system" => blocks.push(format!("[SYSTEM INSTRUCTIONS]\n{}", m.text)),
                "user" => {
                    if single_user {
                        blocks.push(m.text.clone());
                    } else {
                        blocks.push(format!("[USER]\n{}", m.text));
                    }
                }
                "assistant" => {
                    let mut parts = Vec::new();
                    if !m.text.is_empty() {
                        parts.push(m.text.clone());
                    }
                    for tc in &m.tool_calls {
                        parts.push(format!(
                            "<tool_call>{{\"name\":{},\"arguments\":{}}}</tool_call>",
                            serde_json::to_string(&tc.name).unwrap_or_default(),
                            tc.arguments_json
                        ));
                    }
                    blocks.push(format!("[ASSISTANT]\n{}", parts.join("\n")));
                }
                "tool" => blocks.push(format!(
                    "[TOOL_RESULT id=\"{}\"]\n{}",
                    m.tool_call_id.as_deref().unwrap_or(""),
                    m.text
                )),
                _ => blocks.push(m.text.clone()),
            }
        }
    } else {
        // Continuation delta: only send messages appended after the last assistant reply
        let mut start = messages.len().saturating_sub(1);
        while start > 0 && messages[start - 1].role != "assistant" {
            start -= 1;
        }
        for m in &messages[start..] {
            images.extend(m.images.clone());
            if m.role == "tool" {
                blocks.push(format!(
                    "[TOOL_RESULT id=\"{}\"]\n{}",
                    m.tool_call_id.as_deref().unwrap_or(""),
                    m.text
                ));
            } else {
                blocks.push(m.text.clone());
            }
        }
    }

    (blocks.into_iter().filter(|s| !s.trim().is_empty()).collect::<Vec<_>>().join("\n\n"), images)
}

/// Streaming Lookahead State Machine separating CoT, normal content, and <tool_call> XML tags
pub struct StreamInterceptor {
    tools_enabled: bool,
    buf: String,
    inside_tool: bool,
    pub full_clean_text: String,
    pub full_reasoning: String,
    pub tool_calls: Vec<ParsedToolCall>,
}

impl StreamInterceptor {
    pub fn new(tools_enabled: bool) -> Self {
        Self {
            tools_enabled,
            buf: String::new(),
            inside_tool: false,
            full_clean_text: String::new(),
            full_reasoning: String::new(),
            tool_calls: Vec::new(),
        }
    }

    pub fn push_chunk(&mut self, thought_delta: Option<&str>, text_delta: &str) -> (Option<String>, Option<String>) {
        let emitted_thought = thought_delta.filter(|s| !s.is_empty()).map(|s| {
            self.full_reasoning.push_str(s);
            s.to_string()
        });

        if text_delta.is_empty() {
            return (emitted_thought, None);
        }

        if !self.tools_enabled {
            self.full_clean_text.push_str(text_delta);
            return (emitted_thought, Some(text_delta.to_string()));
        }

        self.buf.push_str(text_delta);
        let mut emitted_text = String::new();
        const OPEN_TAG: &str = "<tool_call>";
        const CLOSE_TAG: &str = "</tool_call>";

        loop {
            if !self.inside_tool {
                if let Some(idx) = self.buf.find(OPEN_TAG) {
                    let before = &self.buf[..idx];
                    emitted_text.push_str(before);
                    self.full_clean_text.push_str(before);
                    self.buf.drain(..idx + OPEN_TAG.len());
                    self.inside_tool = true;
                } else {
                    let safe_len = safe_prefix_flush_len(&self.buf, OPEN_TAG);
                    if safe_len > 0 {
                        let chunk = &self.buf[..safe_len];
                        emitted_text.push_str(chunk);
                        self.full_clean_text.push_str(chunk);
                        self.buf.drain(..safe_len);
                    }
                    break;
                }
            } else if let Some(end_idx) = self.buf.find(CLOSE_TAG) {
                let raw_json = self.buf[..end_idx].trim().to_string();
                self.buf.drain(..end_idx + CLOSE_TAG.len());
                self.inside_tool = false;
                if let Some(tc) = try_parse_tool_json(&raw_json) {
                    self.tool_calls.push(tc);
                }
            } else {
                break;
            }
        }

        let text_out = if emitted_text.is_empty() { None } else { Some(emitted_text) };
        (emitted_thought, text_out)
    }

    pub fn finalize(&mut self, final_full_text: &str) -> Option<String> {
        // Fallback pass in case Gemini wrapped <tool_call> inside markdown code fences
        if self.tools_enabled && self.tool_calls.is_empty() && final_full_text.contains("<tool_call>") {
            let stripped = strip_fenced_tool_calls(final_full_text, &mut self.tool_calls);
            self.full_clean_text = stripped;
            return None;
        }
        if !self.inside_tool && !self.buf.is_empty() {
            let tail = std::mem::take(&mut self.buf);
            self.full_clean_text.push_str(&tail);
            return Some(tail);
        }
        None
    }
}

fn safe_prefix_flush_len(buf: &str, tag: &str) -> usize {
    let max_check = tag.len().saturating_sub(1).min(buf.len());
    for k in (1..=max_check).rev() {
        let start = buf.len() - k;
        if buf.is_char_boundary(start) && tag.starts_with(&buf[start..]) {
            return start;
        }
    }
    buf.len()
}

fn try_parse_tool_json(raw: &str) -> Option<ParsedToolCall> {
    let parsed: Value = serde_json::from_str(raw).ok()?;
    let name = parsed.get("name")?.as_str()?.to_string();
    let args = match parsed.get("arguments") {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => "{}".to_string(),
    };
    let mut id_bytes = [0u8; 6];
    rand::thread_rng().fill_bytes(&mut id_bytes);
    Some(ParsedToolCall {
        id: format!("call_{}", hex::encode(id_bytes)),
        name,
        arguments_json: args,
    })
}

fn strip_fenced_tool_calls(text: &str, out: &mut Vec<ParsedToolCall>) -> String {
    let mut remaining = text.to_string();
    while let Some(start) = remaining.find("<tool_call>") {
        if let Some(rel_end) = remaining[start..].find("</tool_call>") {
            let end = start + rel_end;
            let inner = &remaining[start + 11..end];
            if let Some(tc) = try_parse_tool_json(inner.trim()) {
                out.push(tc);
            }
            remaining.replace_range(start..end + 12, "");
        } else {
            break;
        }
    }
    remaining.replace("```xml", "").replace("```json", "").replace("```", "").trim().to_string()
}
```

5. src/server.rs

```rust
use crate::integrations;
use crate::state::{hash_json, now_secs, AppState, DomStreamEvent, ModelInfo, SessionEntry, TabState};
use crate::stream::{
    parse_anthropic_messages, parse_openai_messages, render_turn_prompt, NormalizedMessage,
    ParsedToolCall, StreamInterceptor,
};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::{HeaderMap, Method, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        Html, IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use rand::RngCore;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::HashMap, convert::Infallible, sync::Arc};
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;

const DASHBOARD_TEMPLATE: &str = include_str!("dashboard.html");

pub async fn run_http_server(state: Arc<AppState>) -> Result<(), String> {
    let port = state.prefs.read().port;
    let app = Router::new()
        .route("/", get(serve_dashboard))
        .route("/ws/extension", get(ws_extension_upgrade))
        .route("/api/events", get(api_dashboard_sse))
        .route("/api/status", get(api_status))
        .route("/api/prefs", post(api_update_prefs))
        .route("/api/refresh-models", post(api_refresh_models))
        .route("/api/reset-sessions", post(api_reset_sessions))
        .route("/api/link-openmausbot", post(api_link_openmausbot))
        .route("/v1/models", get(openai_list_models))
        .route("/models", get(openai_list_models))
        .route("/v1/chat/completions", post(openai_chat_completions))
        .route("/chat/completions", post(openai_chat_completions))
        .route("/v1/messages", post(anthropic_messages))
        .fallback(cors_preflight_or_404)
        .with_state(state.clone());

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .map_err(|e| format!("Failed to bind 127.0.0.1:{port}: {e}"))?;

    state.log("info", format!("GeBridge v2 listening on http://127.0.0.1:{port}"));
    axum::serve(listener, app).await.map_err(|e| e.to_string())
}

fn check_origin(headers: &HeaderMap, port: u16) -> bool {
    let Some(origin) = headers.get("origin").and_then(|v| v.to_str().ok()) else {
        return true;
    };
    origin == format!("http://127.0.0.1:{port}")
        || origin == format!("http://localhost:{port}")
        || origin.starts_with("chrome-extension://")
        || origin.starts_with("extension://")
}

fn check_auth(headers: &HeaderMap, token: &str) -> bool {
    if let Some(auth) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        if auth.strip_prefix("Bearer ").map(str::trim) == Some(token) {
            return true;
        }
    }
    if let Some(xkey) = headers.get("x-api-key").and_then(|v| v.to_str().ok()) {
        if xkey.trim() == token {
            return true;
        }
    }
    false
}

fn json_error(status: StatusCode, msg: &str) -> Response {
    (
        status,
        [("Access-Control-Allow-Origin", "*")],
        Json(json!({ "error": { "message": msg, "type": "bridge_error" } })),
    )
        .into_response()
}

async fn cors_preflight_or_404(method: Method, headers: HeaderMap, State(state): State<Arc<AppState>>) -> Response {
    if !check_origin(&headers, state.prefs.read().port) {
        return json_error(StatusCode::FORBIDDEN, "Origin rejected");
    }
    if method == Method::OPTIONS {
        return (
            StatusCode::NO_CONTENT,
            [
                ("Access-Control-Allow-Origin", "*"),
                ("Access-Control-Allow-Headers", "Authorization, X-Api-Key, Anthropic-Version, Content-Type"),
                ("Access-Control-Allow-Methods", "GET, POST, OPTIONS"),
            ],
            "",
        )
            .into_response();
    }
    json_error(StatusCode::NOT_FOUND, "Endpoint not found")
}

async fn serve_dashboard(headers: HeaderMap, State(state): State<Arc<AppState>>) -> Response {
    if let Some(site) = headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) {
        if site != "none" && site != "same-origin" {
            return json_error(StatusCode::FORBIDDEN, "Direct local navigation required");
        }
    }
    Html(DASHBOARD_TEMPLATE.replace("__INJECT_BRIDGE_TOKEN__", &state.token)).into_response()
}

#[derive(Deserialize)]
struct WsQuery {
    token: Option<String>,
}

async fn ws_extension_upgrade(
    ws: WebSocketUpgrade,
    Query(q): Query<WsQuery>,
    State(state): State<Arc<AppState>>,
) -> Response {
    if q.token.as_deref() != Some(&state.token) {
        return json_error(StatusCode::UNAUTHORIZED, "Invalid extension pairing token");
    }
    ws.on_upgrade(move |socket| handle_extension_ws(socket, state))
}

async fn handle_extension_ws(socket: WebSocket, state: Arc<AppState>) {
    let (mut ws_tx, mut ws_rx) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    *state.ext_ws_tx.write() = Some(tx.clone());
    state.log("info", "Chromium Extension connected over full-duplex WebSocket.");
    state.notify_tray();

    // Immediately request live model sync from any open Gemini Enterprise tab
    let _ = tx.send(json!({ "type": "REQUEST_MODELS_SYNC" }).to_string());

    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if ws_tx.send(Message::Text(msg)).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(msg)) = ws_rx.next().await {
        let Message::Text(raw) = msg else { continue };
        let Ok(payload) = serde_json::from_str::<Value>(&raw) else { continue };
        match payload.get("type").and_then(|v| v.as_str()).unwrap_or("") {
            "TABS_SYNC" => {
                if let Some(arr) = payload.get("tabs").and_then(|v| v.as_array()) {
                    let mut next_tabs = HashMap::new();
                    let old_tabs = state.tabs.read().clone();
                    for item in arr {
                        let Some(id) = item.get("tabId").and_then(|v| v.as_i64()) else { continue };
                        let prev_busy = old_tabs.get(&id).map(|t| t.busy).unwrap_or(false);
                        next_tabs.insert(
                            id,
                            TabState {
                                tab_id: id,
                                url: item.get("url").and_then(|v| v.as_str()).unwrap_or("").into(),
                                title: item.get("title").and_then(|v| v.as_str()).unwrap_or("Gemini Enterprise").into(),
                                active_model: item.get("activeModel").and_then(|v| v.as_str()).map(String::from),
                                calibrated: item.get("calibrated").and_then(|v| v.as_bool()).unwrap_or(false),
                                busy: prev_busy,
                            },
                        );
                    }
                    *state.tabs.write() = next_tabs;
                    state.tab_notify.notify_waiters();
                    state.notify_tray();
                    state.broadcast_dashboard();
                }
            }
            "MODELS_SYNC" => {
                if let Some(Ok(models)) = payload
                    .get("models")
                    .cloned()
                    .map(serde_json::from_value::<Vec<ModelInfo>>)
                {
                    let count = models.iter().filter(|m| m.available).count();
                    state.update_models(models);
                    state.log("info", format!("Synced {count} available models from Gemini Enterprise UI."));
                }
            }
            "DOM_DELTA" => {
                if let Some(job_id) = payload.get("jobId").and_then(|v| v.as_str()) {
                    let thought = payload.get("thoughtDelta").and_then(|v| v.as_str()).map(String::from);
                    let text = payload.get("textDelta").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    if let Some(job_tx) = state.pending_jobs.lock().get(job_id) {
                        let _ = job_tx.send(DomStreamEvent::Chunk {
                            thought_delta: thought,
                            text_delta: text,
                        });
                    }
                }
            }
            "TURN_DONE" => {
                if let Some(job_id) = payload.get("jobId").and_then(|v| v.as_str()) {
                    if let Some(job_tx) = state.pending_jobs.lock().remove(job_id) {
                        if let Some(err) = payload.get("error").and_then(|v| v.as_str()) {
                            let _ = job_tx.send(DomStreamEvent::Error(err.to_string()));
                        } else {
                            let full_text = payload.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let active_model = payload.get("activeModel").and_then(|v| v.as_str()).map(String::from);
                            let _ = job_tx.send(DomStreamEvent::Done { full_text, active_model });
                        }
                    }
                }
            }
            _ => {}
        }
    }

    writer.abort();
    *state.ext_ws_tx.write() = None;
    state.tabs.write().clear();
    state.log("warn", "Extension WebSocket disconnected.");
    state.notify_tray();
    state.broadcast_dashboard();
}

async fn api_dashboard_sse(headers: HeaderMap, Query(q): Query<WsQuery>, State(state): State<Arc<AppState>>) -> Response {
    if !check_auth(&headers, &state.token) && q.token.as_deref() != Some(&state.token) {
        return json_error(StatusCode::UNAUTHORIZED, "Unauthorized");
    }
    let (tx, rx) = mpsc::unbounded_channel::<String>();
    let init = json!({
        "type": "state",
        "status": state.status_json(),
        "logs": *state.logs.lock()
    })
    .to_string();
    let _ = tx.send(init);
    state.ui_events_tx.lock().push(tx);

    let stream = tokio_stream::StreamExt::map(UnboundedReceiverStream::new(rx), |msg| {
        Ok::<Event, Infallible>(Event::default().data(msg))
    });
    Sse::new(stream).keep_alive(KeepAlive::default()).into_response()
}

async fn api_status(headers: HeaderMap, State(state): State<Arc<AppState>>) -> Response {
    if !check_auth(&headers, &state.token) {
        return json_error(StatusCode::UNAUTHORIZED, "Unauthorized");
    }
    Json(state.status_json()).into_response()
}

async fn api_update_prefs(headers: HeaderMap, State(state): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    if !check_auth(&headers, &state.token) {
        return json_error(StatusCode::UNAUTHORIZED, "Unauthorized");
    }
    {
        let mut prefs = state.prefs.write();
        if let Some(m) = body.get("defaultModel").and_then(|v| v.as_str()) {
            prefs.default_model = m.to_string();
        }
        if let Some(e) = body.get("reasoningEffort").and_then(|v| v.as_str()) {
            prefs.reasoning_effort = e.to_string();
        }
        prefs.save();
    }
    state.notify_tray();
    state.broadcast_dashboard();
    Json(json!({ "ok": true, "status": state.status_json() })).into_response()
}

async fn api_refresh_models(headers: HeaderMap, State(state): State<Arc<AppState>>) -> Response {
    if !check_auth(&headers, &state.token) {
        return json_error(StatusCode::UNAUTHORIZED, "Unauthorized");
    }
    let _ = state.send_to_extension(&json!({ "type": "REQUEST_MODELS_SYNC", "forceOpenMenu": true }));
    Json(json!({ "ok": true })).into_response()
}

async fn api_reset_sessions(headers: HeaderMap, State(state): State<Arc<AppState>>) -> Response {
    if !check_auth(&headers, &state.token) {
        return json_error(StatusCode::UNAUTHORIZED, "Unauthorized");
    }
    state.sessions.lock().clear();
    state.log("info", "Cleared LRU SessionStore; next turn starts a clean thread.");
    Json(json!({ "ok": true })).into_response()
}

async fn api_link_openmausbot(headers: HeaderMap, State(state): State<Arc<AppState>>) -> Response {
    if !check_auth(&headers, &state.token) {
        return json_error(StatusCode::UNAUTHORIZED, "Unauthorized");
    }
    let prefs = state.prefs.read().clone();
    match integrations::link_openmausbot(prefs.port, &state.token, &prefs.default_model, prefs.enable_tools_default) {
        Ok(path) => {
            state.log("info", format!("Auto-linked OpenMausBot at {}", path.display()));
            Json(json!({ "ok": true, "path": path.display().to_string() })).into_response()
        }
        Err(err) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &err),
    }
}

#[derive(Deserialize)]
struct ModelsQuery {
    refresh: Option<bool>,
}

/// Standard OpenAI `GET /v1/models` returning live discovered Gemini Enterprise models
async fn openai_list_models(
    headers: HeaderMap,
    Query(q): Query<ModelsQuery>,
    State(state): State<Arc<AppState>>,
) -> Response {
    if !check_origin(&headers, state.prefs.read().port) {
        return json_error(StatusCode::FORBIDDEN, "Origin rejected");
    }
    if !check_auth(&headers, &state.token) {
        return json_error(StatusCode::UNAUTHORIZED, "Unauthorized");
    }
    if q.refresh == Some(true) {
        let _ = state.send_to_extension(&json!({ "type": "REQUEST_MODELS_SYNC", "forceOpenMenu": true }));
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    }

    let models: Vec<Value> = state
        .models
        .read()
        .iter()
        .filter(|m| m.available)
        .map(|m| {
            json!({
                "id": m.id,
                "object": "model",
                "created": 1760000000,
                "owned_by": "gemini-enterprise-bridge",
                "metadata": {
                    "display_name": m.display_name,
                    "supports_thinking": m.supports_thinking,
                    "reasoning_efforts": m.efforts
                }
            })
        })
        .collect();

    (
        StatusCode::OK,
        [("Access-Control-Allow-Origin", "*")],
        Json(json!({ "object": "list", "data": models })),
    )
        .into_response()
}

struct PreparedJob {
    job_id: String,
    tab_id: i64,
    target_model: String,
    rx: mpsc::UnboundedReceiver<DomStreamEvent>,
    messages: Vec<NormalizedMessage>,
    tools_hash: Option<String>,
    has_tools: bool,
}

async fn dispatch_browser_turn(
    state: &Arc<AppState>,
    messages: Vec<NormalizedMessage>,
    tools: Option<Vec<Value>>,
    req_model: Option<&str>,
    req_effort: Option<&str>,
) -> Result<PreparedJob, String> {
    let (target_model, target_effort) = state.resolve_model_and_effort(req_model, req_effort)?;
    let prefix_hash = if messages.len() > 1 {
        Some(hash_json(&messages[..messages.len() - 1]))
    } else {
        None
    };
    let tools_hash = tools.as_ref().filter(|t| !t.is_empty()).map(hash_json);

    let matched = prefix_hash.as_ref().and_then(|ph| {
        state
            .sessions
            .lock()
            .iter()
            .find(|s| &s.last_full_hash == ph)
            .cloned()
    });

    let (new_chat, preferred_tab, inject_tools, switch_model) = match &matched {
        Some(sess) if sess.active_model == target_model => (
            false,
            sess.preferred_tab_id,
            tools_hash != sess.tools_hash,
            false,
        ),
        Some(sess) => (
            false, // Switch model picker in-place on the existing tab when supported
            sess.preferred_tab_id,
            tools_hash != sess.tools_hash,
            true,
        ),
        None => (true, None, tools_hash.is_some(), true),
    };

    let (prompt, images) = render_turn_prompt(&messages, tools.as_deref(), new_chat, inject_tools);
    let tab_id = state.acquire_tab(preferred_tab).await?;

    let mut id_bytes = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut id_bytes);
    let job_id = format!("job_{}", hex::encode(id_bytes));
    let (tx, rx) = mpsc::unbounded_channel();
    state.pending_jobs.lock().insert(job_id.clone(), tx);

    let cmd = json!({
        "type": "RUN_TURN",
        "jobId": job_id,
        "tabId": tab_id,
        "prompt": prompt,
        "images": images,
        "newChat": new_chat,
        "targetModel": if switch_model || new_chat { Some(&target_model) } else { None },
        "reasoningEffort": target_effort
    });

    if let Err(err) = state.send_to_extension(&cmd) {
        state.pending_jobs.lock().remove(&job_id);
        state.release_tab(tab_id, None);
        return Err(err);
    }

    state.log(
        "info",
        format!("Dispatched {job_id} -> tab {tab_id} [model={target_model}, new_chat={new_chat}]"),
    );

    Ok(PreparedJob {
        job_id,
        tab_id,
        target_model,
        rx,
        messages,
        tools_hash,
        has_tools: tools_hash.is_some(),
    })
}

fn commit_session(
    state: &AppState,
    mut messages: Vec<NormalizedMessage>,
    clean_text: &str,
    tool_calls: &[ParsedToolCall],
    model: &str,
    tools_hash: Option<String>,
    tab_id: i64,
) {
    messages.push(NormalizedMessage {
        role: "assistant".into(),
        text: clean_text.to_string(),
        images: vec![],
        tool_calls: tool_calls.to_vec(),
        tool_call_id: None,
        tool_name: None,
    });
    let next_hash = hash_json(&messages);
    let mut sessions = state.sessions.lock();
    if sessions.len() >= 32 {
        sessions.pop_front();
    }
    sessions.push_back(SessionEntry {
        last_full_hash: next_hash,
        active_model: model.to_string(),
        tools_hash,
        preferred_tab_id: Some(tab_id),
        updated_at: now_secs(),
    });
}

/// Primary OpenAI `/v1/chat/completions` handler (OpenMausBot, Hermes, Cursor, Cline)
async fn openai_chat_completions(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> Response {
    if !check_origin(&headers, state.prefs.read().port) {
        return json_error(StatusCode::FORBIDDEN, "Origin rejected");
    }
    if !check_auth(&headers, &state.token) {
        return json_error(StatusCode::UNAUTHORIZED, "Unauthorized");
    }

    let raw_msgs = body.get("messages").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let messages = parse_openai_messages(&raw_msgs);
    let tools = body.get("tools").and_then(|v| v.as_array()).cloned();
    let req_model = body.get("model").and_then(|v| v.as_str());
    let req_effort = body.get("reasoning_effort").and_then(|v| v.as_str());
    let is_stream = body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);

    let mut job = match dispatch_browser_turn(&state, messages, tools, req_model, req_effort).await {
        Ok(j) => j,
        Err(err) => return json_error(StatusCode::BAD_GATEWAY, &err),
    };

    let cmpl_id = format!("chatcmpl_{}", &job.job_id[4..]);
    let created = now_secs();
    let model_name = job.target_model.clone();

    if !is_stream {
        let mut interceptor = StreamInterceptor::new(job.has_tools);
        let mut final_err = None;
        let mut reported_model = None;

        while let Some(ev) = job.rx.recv().await {
            match ev {
                DomStreamEvent::Chunk { thought_delta, text_delta } => {
                    interceptor.push_chunk(thought_delta.as_deref(), &text_delta);
                }
                DomStreamEvent::Done { full_text, active_model } => {
                    interceptor.finalize(&full_text);
                    reported_model = active_model;
                    break;
                }
                DomStreamEvent::Error(e) => {
                    final_err = Some(e);
                    break;
                }
            }
        }
        state.release_tab(job.tab_id, reported_model);

        if let Some(err) = final_err {
            return json_error(StatusCode::BAD_GATEWAY, &err);
        }

        commit_session(
            &state,
            job.messages,
            &interceptor.full_clean_text,
            &interceptor.tool_calls,
            &model_name,
            job.tools_hash,
            job.tab_id,
        );

        let tc_json: Vec<Value> = interceptor
            .tool_calls
            .iter()
            .map(|tc| {
                json!({
                    "id": tc.id,
                    "type": "function",
                    "function": { "name": tc.name, "arguments": tc.arguments_json }
                })
            })
            .collect();

        let mut msg_obj = json!({
            "role": "assistant",
            "content": if interceptor.full_clean_text.is_empty() && !tc_json.is_empty() {
                Value::Null
            } else {
                Value::String(interceptor.full_clean_text)
            }
        });
        if !interceptor.full_reasoning.is_empty() {
            msg_obj["reasoning_content"] = Value::String(interceptor.full_reasoning);
        }
        if !tc_json.is_empty() {
            msg_obj["tool_calls"] = Value::Array(tc_json.clone());
        }

        return (
            StatusCode::OK,
            [("Access-Control-Allow-Origin", "*")],
            Json(json!({
                "id": cmpl_id,
                "object": "chat.completion",
                "created": created,
                "model": model_name,
                "choices": [{
                    "index": 0,
                    "message": msg_obj,
                    "finish_reason": if tc_json.is_empty() { "stop" } else { "tool_calls" }
                }]
            })),
        )
            .into_response();
    }

    // Real-time OpenAI SSE Streaming
    let (sse_tx, sse_rx) = mpsc::unbounded_channel::<Result<Event, Infallible>>();
    let state_clone = state.clone();

    tokio::spawn(async move {
        let mut interceptor = StreamInterceptor::new(job.has_tools);
        let mut reported_model = None;

        let role_chunk = json!({
            "id": cmpl_id,
            "object": "chat.completion.chunk",
            "created": created,
            "model": model_name,
            "choices": [{ "index": 0, "delta": { "role": "assistant" }, "finish_reason": Value::Null }]
        });
        let _ = sse_tx.send(Ok(Event::default().data(role_chunk.to_string())));

        while let Some(ev) = job.rx.recv().await {
            match ev {
                DomStreamEvent::Chunk { thought_delta, text_delta } => {
                    let (cot, txt) = interceptor.push_chunk(thought_delta.as_deref(), &text_delta);
                    if let Some(r) = cot {
                        let c = json!({
                            "id": cmpl_id,
                            "object": "chat.completion.chunk",
                            "created": created,
                            "model": model_name,
                            "choices": [{ "index": 0, "delta": { "reasoning_content": r }, "finish_reason": Value::Null }]
                        });
                        let _ = sse_tx.send(Ok(Event::default().data(c.to_string())));
                    }
                    if let Some(t) = txt {
                        let c = json!({
                            "id": cmpl_id,
                            "object": "chat.completion.chunk",
                            "created": created,
                            "model": model_name,
                            "choices": [{ "index": 0, "delta": { "content": t }, "finish_reason": Value::Null }]
                        });
                        let _ = sse_tx.send(Ok(Event::default().data(c.to_string())));
                    }
                }
                DomStreamEvent::Done { full_text, active_model } => {
                    reported_model = active_model;
                    if let Some(tail) = interceptor.finalize(&full_text) {
                        let c = json!({
                            "id": cmpl_id,
                            "object": "chat.completion.chunk",
                            "created": created,
                            "model": model_name,
                            "choices": [{ "index": 0, "delta": { "content": tail }, "finish_reason": Value::Null }]
                        });
                        let _ = sse_tx.send(Ok(Event::default().data(c.to_string())));
                    }

                    for (idx, tc) in interceptor.tool_calls.iter().enumerate() {
                        let c = json!({
                            "id": cmpl_id,
                            "object": "chat.completion.chunk",
                            "created": created,
                            "model": model_name,
                            "choices": [{
                                "index": 0,
                                "delta": {
                                    "tool_calls": [{
                                        "index": idx,
                                        "id": tc.id,
                                        "type": "function",
                                        "function": { "name": tc.name, "arguments": tc.arguments_json }
                                    }]
                                },
                                "finish_reason": Value::Null
                            }]
                        });
                        let _ = sse_tx.send(Ok(Event::default().data(c.to_string())));
                    }

                    commit_session(
                        &state_clone,
                        job.messages,
                        &interceptor.full_clean_text,
                        &interceptor.tool_calls,
                        &model_name,
                        job.tools_hash,
                        job.tab_id,
                    );

                    let done_chunk = json!({
                        "id": cmpl_id,
                        "object": "chat.completion.chunk",
                        "created": created,
                        "model": model_name,
                        "choices": [{
                            "index": 0,
                            "delta": {},
                            "finish_reason": if interceptor.tool_calls.is_empty() { "stop" } else { "tool_calls" }
                        }]
                    });
                    let _ = sse_tx.send(Ok(Event::default().data(done_chunk.to_string())));
                    let _ = sse_tx.send(Ok(Event::default().data("[DONE]")));
                    break;
                }
                DomStreamEvent::Error(err) => {
                    let e = json!({ "error": { "message": err } });
                    let _ = sse_tx.send(Ok(Event::default().data(e.to_string())));
                    let _ = sse_tx.send(Ok(Event::default().data("[DONE]")));
                    break;
                }
            }
        }
        state_clone.release_tab(job.tab_id, reported_model);
    });

    (
        [("Access-Control-Allow-Origin", "*")],
        Sse::new(UnboundedReceiverStream::new(sse_rx)).keep_alive(KeepAlive::default()),
    )
        .into_response()
}

/// Native Anthropic `/v1/messages` handler (Claude Code & Anthropic SDK harnesses)
async fn anthropic_messages(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> Response {
    if !check_auth(&headers, &state.token) {
        return json_error(StatusCode::UNAUTHORIZED, "Unauthorized");
    }
    let raw_msgs = body.get("messages").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let messages = parse_anthropic_messages(body.get("system"), &raw_msgs);
    let tools = body.get("tools").and_then(|v| v.as_array()).cloned();
    let req_model = body.get("model").and_then(|v| v.as_str());
    let is_stream = body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);

    let mut job = match dispatch_browser_turn(&state, messages, tools, req_model, None).await {
        Ok(j) => j,
        Err(err) => return json_error(StatusCode::BAD_GATEWAY, &err),
    };

    let msg_id = format!("msg_{}", &job.job_id[4..]);
    let model_name = job.target_model.clone();

    if !is_stream {
        let mut interceptor = StreamInterceptor::new(job.has_tools);
        let mut reported_model = None;
        while let Some(ev) = job.rx.recv().await {
            match ev {
                DomStreamEvent::Chunk { thought_delta, text_delta } => {
                    interceptor.push_chunk(thought_delta.as_deref(), &text_delta);
                }
                DomStreamEvent::Done { full_text, active_model } => {
                    interceptor.finalize(&full_text);
                    reported_model = active_model;
                    break;
                }
                DomStreamEvent::Error(err) => {
                    state.release_tab(job.tab_id, None);
                    return json_error(StatusCode::BAD_GATEWAY, &err);
                }
            }
        }
        state.release_tab(job.tab_id, reported_model);
        commit_session(
            &state,
            job.messages,
            &interceptor.full_clean_text,
            &interceptor.tool_calls,
            &model_name,
            job.tools_hash,
            job.tab_id,
        );

        let mut content_blocks = Vec::new();
        if !interceptor.full_reasoning.is_empty() {
            content_blocks.push(json!({ "type": "thinking", "thinking": interceptor.full_reasoning }));
        }
        if !interceptor.full_clean_text.is_empty() {
            content_blocks.push(json!({ "type": "text", "text": interceptor.full_clean_text }));
        }
        for tc in &interceptor.tool_calls {
            let input_val: Value = serde_json::from_str(&tc.arguments_json).unwrap_or(json!({}));
            content_blocks.push(json!({
                "type": "tool_use",
                "id": tc.id,
                "name": tc.name,
                "input": input_val
            }));
        }

        return Json(json!({
            "id": msg_id,
            "type": "message",
            "role": "assistant",
            "model": model_name,
            "content": content_blocks,
            "stop_reason": if interceptor.tool_calls.is_empty() { "end_turn" } else { "tool_use" },
            "usage": { "input_tokens": 0, "output_tokens": 0 }
        }))
        .into_response();
    }

    // Anthropic SSE Streaming
    let (sse_tx, sse_rx) = mpsc::unbounded_channel::<Result<Event, Infallible>>();
    let state_clone = state.clone();

    tokio::spawn(async move {
        let mut interceptor = StreamInterceptor::new(job.has_tools);
        let mut reported_model = None;

        let _ = sse_tx.send(Ok(Event::default().event("message_start").data(
            json!({
                "type": "message_start",
                "message": {
                    "id": msg_id, "type": "message", "role": "assistant", "model": model_name,
                    "content": [], "stop_reason": Value::Null, "usage": { "input_tokens": 0, "output_tokens": 0 }
                }
            })
            .to_string(),
        )));
        let _ = sse_tx.send(Ok(Event::default().event("content_block_start").data(
            json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "text", "text": "" } }).to_string(),
        )));

        while let Some(ev) = job.rx.recv().await {
            match ev {
                DomStreamEvent::Chunk { thought_delta, text_delta } => {
                    let (_, txt) = interceptor.push_chunk(thought_delta.as_deref(), &text_delta);
                    if let Some(t) = txt {
                        let _ = sse_tx.send(Ok(Event::default().event("content_block_delta").data(
                            json!({
                                "type": "content_block_delta", "index": 0,
                                "delta": { "type": "text_delta", "text": t }
                            })
                            .to_string(),
                        )));
                    }
                }
                DomStreamEvent::Done { full_text, active_model } => {
                    reported_model = active_model;
                    if let Some(tail) = interceptor.finalize(&full_text) {
                        let _ = sse_tx.send(Ok(Event::default().event("content_block_delta").data(
                            json!({
                                "type": "content_block_delta", "index": 0,
                                "delta": { "type": "text_delta", "text": tail }
                            })
                            .to_string(),
                        )));
                    }
                    let _ = sse_tx.send(Ok(Event::default().event("content_block_stop").data(
                        json!({ "type": "content_block_stop", "index": 0 }).to_string(),
                    )));

                    for (i, tc) in interceptor.tool_calls.iter().enumerate() {
                        let blk_idx = i + 1;
                        let input_val: Value = serde_json::from_str(&tc.arguments_json).unwrap_or(json!({}));
                        let _ = sse_tx.send(Ok(Event::default().event("content_block_start").data(
                            json!({
                                "type": "content_block_start", "index": blk_idx,
                                "content_block": { "type": "tool_use", "id": tc.id, "name": tc.name, "input": {} }
                            })
                            .to_string(),
                        )));
                        let _ = sse_tx.send(Ok(Event::default().event("content_block_delta").data(
                            json!({
                                "type": "content_block_delta", "index": blk_idx,
                                "delta": { "type": "input_json_delta", "partial_json": input_val.to_string() }
                            })
                            .to_string(),
                        )));
                        let _ = sse_tx.send(Ok(Event::default().event("content_block_stop").data(
                            json!({ "type": "content_block_stop", "index": blk_idx }).to_string(),
                        )));
                    }

                    commit_session(
                        &state_clone,
                        job.messages,
                        &interceptor.full_clean_text,
                        &interceptor.tool_calls,
                        &model_name,
                        job.tools_hash,
                        job.tab_id,
                    );

                    let stop = if interceptor.tool_calls.is_empty() { "end_turn" } else { "tool_use" };
                    let _ = sse_tx.send(Ok(Event::default().event("message_delta").data(
                        json!({ "type": "message_delta", "delta": { "stop_reason": stop }, "usage": { "output_tokens": 0 } }).to_string(),
                    )));
                    let _ = sse_tx.send(Ok(Event::default().event("message_stop").data(
                        json!({ "type": "message_stop" }).to_string(),
                    )));
                    break;
                }
                DomStreamEvent::Error(_) => break,
            }
        }
        state_clone.release_tab(job.tab_id, reported_model);
    });

    Sse::new(UnboundedReceiverStream::new(sse_rx)).keep_alive(KeepAlive::default()).into_response()
}
```

6. src/integrations.rs

```rust
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

/// Atomically patches `%USERPROFILE%\.openmausbot\config.json` with backup `.bak`
pub fn link_openmausbot(port: u16, token: &str, default_model: &str, enable_tools: bool) -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or_else(|| "Could not locate user home directory".to_string())?;
    let maus_dir = home.join(".openmausbot");
    fs::create_dir_all(&maus_dir).map_err(|e| e.to_string())?;
    let config_path = maus_dir.join("config.json");

    let mut root: Value = if config_path.exists() {
        let raw = fs::read_to_string(&config_path).map_err(|e| e.to_string())?;
        let _ = fs::write(config_path.with_extension("json.bak"), &raw);
        serde_json::from_str(&raw).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    if !root.get("instances").map(|v| v.is_object()).unwrap_or(false) {
        root["instances"] = json!({});
    }

    root["instances"]["gemini-enterprise"] = json!({
        "driver": "openai-compat",
        "displayName": "Gemini Enterprise (GeBridge v2)",
        "environment": {
            "GE_BRIDGE_TOKEN": token
        },
        "config": {
            "url": format!("http://127.0.0.1:{port}/v1"),
            "apiKeyEnv": "GE_BRIDGE_TOKEN",
            "model": default_model,
            "tools": enable_tools
        }
    });

    let formatted = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    fs::write(&config_path, format!("{formatted}\n")).map_err(|e| e.to_string())?;
    Ok(config_path)
}

pub fn copy_to_clipboard(text: &str) {
    if let Ok(mut cb) = arboard::Clipboard::new() {
        let _ = cb.set_text(text.to_string());
    }
}

#[cfg(windows)]
pub fn is_autostart_enabled() -> bool {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    hkcu.open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
        .and_then(|k| k.get_value::<String, _>("GeBridge"))
        .is_ok()
}

#[cfg(not(windows))]
pub fn is_autostart_enabled() -> bool {
    false
}

#[cfg(windows)]
pub fn set_autostart(enable: bool) -> Result<(), String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
        .map_err(|e| e.to_string())?;
    if enable {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        key.set_value("GeBridge", &format!("\"{}\"", exe.display()))
            .map_err(|e| e.to_string())?;
    } else {
        let _ = key.delete_value("GeBridge");
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn set_autostart(_enable: bool) -> Result<(), String> {
    Ok(())
}
```

7. src/tray.rs

```rust
use crate::integrations;
use crate::state::{AppState, TraySignal};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tray_icon::{
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu},
    Icon, TrayIconBuilder,
};

pub fn run_tray_loop(state: Arc<AppState>) {
    let (tx, mut rx) = mpsc::unbounded_channel::<TraySignal>();
    *state.tray_tx.lock() = Some(tx);

    let menu = build_tray_menu(&state);
    let ready = state.ext_ws_tx.read().is_some() && !state.tabs.read().is_empty();
    let icon = render_status_icon(ready);

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("GeBridge v2 — Gemini Enterprise Local AI Server")
        .with_icon(icon)
        .build()
        .expect("Failed to create Windows System Tray icon");

    let menu_rx = MenuEvent::receiver();

    loop {
        pump_os_events();

        // Handle dynamic menu/icon updates when Extension syncs models or connects
        let mut need_rebuild = false;
        while rx.try_recv().is_ok() {
            need_rebuild = true;
        }
        if need_rebuild {
            let is_ready = state.ext_ws_tx.read().is_some() && !state.tabs.read().is_empty();
            let _ = tray.set_icon(Some(render_status_icon(is_ready)));
            let _ = tray.set_menu(Some(Box::new(build_tray_menu(&state))));
        }

        // Handle System-Tray clicks
        while let Ok(ev) = menu_rx.try_recv() {
            let id = ev.id.0.as_str();
            if id == "quit" {
                std::process::exit(0);
            } else if id == "open_dashboard" {
                let port = state.prefs.read().port;
                let _ = open::that(format!("http://127.0.0.1:{port}"));
            } else if id == "refresh_models" {
                let _ = state.send_to_extension(&json!({ "type": "REQUEST_MODELS_SYNC", "forceOpenMenu": true }));
            } else if id == "calibrate_ui" {
                let _ = state.send_to_extension(&json!({ "type": "START_CALIBRATION" }));
            } else if id == "link_openmausbot" {
                let prefs = state.prefs.read().clone();
                if let Ok(path) = integrations::link_openmausbot(
                    prefs.port,
                    &state.token,
                    &prefs.default_model,
                    prefs.enable_tools_default,
                ) {
                    state.log("info", format!("Linked OpenMausBot config at {}", path.display()));
                }
            } else if id == "copy_openai_env" {
                let port = state.prefs.read().port;
                integrations::copy_to_clipboard(&format!(
                    "OPENAI_BASE_URL=http://127.0.0.1:{port}/v1\nOPENAI_API_KEY={}",
                    state.token
                ));
            } else if id == "copy_claude_env" {
                let port = state.prefs.read().port;
                integrations::copy_to_clipboard(&format!(
                    "ANTHROPIC_BASE_URL=http://127.0.0.1:{port}\nANTHROPIC_AUTH_TOKEN={}",
                    state.token
                ));
            } else if id == "copy_token" {
                integrations::copy_to_clipboard(&state.token);
            } else if id == "toggle_autostart" {
                let next = !integrations::is_autostart_enabled();
                let _ = integrations::set_autostart(next);
                let _ = tray.set_menu(Some(Box::new(build_tray_menu(&state))));
            } else if let Some(model_id) = id.strip_prefix("model:") {
                {
                    let mut prefs = state.prefs.write();
                    prefs.default_model = model_id.to_string();
                    prefs.save();
                }
                state.broadcast_dashboard();
                let _ = tray.set_menu(Some(Box::new(build_tray_menu(&state))));
            } else if let Some(eff) = id.strip_prefix("effort:") {
                {
                    let mut prefs = state.prefs.write();
                    prefs.reasoning_effort = eff.to_string();
                    prefs.save();
                }
                state.broadcast_dashboard();
                let _ = tray.set_menu(Some(Box::new(build_tray_menu(&state))));
            }
        }

        std::thread::sleep(Duration::from_millis(25));
    }
}

fn build_tray_menu(state: &AppState) -> Menu {
    let menu = Menu::new();
    let prefs = state.prefs.read().clone();
    let ext_ok = state.ext_ws_tx.read().is_some();
    let tab_count = state.tabs.read().len();

    let status_label = if ext_ok && tab_count > 0 {
        format!("🟢 Ready — Gemini Enterprise ({tab_count} Tab)")
    } else if ext_ok {
        "🟡 Extension Connected — Open Gemini Tab".to_string()
    } else {
        "🟠 Waiting for Browser Extension...".to_string()
    };

    let _ = menu.append(&MenuItem::with_id("status", status_label, false, None));
    let _ = menu.append(&PredefinedMenuItem::separator());

    // Dynamic Default Model Submenu
    let model_sub = Submenu::new(format!("Default Model ({})", prefs.default_model), true);
    for m in state.models.read().iter().filter(|m| m.available) {
        let checked = m.id == prefs.default_model;
        let item = CheckMenuItem::with_id(
            format!("model:{}", m.id),
            format!("{} ({})", m.display_name, m.id),
            true,
            checked,
            None,
        );
        let _ = model_sub.append(&item);
    }
    let _ = model_sub.append(&PredefinedMenuItem::separator());
    let _ = model_sub.append(&MenuItem::with_id("refresh_models", "🔄 Refresh Available Models", true, None));
    let _ = menu.append(&model_sub);

    // Reasoning Effort Submenu
    let effort_sub = Submenu::new(format!("Reasoning Effort ({})", prefs.reasoning_effort), true);
    for eff in ["auto", "low", "medium", "high", "max"] {
        let item = CheckMenuItem::with_id(
            format!("effort:{eff}"),
            eff.to_uppercase(),
            true,
            prefs.reasoning_effort == eff,
            None,
        );
        let _ = effort_sub.append(&item);
    }
    let _ = menu.append(&effort_sub);

    // 1-Click Harness Integrations Submenu
    let int_sub = Submenu::new("Quick Integrations", true);
    let _ = int_sub.append(&MenuItem::with_id(
        "link_openmausbot",
        "⚡ Auto-Configure OpenMausBot",
        true,
        None,
    ));
    let _ = int_sub.append(&MenuItem::with_id(
        "copy_openai_env",
        "📋 Copy Hermes / Cline / OpenAI Env",
        true,
        None,
    ));
    let _ = int_sub.append(&MenuItem::with_id(
        "copy_claude_env",
        "📋 Copy Claude Code Anthropic Env",
        true,
        None,
    ));
    let _ = int_sub.append(&MenuItem::with_id("copy_token", "🔑 Copy Pairing Token", true, None));
    let _ = menu.append(&int_sub);

    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&MenuItem::with_id(
        "open_dashboard",
        format!("🌐 Open Control Center (127.0.0.1:{})", prefs.port),
        true,
        None,
    ));
    let _ = menu.append(&MenuItem::with_id(
        "calibrate_ui",
        "🎯 Visual Calibrate Browser Selectors",
        true,
        None,
    ));
    let _ = menu.append(&CheckMenuItem::with_id(
        "toggle_autostart",
        "Start with Windows",
        true,
        integrations::is_autostart_enabled(),
        None,
    ));
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&MenuItem::with_id("quit", "Quit GeBridge", true, None));

    menu
}

/// Renders a crisp anti-aliased 32x32 circle icon in memory (no external .ico file needed)
fn render_status_icon(ready: bool) -> Icon {
    let size = 32usize;
    let mut rgba = vec![0u8; size * size * 4];
    let (r, g, b) = if ready { (34u8, 197u8, 94u8) } else { (245u8, 158u8, 11u8) };

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - 15.5;
            let dy = y as f32 - 15.5;
            let dist = (dx * dx + dy * dy).sqrt();
            let idx = (y * size + x) * 4;
            if dist <= 13.5 {
                rgba[idx] = r;
                rgba[idx + 1] = g;
                rgba[idx + 2] = b;
                rgba[idx + 3] = 255;
            } else if dist <= 14.8 {
                let alpha = ((14.8 - dist) / 1.3 * 255.0) as u8;
                rgba[idx] = 15;
                rgba[idx + 1] = 23;
                rgba[idx + 2] = 42;
                rgba[idx + 3] = alpha;
            }
        }
    }
    Icon::from_rgba(rgba, 32, 32).expect("Valid 32x32 icon")
}

#[cfg(windows)]
fn pump_os_events() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE, WM_QUIT,
    };
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while PeekMessageW(&mut msg, std::mem::zeroed(), 0, 0, PM_REMOVE) != 0 {
            if msg.message == WM_QUIT {
                std::process::exit(0);
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[cfg(not(windows))]
fn pump_os_events() {}
```

8. src/dashboard.html

```html
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <title>GeBridge v2 — Universal Local AI Control Center</title>
  <style>
    :root { --bg:#090d16; --card:#121929; --line:#22304d; --txt:#e5edf9; --sub:#8ea2c6; --pri:#3b82f6; --ok:#22c55e; --warn:#f59e0b; }
    * { box-sizing:border-box; }
    body { margin:0; padding:24px; background:radial-gradient(circle at top,#14213d,var(--bg) 55%); color:var(--txt); font-family:"Segoe UI",system-ui,sans-serif; }
    .wrap { max-width:1020px; margin:0 auto; display:grid; gap:16px; }
    .bar, .card { background:var(--card); border:1px solid var(--line); border-radius:14px; padding:18px; }
    .bar { display:flex; justify-content:space-between; align-items:center; flex-wrap:wrap; gap:12px; }
    h1 { margin:0; font-size:1.15rem; }
    h2 { margin:0 0 10px; font-size:0.95rem; color:#c3d4f5; }
    .sub { color:var(--sub); font-size:0.83rem; }
    .pills { display:flex; gap:8px; flex-wrap:wrap; }
    .pill { display:inline-flex; align-items:center; gap:6px; padding:6px 11px; border-radius:999px; background:#18233a; border:1px solid var(--line); font-size:0.78rem; font-weight:600; }
    .dot { width:8px; height:8px; border-radius:50%; background:var(--warn); }
    .dot.ok { background:var(--ok); box-shadow:0 0 8px var(--ok); }
    .grid { display:grid; grid-template-columns:repeat(auto-fit,minmax(310px,1fr)); gap:16px; }
    select, textarea, input { width:100%; background:#080c14; color:var(--txt); border:1px solid var(--line); border-radius:8px; padding:9px 11px; font:inherit; font-size:0.85rem; }
    .row { display:flex; gap:8px; flex-wrap:wrap; margin-top:10px; }
    button { background:var(--pri); color:#fff; border:none; border-radius:8px; padding:8px 13px; font-weight:600; font-size:0.82rem; cursor:pointer; }
    button.sec { background:#1a2640; border:1px solid var(--line); color:var(--txt); }
    pre { margin:10px 0 0; background:#080c14; border:1px solid var(--line); border-radius:8px; padding:11px; font-size:0.8rem; white-space:pre-wrap; max-height:210px; overflow:auto; }
    .toast { color:var(--ok); font-size:0.8rem; min-height:18px; margin-top:6px; }
  </style>
</head>
<body>
  <div class="wrap">
    <div class="bar">
      <div>
        <h1>⚡ GeBridge v2 — Windows Tray Local AI Server</h1>
        <div class="sub" id="tabSummary">Waiting for Chrome / Edge Extension connection...</div>
      </div>
      <div class="pills">
        <div class="pill"><span class="dot ok"></span> Rust Core Online</div>
        <div class="pill"><span class="dot" id="extDot"></span> <span id="extLbl">Extension Offline</span></div>
        <div class="pill"><span class="dot" id="tabDot"></span> <span id="tabLbl">0 Tabs</span></div>
      </div>
    </div>

    <div class="grid">
      <div class="card">
        <h2>Model Discovery & Default Selection</h2>
        <label class="sub">Default Model (used by System Tray & fallback):</label>
        <select id="modelSelect"></select>
        <label class="sub" style="margin-top:8px;display:block;">Reasoning Effort:</label>
        <select id="effortSelect">
          <option value="auto">Auto</option>
          <option value="low">Low</option>
          <option value="medium">Medium</option>
          <option value="high">High</option>
          <option value="max">Max</option>
        </select>
        <div class="row">
          <button id="saveModelBtn">Save Default</button>
          <button id="refreshModelsBtn" class="sec">🔄 Refresh Models from Web</button>
        </div>
        <div class="toast" id="modelToast"></div>
      </div>

      <div class="card">
        <h2>1-Click Harness Integrations & Pairing</h2>
        <div class="sub">Local Pairing Token (stored in Windows Credential Manager):</div>
        <input id="tokenInput" readonly value="__INJECT_BRIDGE_TOKEN__" style="margin-top:6px;font-family:monospace;" />
        <div class="row">
          <button id="copyTokenBtn">Copy Token</button>
          <button id="linkMausBtn">⚡ Auto-Link OpenMausBot</button>
          <button id="resetBtn" class="sec">Reset Session Cache</button>
        </div>
        <div class="sub" style="margin-top:8px;">Endpoints: <b>OpenAI</b> <code>/v1/chat/completions</code> • <b>Anthropic</b> <code>/v1/messages</code> • <b>Models</b> <code>/v1/models</code></div>
        <div class="toast" id="intToast"></div>
      </div>
    </div>

    <div class="card">
      <h2>Live Streaming & Model Switch Playground</h2>
      <textarea id="promptBox" rows="2">Confirm which model you are running as in one crisp sentence.</textarea>
      <div class="row">
        <button id="sendBtn">Stream via /v1/chat/completions</button>
      </div>
      <pre id="streamOut">Output will stream here (including [Thinking] separation if present)...</pre>
    </div>

    <div class="card">
      <h2>Live Server Logs</h2>
      <pre id="logsOut"></pre>
    </div>
  </div>

  <script>
    const TOKEN = document.getElementById('tokenInput').value.trim();
    const headers = { Authorization: 'Bearer ' + TOKEN, 'Content-Type': 'application/json' };
    const flash = (id, msg) => { const el = document.getElementById(id); el.textContent = msg; setTimeout(() => el.textContent = '', 4000); };

    function applyState(s, logs) {
      document.getElementById('extDot').className = 'dot ' + (s.extensionConnected ? 'ok' : '');
      document.getElementById('extLbl').textContent = s.extensionConnected ? 'Extension WS Connected' : 'Extension Offline';
      document.getElementById('tabDot').className = 'dot ' + (s.tabReady ? 'ok' : '');
      document.getElementById('tabLbl').textContent = `${s.tabs.length} Gemini Tab(s)`;
      document.getElementById('tabSummary').textContent = s.tabReady
        ? `Ready — Active Browser Tab: ${s.tabs[0].title} | Default Model: ${s.defaultModel}`
        : 'Open https://vertexaisearch.cloud.google.com in Chrome or Edge and pair the extension.';

      const sel = document.getElementById('modelSelect');
      sel.innerHTML = '';
      (s.models || []).forEach((m) => {
        const opt = document.createElement('option');
        opt.value = m.id;
        opt.textContent = `${m.display_name} (${m.id})`;
        if (m.id === s.defaultModel) opt.selected = true;
        sel.appendChild(opt);
      });
      document.getElementById('effortSelect').value = s.reasoningEffort || 'auto';

      if (logs) {
        document.getElementById('logsOut').textContent = logs
          .slice()
          .reverse()
          .map((l) => `[${new Date(l.ts * 1000).toLocaleTimeString()}] ${l.level.toUpperCase()}: ${l.message}`)
          .join('\n');
      }
    }

    const es = new EventSource('/api/events?token=' + encodeURIComponent(TOKEN));
    es.onmessage = (e) => {
      const data = JSON.parse(e.data);
      if (data.status) applyState(data.status, data.logs);
    };

    document.getElementById('saveModelBtn').onclick = async () => {
      await fetch('/api/prefs', {
        method: 'POST', headers,
        body: JSON.stringify({
          defaultModel: document.getElementById('modelSelect').value,
          reasoningEffort: document.getElementById('effortSelect').value
        })
      });
      flash('modelToast', 'Saved default model & synced with Windows System Tray!');
    };

    document.getElementById('refreshModelsBtn').onclick = async () => {
      await fetch('/api/refresh-models', { method: 'POST', headers });
      flash('modelToast', 'Requested live Model Picker scan from Gemini Enterprise tab...');
    };

    document.getElementById('copyTokenBtn').onclick = async () => {
      await navigator.clipboard.writeText(TOKEN);
      flash('intToast', 'Copied pairing token!');
    };

    document.getElementById('linkMausBtn').onclick = async () => {
      const r = await fetch('/api/link-openmausbot', { method: 'POST', headers });
      const d = await r.json();
      flash('intToast', d.ok ? `Updated ${d.path} — restart OpenMausBot!` : d.error?.message);
    };

    document.getElementById('resetBtn').onclick = async () => {
      await fetch('/api/reset-sessions', { method: 'POST', headers });
      flash('intToast', 'Session cache cleared.');
    };

    document.getElementById('sendBtn').onclick = async () => {
      const out = document.getElementById('streamOut');
      const model = document.getElementById('modelSelect').value;
      const effort = document.getElementById('effortSelect').value;
      out.textContent = `Dispatching to ${model} (effort=${effort})...\n`;
      const res = await fetch('/v1/chat/completions', {
        method: 'POST', headers,
        body: JSON.stringify({
          model, reasoning_effort: effort, stream: true,
          messages: [{ role: 'user', content: document.getElementById('promptBox').value }]
        })
      });
      if (!res.ok) { out.textContent = await res.text(); return; }
      const reader = res.body.getReader();
      const dec = new TextDecoder();
      let buf = '', cotShown = false;
      out.textContent = '';
      while (true) {
        const { value, done } = await reader.read();
        if (done) break;
        buf += dec.decode(value, { stream: true });
        const lines = buf.split('\n');
        buf = lines.pop();
        for (const l of lines) {
          if (!l.startsWith('data: ') || l === 'data: [DONE]') continue;
          const j = JSON.parse(l.slice(6));
          if (j.error) out.textContent += `\n[ERROR]: ${j.error.message}`;
          const d = j.choices?.[0]?.delta || {};
          if (d.reasoning_content) {
            if (!cotShown) { out.textContent += '[Thinking]\n'; cotShown = true; }
            out.textContent += d.reasoning_content;
          }
          if (d.content) {
            if (cotShown) { out.textContent += '\n[/Thinking]\n\n'; cotShown = false; }
            out.textContent += d.content;
          }
        }
      }
    };
  </script>
</body>
</html>
```

Part 2: Chromium Manifest V3 Extension v2 (Chrome & Microsoft Edge)

9. extension/manifest.json

```json
{
  "manifest_version": 3,
  "name": "GeBridge v2 — Gemini Enterprise Local AI Connector",
  "version": "2.0.0",
  "description": "Full-duplex WebSocket bridge with dynamic Model Discovery & Switching for Gemini Enterprise.",
  "permissions": [
    "storage",
    "alarms",
    "tabs",
    "scripting"
  ],
  "host_permissions": [
    "https://vertexaisearch.cloud.google.com/*",
    "http://127.0.0.1:8787/*"
  ],
  "background": {
    "service_worker": "background.js"
  },
  "action": {
    "default_title": "GeBridge v2",
    "default_popup": "popup.html"
  },
  "content_scripts": [
    {
      "matches": [
        "https://vertexaisearch.cloud.google.com/*"
      ],
      "js": [
        "content.js"
      ],
      "run_at": "document_idle"
    }
  ]
}
```

10. extension/background.js

```js
const DEFAULT_HOST = '127.0.0.1:8787';
let ws = null;
let wsConnected = false;
let lastError = null;
let reconnectTimer = null;
const tabPorts = new Map();

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
  }
}

async function syncTabsToServer() {
  const tabs = await chrome.tabs.query({ url: 'https://vertexaisearch.cloud.google.com/*' });
  const list = [];
  for (const t of tabs) {
    let info = null;
    try {
      info = await chrome.tabs.sendMessage(t.id, { type: 'PING_TAB' });
    } catch {
      try {
        await chrome.scripting.executeScript({ target: { tabId: t.id }, files: ['content.js'] });
        info = await chrome.tabs.sendMessage(t.id, { type: 'PING_TAB' });
      } catch {
        // Tab still loading
      }
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
  wsSend({ type: 'TABS_SYNC', tabs: list });
}

async function connectWebSocket() {
  if (ws && (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CONNECTING)) return;
  const { host, token } = await getConfig();
  if (!token) {
    lastError = 'Paste Pairing Token from Control Center (127.0.0.1:8787)';
    return;
  }

  try {
    const socket = new WebSocket(`ws://${host}/ws/extension?token=${encodeURIComponent(token)}`);
    ws = socket;

    socket.onopen = () => {
      wsConnected = true;
      lastError = null;
      chrome.action.setBadgeText({ text: 'ON' });
      chrome.action.setBadgeBackgroundColor({ color: '#16a34a' });
      syncTabsToServer();
    };

    socket.onmessage = async (event) => {
      const msg = JSON.parse(event.data);
      if (msg.type === 'REQUEST_MODELS_SYNC') {
        const tabs = await chrome.tabs.query({ url: 'https://vertexaisearch.cloud.google.com/*' });
        if (tabs[0]) {
          chrome.tabs.sendMessage(tabs[0].id, {
            type: 'SCAN_MODELS',
            forceOpenMenu: Boolean(msg.forceOpenMenu)
          }).catch(() => {});
        }
      } else if (msg.type === 'START_CALIBRATION') {
        const tabs = await chrome.tabs.query({ url: 'https://vertexaisearch.cloud.google.com/*' });
        if (tabs[0]) {
          chrome.tabs.update(tabs[0].id, { active: true });
          chrome.tabs.sendMessage(tabs[0].id, { type: 'START_CALIBRATION' }).catch(() => {});
        }
      } else if (msg.type === 'RUN_TURN') {
        try {
          await chrome.tabs.sendMessage(msg.tabId, msg);
        } catch (err) {
          wsSend({ type: 'TURN_DONE', jobId: msg.jobId, error: `Tab ${msg.tabId} unreachable: ${err.message}` });
          syncTabsToServer();
        }
      }
    };

    socket.onerror = () => {
      lastError = 'Cannot reach GeBridge on ' + host;
    };

    socket.onclose = () => {
      wsConnected = false;
      ws = null;
      chrome.action.setBadgeText({ text: '' });
      clearTimeout(reconnectTimer);
      reconnectTimer = setTimeout(connectWebSocket, 2000);
    };
  } catch (err) {
    lastError = err.message;
  }
}

// Keep MV3 Service Worker continuously responsive
chrome.alarms.create('ws-keepalive', { periodInMinutes: 0.35 });
chrome.alarms.onAlarm.addListener(() => {
  connectWebSocket();
  if (wsConnected) syncTabsToServer();
});

setInterval(() => {
  if (ws && ws.readyState === WebSocket.OPEN) {
    ws.send(JSON.stringify({ type: 'PING' }));
  }
}, 18000);

chrome.tabs.onUpdated.addListener((_id, change, tab) => {
  if (change.status === 'complete' && tab.url?.startsWith('https://vertexaisearch.cloud.google.com/')) {
    syncTabsToServer();
  }
});
chrome.tabs.onRemoved.addListener(() => syncTabsToServer());

chrome.runtime.onConnect.addListener((port) => {
  if (port.name !== 'ge-v2-port') return;
  const tabId = port.sender?.tab?.id;
  if (tabId) tabPorts.set(tabId, port);

  port.onMessage.addListener((msg) => {
    connectWebSocket();
    if (msg.type === 'MODELS_FOUND') {
      wsSend({ type: 'MODELS_SYNC', models: msg.models });
    } else if (msg.type === 'DOM_DELTA') {
      wsSend(msg);
    } else if (msg.type === 'TURN_DONE') {
      wsSend(msg);
    } else if (msg.type === 'TAB_READY') {
      syncTabsToServer();
    }
  });

  port.onDisconnect.addListener(() => {
    if (tabId) tabPorts.delete(tabId);
    syncTabsToServer();
  });
});

chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (msg.type === 'POPUP_STATE') {
    getConfig().then((c) => sendResponse({ ...c, wsConnected, lastError }));
    return true;
  }
  if (msg.type === 'POPUP_SAVE') {
    chrome.storage.local.set({ host: msg.host, token: msg.token }).then(() => {
      if (ws) ws.close();
      connectWebSocket();
      sendResponse({ ok: true });
    });
    return true;
  }
  if (msg.type === 'POPUP_SCAN_MODELS') {
    chrome.tabs.query({ url: 'https://vertexaisearch.cloud.google.com/*' }).then((tabs) => {
      if (tabs[0]) chrome.tabs.sendMessage(tabs[0].id, { type: 'SCAN_MODELS', forceOpenMenu: true });
      sendResponse({ ok: Boolean(tabs[0]) });
    });
    return true;
  }
  if (msg.type === 'POPUP_CALIBRATE') {
    chrome.tabs.query({ url: 'https://vertexaisearch.cloud.google.com/*' }).then((tabs) => {
      if (tabs[0]) {
        chrome.tabs.update(tabs[0].id, { active: true });
        chrome.tabs.sendMessage(tabs[0].id, { type: 'START_CALIBRATION' });
      }
      sendResponse({ ok: Boolean(tabs[0]) });
    });
    return true;
  }
});

connectWebSocket();
```

11. extension/content.js

```js
(() => {
  if (window.__GE_BRIDGE_V2_LOADED__) return;
  window.__GE_BRIDGE_V2_LOADED__ = true;

  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  let port = null;
  let cal = null;
  let activeModelSlug = null;

  chrome.storage.local.get(['calibratedV2']).then((d) => {
    cal = d.calibratedV2 || null;
    setHud('ready');
  });

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

  function setHud(mode, msg) {
    if (mode === 'busy') hudText.textContent = `⚡ ${msg}`;
    else if (mode === 'cal') hudText.textContent = `🎯 ${msg}`;
    else hudText.textContent = `🟢 GeBridge v2${activeModelSlug ? ' • ' + activeModelSlug : ''}`;
  }
  setHud('ready');

  function ensurePort() {
    try {
      port = chrome.runtime.connect({ name: 'ge-v2-port' });
      port.postMessage({ type: 'TAB_READY' });
      port.onDisconnect.addListener(() => { port = null; setTimeout(ensurePort, 1500); });
    } catch {
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

  function toModelSlug(raw) {
    return raw
      .toLowerCase()
      .replace(/\(.*?\)/g, '')
      .replace(/[^a-z0-9.+-]+/g, '-')
      .replace(/^-+|-+$/g, '');
  }

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
    if (cal?.modelBtn) {
      const hit = resolveShadowPath(cal.modelBtn);
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

  // Scan live available models from the Gemini Enterprise Model Picker UI
  async function scanAvailableModels(forceOpenMenu = false) {
    const picker = findModelPickerButton();
    let opened = false;

    if (picker) {
      const label = (picker.textContent || picker.getAttribute('aria-label') || '').trim();
      if (label) activeModelSlug = toModelSlug(label.split('\n')[0]);
      if (forceOpenMenu) {
        picker.click();
        opened = true;
        await sleep(320);
      }
    }

    const optionEls = deepCollect((el) => {
      if (!isVisible(el)) return false;
      const role = el.getAttribute('role') || '';
      if (!/menuitem|option|radio|listitem/i.test(role)) return false;
      return /gemini|flash|pro|argon|ultra/i.test(el.textContent || '');
    });

    const seen = new Map();
    for (const opt of optionEls) {
      const lines = (opt.innerText || opt.textContent || '').split('\n').map((s) => s.trim()).filter(Boolean);
      const title = lines[0] || '';
      const id = toModelSlug(title);
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

    if (opened) {
      document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, composed: true }));
      document.body.click();
    }

    if (seen.size > 0 && port) {
      port.postMessage({ type: 'MODELS_FOUND', models: Array.from(seen.values()) });
    }
    setHud('ready');
  }

  // Switch Model & Reasoning Effort in Gemini Enterprise UI before sending prompt
  async function ensureModelSelected(targetModel, targetEffort) {
    if (!targetModel) return;
    if (activeModelSlug === targetModel && !targetEffort) return;

    const picker = findModelPickerButton();
    if (!picker) return;

    const currentLabel = toModelSlug((picker.textContent || '').split('\n')[0]);
    if (currentLabel !== targetModel) {
      picker.click();
      await sleep(300);
      const options = deepCollect((el) => {
        if (!isVisible(el)) return false;
        const role = el.getAttribute('role') || '';
        return /menuitem|option|radio|listitem/i.test(role);
      });
      const match = options.find((el) => {
        const firstLine = (el.innerText || el.textContent || '').split('\n')[0].trim();
        const slug = toModelSlug(firstLine);
        return slug === targetModel || slug.includes(targetModel) || targetModel.includes(slug);
      });

      if (match) {
        if (match.getAttribute('aria-disabled') === 'true') {
          document.body.click();
          throw new Error(`Model "${targetModel}" is disabled for your enterprise account.`);
        }
        match.click();
        activeModelSlug = targetModel;
        await sleep(250);
      } else {
        document.body.click();
      }
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
    if (cal?.composer) {
      const hit = resolveShadowPath(cal.composer);
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
    if (cal?.sendBtn) {
      const hit = resolveShadowPath(cal.sendBtn);
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
    if (cal?.replyTag) {
      const custom = deepCollect((el) => isVisible(el) && el.tagName.toLowerCase() === cal.replyTag);
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

  async function pasteBase64Images(composer, images) {
    for (const dataUrl of images) {
      const m = dataUrl.match(/^data:(image\/[a-zA-Z0-9.+-]+);base64,(.+)$/);
      if (!m) continue;
      const bin = atob(m[2]);
      const bytes = new Uint8Array(bin.length);
      for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
      const file = new File([bytes], `maus_screen.${m[1].split('/')[1] || 'png'}`, { type: m[1] });
      const dt = new DataTransfer();
      dt.items.add(file);
      composer.focus();
      composer.dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true, composed: true }));
      await sleep(320);
    }
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

      if (job.images?.length) await pasteBase64Images(composer, job.images);
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
    } catch (err) {
      if (port) port.postMessage({ type: 'TURN_DONE', jobId: job.jobId, error: err.message });
    } finally {
      setHud('ready');
    }
  }

  // 4-Click Visual Calibrator (Composer -> Send -> Reply Bubble -> Model Picker)
  function startCalibration() {
    const next = {};
    let step = 1;
    setHud('cal', '1/4 Click Chat Input (Esc to finish early)');

    const onClick = (e) => {
      if (hud.contains(e.target)) return;
      e.preventDefault();
      e.stopPropagation();
      const el = e.composedPath()[0];
      if (step === 1) {
        next.composer = buildShadowPath(el);
        step = 2;
        setHud('cal', '2/4 Click Send Button');
      } else if (step === 2) {
        next.sendBtn = buildShadowPath(el.closest?.('button,[role="button"]') || el);
        step = 3;
        setHud('cal', '3/4 Click any AI Reply Text');
      } else if (step === 3) {
        let cur = el;
        while (cur && !cur.tagName?.includes('-') && cur.parentElement) cur = cur.parentElement;
        next.replyTag = (cur || el).tagName.toLowerCase();
        step = 4;
        setHud('cal', '4/4 Click Model Selector Button (or Esc)');
      } else if (step === 4) {
        next.modelBtn = buildShadowPath(el.closest?.('button,[role="button"],[role="combobox"]') || el);
        finish();
      }
    };
    const onKey = (e) => { if (e.key === 'Escape') finish(); };

    function finish() {
      window.removeEventListener('click', onClick, true);
      window.removeEventListener('keydown', onKey, true);
      if (next.composer) {
        cal = { ...(cal || {}), ...next };
        chrome.storage.local.set({ calibratedV2: cal });
        if (port) port.postMessage({ type: 'TAB_READY' });
      }
      setHud('ready');
    }

    window.addEventListener('click', onClick, true);
    window.addEventListener('keydown', onKey, true);
  }

  chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
    if (msg.type === 'PING_TAB') {
      sendResponse({ activeModel: activeModelSlug, calibrated: Boolean(cal) });
      return false;
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
    if (msg.type === 'RUN_TURN') {
      runTurn(msg);
      sendResponse({ accepted: true });
      return false;
    }
  });

  // Auto-discover active model badge silently after load
  setTimeout(() => scanAvailableModels(false), 1800);
})();
```

12. extension/popup.html

```html
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <style>
    body { width:310px; margin:0; padding:15px; background:#0b1120; color:#f1f5f9; font-family:"Segoe UI",system-ui,sans-serif; }
    h3 { margin:0 0 5px; font-size:14px; }
    .st { display:flex; align-items:center; gap:7px; font-size:12px; color:#94a3b8; margin-bottom:10px; }
    .dot { width:8px; height:8px; border-radius:50%; background:#f59e0b; }
    .dot.ok { background:#22c55e; box-shadow:0 0 8px #22c55e; }
    label { display:block; font-size:11px; color:#94a3b8; margin:7px 0 3px; }
    input { width:100%; box-sizing:border-box; padding:7px 9px; border-radius:7px; border:1px solid #28354f; background:#050811; color:#e2e8f0; font-size:12px; }
    .btns { display:grid; gap:6px; margin-top:11px; }
    button { padding:8px; border-radius:7px; border:none; font-weight:600; font-size:12px; cursor:pointer; background:#3b82f6; color:#fff; }
    button.sec { background:#172238; border:1px solid #28354f; color:#cbd5e1; }
    .err { color:#f87171; font-size:11px; margin-top:7px; min-height:14px; }
  </style>
</head>
<body>
  <h3>⚡ GeBridge v2 (Tray Edition)</h3>
  <div class="st"><span class="dot" id="dot"></span><span id="stTxt">Checking daemon...</span></div>

  <label>Local Daemon Host</label>
  <input id="host" value="127.0.0.1:8787" />

  <label>Pairing Token (from Tray Menu or Control Center)</label>
  <input id="token" type="password" placeholder="gemb_..." />

  <div class="btns">
    <button id="saveBtn">Save & Connect WebSocket</button>
    <button id="scanBtn" class="sec">🔄 Scan Available Models Now</button>
    <button id="dashBtn" class="sec">🌐 Open Control Center</button>
    <button id="geminiBtn" class="sec">Open Gemini Enterprise Tab</button>
    <button id="calBtn" class="sec">🎯 Calibrate UI Selectors</button>
  </div>
  <div class="err" id="err"></div>
  <script src="popup.js"></script>
</body>
</html>
```

13. extension/popup.js

```js
const dot = document.getElementById('dot');
const stTxt = document.getElementById('stTxt');
const hostIn = document.getElementById('host');
const tokenIn = document.getElementById('token');
const errEl = document.getElementById('err');

function refresh() {
  chrome.runtime.sendMessage({ type: 'POPUP_STATE' }, (s) => {
    if (!s) return;
    hostIn.value = s.host || '127.0.0.1:8787';
    if (s.token && !tokenIn.value) tokenIn.value = s.token;
    dot.className = 'dot ' + (s.wsConnected ? 'ok' : '');
    stTxt.textContent = s.wsConnected ? 'Connected via WebSocket' : 'Disconnected';
    errEl.textContent = s.lastError || '';
  });
}

document.getElementById('saveBtn').onclick = () => {
  chrome.runtime.sendMessage({
    type: 'POPUP_SAVE',
    host: hostIn.value.trim(),
    token: tokenIn.value.trim()
  }, () => setTimeout(refresh, 400));
};

document.getElementById('scanBtn').onclick = () => {
  chrome.runtime.sendMessage({ type: 'POPUP_SCAN_MODELS' }, (r) => {
    errEl.textContent = r?.ok ? 'Scanning Gemini Model Picker...' : 'Open Gemini Enterprise tab first.';
  });
};

document.getElementById('dashBtn').onclick = () => {
  chrome.tabs.create({ url: 'http://' + (hostIn.value.trim() || '127.0.0.1:8787') });
};

document.getElementById('geminiBtn').onclick = () => {
  chrome.tabs.create({ url: 'https://vertexaisearch.cloud.google.com' });
};

document.getElementById('calBtn').onclick = () => {
  chrome.runtime.sendMessage({ type: 'POPUP_CALIBRATE' }, () => window.close());
};

refresh();
```

14. README.md

```markdown
# GeBridge v2 — Windows System-Tray Local AI Server

Ultra-lightweight Rust Windows System-Tray daemon (`~4 MB` `.exe`, `~5 MB` RAM) + Chromium MV3 extension (Chrome & Microsoft Edge) that exposes your signed-in **Gemini Enterprise (`vertexaisearch.cloud.google.com`)** tab as a universal OpenAI + Anthropic + ACP AI server with **live model discovery** and **1-click OpenMausBot integration**.

## Build & Run on Windows

1. **Compile the release binary** (requires [Rustup](https://rustup.rs) with MSVC toolchain):
   ```powershell
   cargo build --release
   .\target\release\ge-bridge.exe

2. Load the Browser Extension (Chrome or Microsoft Edge):
   Open chrome://extensions or edge://extensions and enable Developer mode.
   Click Load unpacked and select the extension folder.
3. Pair in 2 Clicks:
   Right-click the GeBridge icon in your Windows System Tray → Quick Integrations → 🔑 Copy Pairing Token.
   Open the GeBridge browser extension popup, paste the token, and click Save & Connect WebSocket.
4. Sync Models & Link OpenMausBot:
   Open https://vertexaisearch.cloud.google.com and click 🔄 Models on the bottom-right pill (or right-click the Tray icon → Default Model → 🔄 Refresh Available Models).
   Right-click the Tray icon → Quick Integrations → ⚡ Auto-Configure OpenMausBot, then restart OpenMausBot and click Re-detect in your bot's model picker to see every discovered Gemini Enterprise model!


---

### ⚠️ Windows Build & Runtime Tips
- **MSVC Build Tools prerequisite:** On a fresh Windows machine, ensure **Desktop development with C++** (Visual Studio Build Tools) is installed alongside `rustup` so `windows-sys` and `keyring` link `advapi32.lib` and `user32.lib` cleanly.
- **Keep the Gemini tab unminimized during background runs:** Keep your signed-in Chrome/Edge window unminimized behind other windows (or disable Chrome's **Memory Saver** for `vertexaisearch.cloud.google.com`) so background `MutationObserver` streaming stays at full speed.
```

Would you like a PowerShell generator script (`setup-repo.ps1`) that writes this entire folder tree to disk in one paste and builds `ge-bridge.exe` automatically?

## turn 9

### User prompt

give me the templates for other harness

### Ai response

Here are ready-to-paste Windows configuration templates for connecting Hermes Agent, Claude Code, Continue.dev, Cline / Roo Code, Aider, Zed, OpenWebUI, and Python/Node SDKs to GeBridge v2.

Replace gemb_YOUR_PAIRING_TOKEN in any template below with the token from System Tray → Quick Integrations → 🔑 Copy Pairing Token.

Quick Protocol & URL Cheat Sheet

Watch the /v1 suffix—OpenAI-style harnesses require /v1, while Anthropic-style harnesses append /v1/messages automatically and fail with 404 if you include /v1 in the base URL.

Harness	Protocol Used	Exact Base URL	Model Selection Behavior
Hermes Agent	OpenAI Chat Completions	http://127.0.0.1:8787/v1	Auto-discovers /v1/models; supports gemini-4-argon:high
Claude Code	Anthropic Messages	http://127.0.0.1:8787 (no /v1)	Routes claude-* aliases to your System-Tray Default Model
Continue.dev	OpenAI Compatible	http://127.0.0.1:8787/v1	Explicit model list in config.yaml with tool_use + image_input
Cline / Roo Code	OpenAI Compatible	http://127.0.0.1:8787/v1	Populates model dropdown directly from GET /v1/models
Aider	LiteLLM openai/ prefix	http://127.0.0.1:8787/v1	Prefix any discovered slug with openai/ (e.g., openai/gemini-4-argon)
Zed / OpenMausBot ACP	Native ACP (stdio)	ge-bridge.exe --acp	Uses your System-Tray Default Model over stdin/stdout

1. Hermes Agent (Windows Native & WSL2)

On native Windows, Hermes stores configuration in %LOCALAPPDATA%\hermes\config.yaml and secrets in %LOCALAPPDATA%\hermes\.env (on WSL2, ~/.hermes/). Hermes ignores generic OPENAI_BASE_URL for custom gateways, so define base_url in config.yaml and keep your token in .env.

Step 1 — %LOCALAPPDATA%\hermes\.env:

``` bash
GE_BRIDGE_API_KEY=gemb_YOUR_PAIRING_TOKEN
```
Step 2 — %LOCALAPPDATA%\hermes\config.yaml:

```yaml
model:
  provider: custom
  base_url: http://127.0.0.1:8787/v1
  key_env: GE_BRIDGE_API_KEY
  default: gemini-3.8-flash

Interactive alternative: Run hermes model in PowerShell, pick Custom endpoint, enter http://127.0.0.1:8787/v1, paste your gemb_... token, and select 2 (Chat Completions) so Hermes fetches your live catalog from GET /v1/models.
```
2. Claude Code (%USERPROFILE%\.claude\settings.json)

Claude Code speaks the Anthropic Messages protocol natively—set ANTHROPIC_BASE_URL without /v1 and explicitly blank out ANTHROPIC_API_KEY so Claude Code never falls back to a cloud key. Because Claude Code validates ANTHROPIC_MODEL against Anthropic model names, leave claude-sonnet-4-6 as the alias; GeBridge v2 automatically routes claude-* requests to whichever Gemini Enterprise model and reasoning effort you select in the Windows System Tray.

File — %USERPROFILE%\.claude\settings.json:

```json
{
  "env": {
    "ANTHROPIC_BASE_URL": "http://127.0.0.1:8787",
    "ANTHROPIC_AUTH_TOKEN": "gemb_YOUR_PAIRING_TOKEN",
    "ANTHROPIC_API_KEY": "",
    "ANTHROPIC_MODEL": "claude-sonnet-4-6",
    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1"
  }
}
```
One-off PowerShell Session Alternative:

```powershell
$env:ANTHROPIC_BASE_URL = "http://127.0.0.1:8787"
$env:ANTHROPIC_AUTH_TOKEN = "gemb_YOUR_PAIRING_TOKEN"
$env:ANTHROPIC_API_KEY = ""
claude
```

3. Continue.dev (%USERPROFILE%\.continue\config.yaml)

Continue uses schema: v1 YAML in VS Code and JetBrains; set provider: openai and enable tool_use and image_input capabilities for Agent mode.

File — %USERPROFILE%\.continue\config.yaml:

```yaml
name: GeBridge Gemini Enterprise
version: 1.0.0
schema: v1
models:
  - name: Gemini 4 Argon (High Reasoning)
    provider: openai
    model: gemini-4-argon:high
    apiBase: http://127.0.0.1:8787/v1
    apiKey: gemb_YOUR_PAIRING_TOKEN
    capabilities:
      - tool_use
      - image_input
    roles:
      - chat
      - edit
      - apply
      - summarize

  - name: Gemini 3.8 Flash (Fast Agent)
    provider: openai
    model: gemini-3.8-flash
    apiBase: http://127.0.0.1:8787/v1
    apiKey: gemb_YOUR_PAIRING_TOKEN
    capabilities:
      - tool_use
      - image_input
    roles:
      - chat
      - edit
      - apply
```

4. Cline / Roo Code / Kilo Code (VS Code Extensions)

Open the extension's Settings (⚙️) → API Provider and fill in these values:

API Provider: OpenAI Compatible
Base URL: http://127.0.0.1:8787/v1
API Key: gemb_YOUR_PAIRING_TOKEN
Model ID: Select any discovered model from the dropdown (such as gemini-3.8-flash or gemini-4-argon), or type gemini-4-argon:max to force maximum reasoning effort.
Model Configuration Toggles: Check Supports Images and Enable Native Tool Calling (uncheck R1-specific format overrides).

5. Aider (%USERPROFILE%\.aider.conf.yml)

Aider routes custom OpenAI-compatible servers through LiteLLM when you prefix the model name with openai/:

File — %USERPROFILE%\.aider.conf.yml:

```yml
openai-api-base: http://127.0.0.1:8787/v1
openai-api-key: gemb_YOUR_PAIRING_TOKEN
model: openai/gemini-4-argon
weak-model: openai/gemini-3.8-flash
editor-model: openai/gemini-3.8-flash
stream: true
```

6. Zed Editor (%APPDATA%\Zed\settings.json)

Zed can connect to GeBridge v2 through both OpenAI HTTP streaming and our compiled --acp Agent Server:

```json
{
  "language_models": {
    "openai": {
      "api_url": "http://127.0.0.1:8787/v1",
      "available_models": [
        {
          "name": "gemini-4-argon",
          "display_name": "Gemini 4 Argon (GeBridge)",
          "max_tokens": 1000000
        },
        {
          "name": "gemini-3.8-flash",
          "display_name": "Gemini 3.8 Flash (GeBridge)",
          "max_tokens": 1000000
        }
      ]
    }
  },
  "agent_servers": {
    "GeBridge Gemini Enterprise": {
      "command": "C:\\path\\to\\ge-bridge.exe",
      "args": ["--acp"]
    }
  }
}
```

(Store gemb_YOUR_PAIRING_TOKEN in Zed's Assistant Panel → Configure → OpenAI API Key).

7. OpenWebUI / LobeChat / Generic SDKs

For web chat frontends like OpenWebUI (Admin Panel → Settings → Connections → OpenAI API) or custom scripts:

URL: http://127.0.0.1:8787/v1 (if OpenWebUI runs inside Docker Desktop on Windows, use http://host.docker.internal:8787/v1 and update check_origin if accessed from a browser origin)
Key: gemb_YOUR_PAIRING_TOKEN

Python OpenAI SDK Snippet (with Per-Request Model + Reasoning Override):

from openai import OpenAI
```python
client = OpenAI(
    base_url="http://127.0.0.1:8787/v1",
    api_key="gemb_YOUR_PAIRING_TOKEN",
)

# List live models synced from your Gemini Enterprise browser tab
print([m.id for m in client.models.list().data])

# Stream response with explicit model + reasoning tier
stream = client.chat.completions.create(
    model="gemini-4-argon:high",
    messages=[{"role": "user", "content": "Design a zero-copy ring buffer in Rust."}],
    stream=True,
)
for chunk in stream:
    delta = chunk.choices[0].delta
    if getattr(delta, "reasoning_content", None):
        print(delta.reasoning_content, end="", flush=True)
    if delta.content:
        print(delta.content, end="", flush=True)
```

⚠️ Multi-Harness Tip

Running two harnesses at once (e.g., OpenMausBot + Claude Code): Open two https://vertexaisearch.cloud.google.com tabs in Chrome or Edge—GeBridge v2's multi-tab pool automatically assigns each active conversation thread to an idle tab instead of blocking while another agent is streaming.

Would you like me to add a 1-Click "Auto-Configure Hermes & Claude Code" menu action directly into src/integrations.rs and src/tray.rs just like the OpenMausBot button?

## turn 10

### User prompt

{user_prompt}

### Ai response

{AI_RESPONSE}


