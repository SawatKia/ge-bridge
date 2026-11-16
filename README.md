# ge-bridge

ge-bridge is a Windows system-tray app written in Rust. It runs a local server on `127.0.0.1:8787` with OpenAI-compatible (`/v1/chat/completions`, `/v1/models`) and Anthropic-compatible (`/v1/messages`) endpoints. Each chat turn is forwarded over a WebSocket to a Chromium MV3 extension (in `extension/`). The extension sends the prompt to Gemini Enterprise with your signed-in browser session (`vertexaisearch.cloud.google`) and streams the answer back. OpenMausBot has a one-click setup. Any OpenAI- or Anthropic-compatible harness can connect.

Check your organization's IT policy before automating the web UI of an enterprise tenant.

## How it fits together

ge-bridge has four parts:

- **ge-bridge.exe** is the tray app. It runs the server on `127.0.0.1:8787`.
- **The extension** carries every request into the browser.
- **A signed-in Gemini Enterprise tab** is where the model actually runs.
- **ge-bridge-cli.exe** is a console tool. Its `acp` mode is used by OpenMausBot.

OpenMausBot runs its bots on the ACP engine and draws pictures through the images API. Other harnesses, such as Claude Code, Cline or Hermes, call the HTTP API directly.

```mermaid
flowchart LR
  subgraph OMB["OpenMausBot"]
    bot["Bot (chat thread)"]
    imgtool["generate_image tool"]
    mcp["MCP servers<br/>agents, browser, computer, ..."]
  end

  subgraph PC["Your PC"]
    cli["ge-bridge-cli acp<br/>ACP agent: tool loop,<br/>file + command tools,<br/>MCP client"]
    folder[("Bot working folder<br/>cwd")]
    server["ge-bridge.exe<br/>tray + server :8787<br/>/v1/chat/completions<br/>/v1/messages<br/>/v1/images/generations"]
  end

  subgraph Browser["Chrome / Edge"]
    ext["Extension<br/>service worker"]
    tab["Gemini Enterprise tab<br/>signed in"]
  end

  gemini(["Gemini Enterprise<br/>vertexaisearch.cloud.google"])

  other["Other harnesses<br/>Claude Code, Cline, Hermes"] -- "HTTP + token" --> server
  imgtool -- "HTTP images API" --> server
  bot -- "customAcp engine<br/>ACP over stdio" --> cli
  cli -- "spawns, stdio" --> mcp
  cli -- "read / write / run<br/>after approval" --> folder
  cli -- "HTTP /v1/chat/completions" --> server
  server -- "WebSocket /ws/extension" --> ext
  ext -- "runs the turn in the page<br/>API or DOM" --> tab
  tab -- "your session cookies" --> gemini
```

**One chat turn.** Both ways reach Gemini the same way.

1. The harness or `ge-bridge-cli acp` sends `/v1/chat/completions`. If tools are given, they are written into the prompt.
2. The server sends the prompt to the extension over the WebSocket, along with the model and whether to start a new chat.
3. The extension waits for the human pacing delay. Then it runs the turn inside the Gemini tab, using Gemini's internal API from the page or by typing into the page. It streams thoughts and text back.
4. The server turns `<tool_call>{…}</tool_call>` in the answer into real tool calls. It also handles refusals with one retry in a new chat.
5. The answer streams back to the caller.

**Images** use `/v1/images/generations`. The extension turns on Gemini's image tool, downloads the picture, and returns it as base64. OpenMausBot attaches it to the reply, and ge-bridge also saves a copy in the bot's folder.

```mermaid
sequenceDiagram
  participant O as OpenMausBot
  participant A as ge-bridge-cli acp
  participant S as ge-bridge.exe
  participant E as Extension + Gemini tab
  O->>A: session/new (cwd, mcpServers)
  A->>A: start MCP servers
  O->>A: session/prompt "fix the failing test"
  loop until the model answers without a tool call (max 30 steps)
    A->>S: /v1/chat/completions (history + tools, stream)
    S->>E: RUN_TURN over WebSocket
    E-->>S: thoughts + text (with tool_call)
    S-->>A: SSE chunks + tool_calls
    A-->>O: session/update (thought, text, tool_call)
    opt write_file / edit_file / run_command
      A->>O: session/request_permission
      O-->>A: allow once / always / reject (approval card)
    end
    A->>A: run the tool in cwd, or call the MCP server
    A-->>O: session/update tool_call_update (completed / failed)
  end
  A-->>O: stopReason end_turn
```

## How to use

This section is for people who only want to use ge-bridge. You need `ge-bridge.exe`, the `extension` folder, and Chrome or Edge where you can sign in to Gemini Enterprise. If you do not have the exe yet, ask whoever maintains your copy, or build it yourself (see [Build](#build)).

ge-bridge works with any tool that can talk to an OpenAI- or Anthropic-compatible API. OpenMausBot gets a one-click setup. Every other harness needs three values, listed in [Any other harness](#any-other-harness).

### Step 1: First-time setup (about 5 minutes)

1. **Start ge-bridge.** Double-click `ge-bridge.exe`. No window opens. A ge-bridge icon appears in the system tray at the bottom-right of the taskbar. If you cannot see it, click the `^` arrow to show hidden icons. If Windows shows a "Windows protected your PC" warning, the exe is unsigned. Click **More info**, then **Run anyway** only if you trust where the file came from.
2. **Install the browser extension.** Go to `chrome://extensions` (or `edge://extensions` in Edge). Turn on **Developer mode** in the corner of the page. Click **Load unpacked** and pick the `extension` folder. Pin the extension to the toolbar so you can find it again.
3. **Check the connection.** You do not need to copy any key. The extension finds ge-bridge on `127.0.0.1:8787` and pairs by itself within about 20 seconds. Click the extension icon. The **Server** line turns green, and **Ping server** shows the round-trip time.
4. **Open your Gemini Enterprise page.** Each organization has its own address, shaped like `https://vertexaisearch.cloud.google/us/home/cid/<your-id>?csesidx=<number>`. Open it the way you normally do (bookmark or the link from your organization) and sign in. The extension remembers this address. After that, **Open Gemini Enterprise** in the popup and **🌐 Open Gemini Enterprise** in the tray open it for you. You can also paste the address once under **Advanced → Gemini URL** in the popup. A small status pill appears in the bottom-right corner of the page. Click **Ping Gemini** in the popup to check that your Gemini session works. It may add an empty chat to your Gemini history.
5. **Load your models and pick a default.** Click **🔄 Models** on the page pill. The models your tenant offers appear in the tray under **Default Model** (for example `gemini-4-argon`, `gemini-3.8-flash`, `gemini-3.1-pro-preview`). Tick the one to use when a harness does not name a model.

Scan the models **before** you connect a harness in Step 2. Most harnesses read the model list only once, when you add the connection.

### Step 2: Connect your harness

#### OpenMausBot (one click)

1. **Quit OpenMausBot** (from its tray icon or window). Otherwise OpenMausBot writes its own copy of the bot list back over the change in step 2.
2. Right-click the ge-bridge tray icon and choose **Quick Integrations → ⚡ Auto-Configure OpenMausBot**. It makes these changes:
   - **Adds the engine.** A "Gemini Enterprise (GeBridge)" instance named `ge-bridge-acp` goes into `%USERPROFILE%\.openmausbot\config.json`. It is a `customAcp` engine that runs `"<folder of ge-bridge.exe>\ge-bridge-cli.exe" acp` (see [What the ACP engine can do](#what-the-acp-engine-can-do-files-and-commands)). The old file is saved first as `config.json.bak`.
   - **Sets the icon.** The engine shows the GeBridge icon. If you pick a different icon in OpenMausBot, running Auto-Configure again keeps it.
   - **Removes the old engine.** Earlier versions wrote an OpenAI-compatible instance named `gemini-enterprise`. It is removed.
   - **Moves your bots.** Every bot and thread that used `gemini-enterprise` is switched to `ge-bridge-acp` in `bots.json` (backup `bots.json.bak`). Each one keeps its chosen Gemini model. If OpenMausBot was still running, `bots.json` is left alone and the notification says so. Quit OpenMausBot and click again.

   A Windows notification confirms what was set: whether image generation was turned on, and how many bots were moved. If the config could not be written, it shows the error.
3. Start OpenMausBot. In a bot's model picker, choose **Gemini Enterprise (GeBridge)**, then click **Refresh models** and pick the Gemini model you want.

If the picker shows only one model or old names, scan the models again (Step 1, item 5) and click **Refresh models** again.

**Approval cards depend on the thread's approval mode.** OpenMausBot asks before every file write and command only when the thread's approval mode is **Ask**. A thread in **Full access** mode approves everything at once, so no card appears. The `request.resolved … allow` event in the log a few milliseconds after `request.opened` means the thread approved the call automatically.

Auto-Configure also turns on **image generation** in OpenMausBot, unless you already chose another image provider there. It sets OpenMausBot's image provider to **Custom**, pointed at ge-bridge (`imageGen` in the same config file). Bots then get OpenMausBot's own `generate_image` tool, Gemini Enterprise draws the picture, and OpenMausBot attaches it to the reply like any other generated image. To use it, ask a bot for a picture, for example "draw a red apple on a white table". Each image takes 10 to 60 seconds and adds one chat to your Gemini history.

OpenMausBot treats an engine at `127.0.0.1` as a small local model and turns on its tool bridge: the bot sees only a few tools plus `agents_find_tool` and `agents_use_tool`, and `generate_image` is one of the hidden ones. Left alone, Gemini tends to answer "draw…" with a written prompt instead of searching for the tool. ge-bridge therefore adds a **HIDDEN TOOLS** note to the tool instructions it sends Gemini: it names the hidden tools, says to run them through `agents_use_tool`, and gives the exact call for `generate_image`. `ge-bridge-cli logs --level debug` shows `hidden-tools hint added` on each turn where this happened.

Gemini Enterprise still often insists that it "is a text service and cannot make images", whatever the instructions say. When the start of its answer says that and the bot has an image tool, ge-bridge does not pass the refusal on. It asks Gemini, in a new chat, only to write an English image prompt for the conversation, which Gemini does, and answers OpenMausBot with the `generate_image` call itself (through `agents_use_tool` when the tool is hidden). The log shows `image rescue produced a tool call`. The prompt writer gets the user's own request: the "Context from OpenMausBot …" block that OpenMausBot puts in front of a user message (computers, time and so on) and its `[From OpenMausBot` retry notes are left out, because they once turned a portrait request into a picture of a desk with a browser and a terminal. When the request already is a detailed English image prompt, it is used as it is. The picture is square unless the request names an orientation ("landscape format", "portrait orientation", "vertical", "แนวนอน", "แนวตั้ง", 16:9, 9:16); "85mm portrait lens" or "mountain landscape" do not count. This adds one more Gemini chat and about 20 seconds before the image starts.

**Where the picture ends up.** OpenMausBot attaches the image to the bot's reply itself. ge-bridge also saves it into the bot's own folder, as OpenMausBot's Antigravity engine does: when the turn that carries the `generate_image` result arrives, ge-bridge reads the bot's name from the system prompt ("You are Artist, a personal bot in OpenMausBot"), looks it up in `%USERPROFILE%\.openmausbot\bots.json`, and writes the image to `<working folder>\generated-images\<words from the prompt>-<time>.png` (the bot's workspace folder when it has no working folder; `Pictures\GeBridge` when the bot is not found). The reply then ends with a preview and a link to that file, which OpenMausBot shows inline because the file is inside the bot's folder. The server log shows `saved the image to …`.

#### What the ACP engine can do (files and commands)

The engine that Auto-Configure adds is ge-bridge's own coding agent, `ge-bridge-cli acp`. OpenMausBot talks to it over the [Agent Client Protocol](https://agentclientprotocol.com) (its `customAcp` driver). A bot on this engine can read files, write files, edit files and run commands in its working folder. It can also use OpenMausBot's own tools.

- **What OpenMausBot sends.** OpenMausBot sends the bot's persona and standing instructions in front of the user's message in ACP, ending with a "Current time when this message was sent: …" line. The agent moves that part into the system prompt. Only the user's own words count as the request. A persona that talks about pictures (such as a sticker artist) therefore does not turn "สร้างไฟล์ hello.txt" into an image.
- **Version check.** Before it starts the agent, OpenMausBot checks the command with `--version`. `ge-bridge-cli acp --version` prints the version and exits.
- **Setting it up by hand.** Use the real path, double every backslash, and put a path with spaces in quotes, because OpenMausBot splits the command at spaces:

  ```json
  "ge-bridge-acp": {
    "driver": "customAcp",
    "displayName": "Gemini Enterprise (GeBridge)",
    "config": { "cli": "\"C:\\Program Files\\ge-bridge\\ge-bridge-cli.exe\" acp" }
  }
  ```

  `ge-bridge.exe --acp` also works, for configs written for the old stub.
- **What has to be running.** ge-bridge.exe must be running and the Gemini tab must be open, as for every other harness. `ge-bridge-cli acp` reads the same token and port that `ge-bridge-cli doctor` uses.
- **Other harnesses.** The HTTP endpoints are unchanged. Any other harness can still use `/v1/chat/completions` directly.

What the agent can do:

| Tool | What it does | Needs your approval |
| --- | --- | --- |
| `read_file` | Reads a text file, the whole file or a line range (`offset`, `limit`), with line numbers | No |
| `list_dir`, `grep` | Lists folders and searches files for literal text. `.git`, `node_modules` and `target` are skipped. | No |
| `write_file`, `edit_file` | Creates or overwrites a file, or replaces one exact piece of text | Yes |
| `run_command` | Runs a PowerShell command in the bot's folder (default timeout 120 s, maximum 600 s) | Yes |
| `<server>_<tool>` | Every tool of the MCP servers OpenMausBot passes in, such as `agents_use_tool`, which also reaches `generate_image` | Approval is handled by OpenMausBot's own gates |

Safety rules:

- **Folder only.** Every path is resolved and must stay inside the bot's working folder. `..`, absolute paths elsewhere, and symlinks or junctions that point outside are refused.
- **Secrets are off limits.** `.env*`, `*.pem`, `*.key`, `id_rsa*`, `.npmrc`, `credentials.json` and similar files are never read or written. Nothing inside `.git` or `.ssh` is ever written.
- **Approval card.** Every write and every command shows an approval card in OpenMausBot (`session/request_permission`) with three choices: **Allow**, **Always allow in this session** (for that kind of call), and **Reject**. A rejection is passed to the model, which then has to change its plan.
- **Commands are contained.** Commands run without environment variables whose names contain `KEY`, `TOKEN`, `SECRET`, `PASSWORD` or `CREDENTIAL`. On timeout or **Stop**, the whole process tree is killed. Output is capped at 30,000 characters.
- **Limits per turn.** A turn ends after 30 model calls (`max_turn_requests`). **Stop** in OpenMausBot (`session/cancel`) ends the stream, the running command, or a waiting approval.

Images from an MCP tool are saved to `<folder>\generated-images\` and shown in the tool card. Gemini still decides each step through the prompt-based tool format described in [How agent turns are sent to Gemini](#how-agent-turns-are-sent-to-gemini). Long multi-step tasks therefore depend on how well Gemini follows it, and every step waits for the human-pacing delay.

#### Any other harness

Every OpenAI- or Anthropic-compatible tool needs the same three values:

| Setting | OpenAI-style tools (Hermes, Cline, Roo Code, Continue, Aider, OpenWebUI, SDKs) | Anthropic-style tools (Claude Code) |
| --- | --- | --- |
| Base URL | `http://127.0.0.1:8787/v1` | `http://127.0.0.1:8787` (no `/v1`) |
| API key | Your pairing token | Your pairing token |
| Model | An id from `GET /v1/models`, or `default` | Any name. `claude-*` names go to the tray's default model. |

- **Get the token:** tray → **Quick Integrations → 🔑 Copy Pairing Token**. For tools configured through environment variables, **📋 Copy Hermes / Cline / OpenAI Env** and **📋 Copy Claude Code Anthropic Env** in the same menu copy ready-made lines. Each copy shows a Windows notification saying what is on the clipboard and where to paste it.
- **The base URL is the usual mistake.** OpenAI-style tools need `/v1` at the end. Anthropic-style tools add `/v1/messages` themselves, so a base URL that ends in `/v1` returns 404.
- **Ready-made configs** for Hermes, Claude Code, Continue, Cline, Aider and Zed are in [docs/harness-templates.md](docs/harness-templates.md).

To check the three values before you touch the harness, run this in PowerShell with your token:

```powershell
curl.exe -H "Authorization: Bearer gemb_YOUR_PAIRING_TOKEN" http://127.0.0.1:8787/v1/models
```

It prints the model list. A 401 means the token is wrong. An empty list means the models have not been scanned yet. [`ge-bridge-cli doctor`](#command-line-tool) checks all of this in one go.

**Image generation** for any harness: ge-bridge serves the OpenAI Images API at `POST http://127.0.0.1:8787/v1/images/generations` with the same token. Send `{"prompt": "...", "size": "1024x1024"}`; the answer is `{"data": [{"b64_json": "..."}]}`. Set `"response_format": "url"` to get a `data:` URL instead. `size` only picks square, landscape or portrait, `n` must be 1, and `model` is ignored because Gemini chooses its image model. Plain chat requests never generate images.

### Choosing a model

The model name the harness sends decides which Gemini model answers:

| The harness sends | Gemini uses |
| --- | --- |
| An id from `/v1/models`, such as `gemini-3.5-flash` | That model |
| `default`, `auto`, `gemini-enterprise`, or any `claude-*` or `gpt-*` name | The tray's **Default Model** |
| An id followed by `:<effort>`, such as `gemini-4-argon:high` | That model. The effort part has no effect (see [Known limitations](#known-limitations)). |
| A model the tenant has locked | An error that names the model |

To check which model answers, choose **🌐 Open Control Center** in the tray and ask "Which model are you?" in the playground.

### Every day

1. Make sure the ge-bridge icon is in the tray. To start it automatically, tick **Start with Windows** in the tray menu.
2. Keep one signed-in Gemini Enterprise tab open. Leave its window open but not minimized. It can sit behind other windows.
3. Use your harness as usual. Each request goes to Gemini Enterprise through your signed-in session, and the answer streams back.

The tray menu and the Control Center list the Gemini tabs the server can use. A browser tab shows as 🌐 with its chat (`new chat` or `session …123456`) and model. The two ⚡ API lanes are built in, so they appear even when only one browser tab is open.

### If something goes wrong

| What you see | What to do |
| --- | --- |
| The popup says "Server not running" | Start `ge-bridge.exe` and check that the tray icon is there. The extension reconnects by itself. |
| The popup says "Pairing refused by server" | The extension was loaded from a modified `manifest.json` (its ID changed). Reload the original `extension` folder. As a fallback, paste the token from **🔑 Copy Pairing Token** under **Advanced** in the popup. |
| **Ping Gemini** fails at step `target` | The tab is not on your Gemini Enterprise home address. Open the address with `/home/cid/…?csesidx=…`. |
| **Ping Gemini** fails at step `auth` | Your Gemini session expired. Sign in again in the tab. |
| The model list is empty, or a harness shows old model names | Click **🔄 Models** on the page pill, **🔄 Refresh Available Models** in the tray's **Default Model** menu, or run `ge-bridge-cli models --refresh`. Then restart the harness or re-detect its models. The list is cached for 24 hours, so only these explicit refreshes open the Gemini model picker. |
| OpenMausBot says image generation failed | Run `ge-bridge-cli image "a red apple"` to test the same path. The error says whether Gemini made no image (rephrase the request) or the extension was not reachable. |
| The answer comes from a different model than you picked | Check the model name the harness sends (see [Choosing a model](#choosing-a-model)). `default`, `claude-*` and `gpt-*` names go to the tray's default model. |
| "Chat input box not found" or "Timed out waiting for … reply" | Reload the Gemini tab. If it keeps happening, Gemini changed its page layout. Click **🎯 Calibrate** on the pill and follow its prompts: the chat input, the Send button, an answer, and the model picker. It types a test text so the Send button appears. Press Esc to skip a step that has nothing to click. |
| Answers are slow or stop halfway | Make sure the Gemini tab is not in a minimized window. Turn off Chrome's Memory Saver for `vertexaisearch.cloud.google`. A tab Chrome has put to sleep is skipped until you open it again. |
| Your harness returns 401 | The token in the harness is wrong. Copy it again from the tray. |
| Your harness returns 404 | The base URL is wrong. Check the `/v1` rule in [Any other harness](#any-other-harness). |
| You signed out of Gemini or your session expired | Sign in again in the tab. ge-bridge carries on once the page is back. |

### Logs

When something does not work, the logs say why. Right-click the tray icon and choose **📄 Open Logs Folder**. The folder is `%APPDATA%\ge-bridge\logs`, and it holds two files:

- `extension.log` records what the browser extension saw: pairing, WebSocket state, which tabs matched, whether the page could be scripted (and the exact error if not), API preflight results, model scans, and every chat turn with the model it used.
- `server.log` records what ge-bridge itself did.

Both files are JSON Lines, one entry per line with a level: `debug`, `info`, `warn` or `error`. To read them, use `ge-bridge-cli logs` (see [Command-line tool](#command-line-tool)): it prints one line per entry in local time, hides `debug` by default, and filters by level and text. Routine checks (the extension's keep-alive pings and its periodic tab diagnostics) are `debug`, so the default view shows what actually happened. Each file rotates at 1 MB and keeps one `.1` backup. Tokens, cookies, JWTs and session names are never written. Your `cid` and `csesidx` are cut down to their first few characters. If the extension cannot reach ge-bridge, open the popup, expand **Logs**, and click **Download log** to save the same entries as a file.

To stop ge-bridge, right-click the tray icon and choose **Quit GeBridge**.

### Command-line tool

`ge-bridge-cli.exe` sits next to `ge-bridge.exe`. It tests and inspects a running ge-bridge from PowerShell or cmd, using the token stored for your Windows user. It never starts the server.

| Command | What it does |
| --- | --- |
| `ge-bridge-cli doctor` | Checks the server, the token, the extension, the Gemini tab and home URL, the model list, and the OpenMausBot link. Each failed check says how to fix it. Exits with 1 if any check fails. |
| `ge-bridge-cli status` | One-screen summary |
| `ge-bridge-cli models [--refresh]` | Lists the models, with `*` on the default. `--refresh` rescans the Gemini model picker now instead of using the 24-hour cache. |
| `ge-bridge-cli tabs` | Lists the browser tabs and API lanes the server can use |
| `ge-bridge-cli chat "Which model are you?" [--model gemini-3.5-flash]` | Sends one prompt and prints the answer |
| `ge-bridge-cli image "a red apple" [--size 1536x1024] [--out apple.png]` | Generates one image and saves it |
| `ge-bridge-cli logs [--ext] [-n 50] [--level debug\|info\|warn\|error] [--grep TEXT] [--json]` | Prints the last entries of `server.log`, or `extension.log` with `--ext`, one readable line each: time, level, source, tab, message, then the details as `key=value`. `--level` is the lowest level shown (default `info`; `debug` shows everything). `--grep` keeps entries containing the text (case-insensitive). `--json` prints the raw lines. `-n` counts entries after filtering. |
| `ge-bridge-cli link-openmausbot` | Same as **⚡ Auto-Configure OpenMausBot** in the tray |
| `ge-bridge-cli acp` | Runs as an ACP agent on stdin/stdout, for OpenMausBot's `customAcp` driver (see [What the ACP engine can do](#what-the-acp-engine-can-do-files-and-commands)). It prints only protocol JSON and logs to stderr. |
| `ge-bridge-cli token` | Prints the pairing token. Keep it private. |

Example: `ge-bridge-cli logs --ext --level warn -n 20` shows the last 20 warnings and errors from the extension, such as

```
17:27:32.930 WARN  bg      api session failed  attempt=0 httpStatus=401 error="..."
```

## Requirements

- Windows 10 or later.
- Rust via rustup, with the MSVC toolchain.
- Visual Studio Build Tools with the "Desktop development with C++" workload.
- Chrome or Edge, signed in to Gemini Enterprise.
- Node.js 24 or later, only for the tests.

## Build

```powershell
git clone https://github.com/SawatKia/ge-bridge.git
cd ge-bridge
cargo build --release
```

This builds two programs in `target\release`: `ge-bridge.exe` (the tray app; run it to start the server) and `ge-bridge-cli.exe` (the console tool described in [Command-line tool](#command-line-tool)). Both share the code in `src/lib.rs`.

## Setup

1. Load the extension. Open `chrome://extensions` or `edge://extensions`, enable Developer mode, click "Load unpacked", and select the `extension` folder.
2. Pairing is automatic. The extension polls `GET /api/health`. When the server answers, it calls `POST /api/pair`. The server returns the token only when the `Origin` header is `chrome-extension://lkbfapcoeagmekmncfbdgiefneohalej`. That ID is fixed by the `key` in `manifest.json`, and web pages cannot forge it. The extension then connects to `ws://127.0.0.1:8787/ws/extension?token=<token>`. The token is stored in Windows Credential Manager. Pasting it manually under **Advanced** remains a fallback.
3. Open your organization's Gemini Enterprise URL (`https://vertexaisearch.cloud.google/<location>/home/cid/<cid>?csesidx=<n>`; the `.google.com` host also works) and sign in. The extension learns the URL from the tab, stores it, and sends it to the server (`GEMINI_TARGET` frame), so the tray can open it. Keep that tab in a non-minimized window, or Chrome may throttle it and slow streaming.
4. Sync models. Model ids are read from the browser tab's model picker at runtime. They are the ids Gemini's API takes: `Gemini 3.1 Pro · Preview` becomes `gemini-3.1-pro-preview`. Use the on-page pill or **🔄 Refresh Available Models** in the tray's **Default Model** submenu to refresh the list, then pick a default model. The list is saved in `models_cache.json` and reused for 24 hours. Within that time, connecting the extension and `GET /v1/models?refresh=true` do not rescan. Only the tray, the dashboard button, the page pill and `ge-bridge-cli models --refresh` do.
5. Connect a harness. See [Step 2: Connect your harness](#step-2-connect-your-harness).

The dashboard is at `http://127.0.0.1:8787/`.

A request can append a reasoning suffix to the model id, for example `some-model:high`. It is parsed but has no effect on Gemini Enterprise.

## Endpoints

HTTP auth is `Authorization: Bearer <token>` or `x-api-key: <token>`.

| Endpoint | Protocol | Client base URL |
| --- | --- | --- |
| `POST /v1/chat/completions` | OpenAI | `http://127.0.0.1:8787/v1` |
| `GET /v1/models` | OpenAI | `http://127.0.0.1:8787/v1` |
| `POST /v1/messages` | Anthropic | `http://127.0.0.1:8787` (no `/v1`) |
| `POST /v1/images/generations` | OpenAI Images (`b64_json` or data `url`) | `http://127.0.0.1:8787/v1` |
| `GET /api/health` | Health check, no auth | |
| `POST /api/pair` | Token for the extension origin only | |
| `GET /` | Dashboard | |
| `ws://127.0.0.1:8787/ws/extension?token=<token>` | Extension link | |

Requests for `default`, `auto`, `gemini-enterprise`, `claude-*` and `gpt-*` model names are routed to the default model chosen in the tray.

### How agent turns are sent to Gemini

Gemini Enterprise has no native tool calling, so ge-bridge writes the tools into the prompt and turns `<tool_call>{...}</tool_call>` in the answer back into OpenAI `tool_calls` or Anthropic `tool_use` blocks. Several parts of this follow the [FreeAPI-TH-AI-Gemini-Enterprise](https://github.com/newbiexM/FreeAPI-TH-AI-Gemini-Enterprise) bridge:

| Part | What it does |
| --- | --- |
| `[ROLE]` first | Tells Gemini it is an agent's model whose tools really run, and must not answer "I am only a chat model" |
| Tool descriptions cut to 900 characters | An agent with 100+ tools otherwise fills most of the prompt (one OpenMausBot turn was 188,000 characters) |
| Tool results pruned | The last 8 stay whole (up to 20,000 characters, longer ones keep their start and end); older ones are cut to 300 characters |
| `[ACTIVE OBJECTIVE]` | When a turn ends on tool results, the user's request is restated so Gemini answers it instead of summarising |
| `[OUTPUT_RULES]` last | The call format and the hidden-tool note come after the system prompt and history, where Gemini pays most attention |
| Tolerant parsing | `<<<TOOL_CALL>>>…<<<END_TOOL_CALL>>>`, `<TOOL_CALL>`, fenced JSON and `{"function":{...}}` are accepted too |

**Refusal and policy recovery.** The first 300 characters of an agent turn's answer (120 for plain chat) are held back. If they are a refusal ("Sorry, I could not respond to this request", "ขออภัย ไม่สามารถตอบคำถามนี้ได้", or, when tools exist, "I can't …" / "ไม่สามารถ …"), or the turn fails with a policy or bad-request error (HTTP 400/422, RAI), ge-bridge retries once in a new Gemini chat with a short prompt: the role, the system prompt cut to 8,000 characters, the tools, and only the current request with its tool results. Card numbers (4-4-4-4) and Thai bank account numbers (3-1-5-1 with separators) are masked in that retry. The client never sees the refused text. Image refusals get the image rescue described under OpenMausBot instead. ge-bridge does not reword the user's text to get past Gemini's safety filters.

**Reasoning.** Gemini's thoughts go to `reasoning_content` (OpenAI) or a `thinking` content block before the text block (Anthropic, streamed as `thinking_delta`), never into the answer text. Thoughts are never held back.

**Latency.** Text is forwarded as soon as the extension reads it from Gemini's stream, apart from the recovery window above and any partial `<tool_call>` tag. `ge-bridge-cli logs --level debug` shows `first thought N ms, first text N ms, done N ms` for every turn; most of that time is Gemini's own.

**Human pacing.** The extension waits before sending each question about as long as a person would take to read the previous answer: the answer's word count (Thai words are found with `Intl.Segmenter`, so they count without spaces) divided by a random reading speed between 200 and 250 words per minute, plus Gaussian jitter (standard deviation 15% of the reading time), kept between a minimum of 1.5 s and a maximum of 20 s. The wait is shared by all tabs and API lanes, and queued questions go one at a time. Turn it off or change the reading speed, the minimum and the **maximum delay** (up to 600 s) in the popup under **Advanced**. Image requests never wait more than 20 s, because ge-bridge gives an image 115 s and OpenMausBot 120 s. Pacing makes agent tool loops slower: every tool round waits too. It spaces requests out; it does not hide that the requests come from the extension. The extension log shows `pacing: next question not before` (with words, wpm and ms) and `pacing: waiting before the next question` at debug level.

## Connecting other harnesses

See [docs/harness-templates.md](docs/harness-templates.md) for config templates.

## Tests

Run every layer with one command from the repo root:

```powershell
powershell -File tools/test-all.ps1
```

It stops at the first failing stage. The stages run in this order:

| Stage | Command | What it covers |
| --- | --- | --- |
| Rust unit | `cargo test` | Stream parser (`<tool_call>` split at any byte), prompt rendering (role, rules last, tool pruning, recovery prompt, number masking), refusal detection and image rescue helpers, model and effort resolution, URL validation, origin and auth checks, extension link bookkeeping, env config |
| Build | `cargo build --release` | The exe the integration tests run |
| Extension unit | `node --test "tests/unit/*.test.mjs"` | `extension/lib/gemini.js`: Gemini URL parser, base64url, JWT signing, the Ping Gemini probe with a fake `fetch`, human pacing (word count, reading speed, Gaussian jitter, clamping) |
| Integration | `node --test "tests/integration/*.test.mjs"` | The real exe end to end, with a fake extension (`tests/support/fake-extension.mjs`) in place of the browser, including `/v1/images/generations`, `ge-bridge-cli`, and `ge-bridge-cli acp` driven by a test ACP client (`tests/support/acp-client.mjs`) with a fake MCP server (`tests/support/fake-mcp.mjs`) |

Integration tests start their own copy of the exe on a free port with a temporary data dir. They do not touch a running ge-bridge, your browser extension, your prefs, or your stored token. Three environment variables make this possible, and you can also use them by hand:

| Variable | Effect |
| --- | --- |
| `GE_BRIDGE_PORT` | Port for this run only. It is not saved to prefs. |
| `GE_BRIDGE_DATA_DIR` | Prefs and a plain `token` file live in this folder. Credential Manager and `%APPDATA%\ge-bridge` are not used. It also allows a second instance to run. |
| `GE_BRIDGE_NO_TRAY` | `1` runs the server without a tray icon. |
| `GE_BRIDGE_TEST_BIN_DIR` | Tests only: the folder holding the `ge-bridge.exe` and `ge-bridge-cli.exe` to test, instead of `target\release`. Use it while your release exe is running and locked: `$env:CARGO_TARGET_DIR="target\test-build"; cargo build --release`, then set it to `target\test-build\release`. |

None of the tests reach Gemini Enterprise. The DOM automation in `extension/content.js` and the live Ping Gemini call still need a manual check on a real tab.

## Known limitations

- The DOM transport finds the chat input, the Send button, the model picker and the answers by the page's own element names (`.ProseMirror`, `.send-button.submit`, `#model-selector-menu-anchor`, `ucs-response-markdown`). These were checked on one live tenant with the Thai UI in November 2026. If Google renames them, run the calibrator from the on-page pill or the tray item **🎯 Visual Calibrate Browser Selectors**. Calibration is stored separately for the new-chat page and for chat-session pages.
- Chat turns go through Gemini Enterprise's internal web API by default. The flow is `/auth/getoxsrf`, then a self-signed JWT, then `widgetCreateSession` and `widgetStreamAssist`. The approach comes from a reference project that ran it against a live tenant. It has not been verified on other tenants, and the internal API can change without notice. The **Transport** setting in the popup's Advanced section controls the order:

  | Transport | Behavior |
  | --- | --- |
  | Auto (default) | API from the Gemini tab, then API from the extension service worker, then DOM automation. Fallback happens only on auth or network errors (including a 401 when the stream starts), and only before any text has streamed. When every way fails, the error names each one's reason and, if no tab was open, says to open the Gemini page. |
  | API from extension | Service worker only. In November 2026 Gemini answered 401 to every session and stream call made from the service worker, even with a fresh JWT, while the same calls from the page worked; the cause is not known. That is why Auto tries the page tab first. |
  | API from tab | Runs inside the Gemini page with its origin. |
  | DOM | Types into the page as before. |

- Both transports switch models. The API path sends the model as `assistGenerationConfig.modelId`, the same field the web UI sends. `auto` sends no model, so Gemini picks one.
- Reasoning effort has no effect. Gemini Enterprise has no effort control in its UI or in its API, so the tray's **Reasoning Effort** setting and `:<effort>` suffixes are accepted and then ignored. In November 2026 `widgetStreamAssist` rejected every candidate field with HTTP 400 "Unknown name … Cannot find field": `thinkingConfig` (with `thinkingLevel` or `thinkingBudget`), `thinkingLevel`, `thinkingBudget`, `reasoningEffort`, `reasoningLevel`, `effort`, `thinking`, `thinkingSpec`, `thinkingMode`, `thoughtConfig`, `generationConfig`, `modelConfig` and `modelSpec` inside `assistGenerationConfig`, and `thinkingLevel` on the request itself. The reference bridge's "low to max" levels apply to its other backend (TH-AI Passport), not to Gemini Enterprise.
- Chat turns that attach images use the DOM transport.
- Image generation runs over the API only (extension service worker, then the Gemini tab). It turns on Gemini's image tool (`toolsSpec.imageGenerationSpec`) and fetches the result with `downloadFile`, which needs the `X-Goog-Encode-Response-If-Executable: base64` header and the tenant's project number (read once from `lookupWidgetConfig`). Both were checked on one live tenant in November 2026.
- The ACP agent (`ge-bridge-cli acp`, or `ge-bridge.exe --acp`) does not support `session/load`, so a restarted OpenMausBot starts a new agent session. It does not accept image prompts, and only stdio MCP servers can be used, not HTTP. Only the file and command tools are tested end to end, with a fake extension; the steps Gemini decides are not.
- The server depends on a signed-in Gemini Enterprise tab being open in a browser with the extension loaded. Chrome's Memory Saver unloads background tabs, and an unloaded tab cannot run a turn (every "No scriptable Gemini tab" failure seen in November 2026 happened that way). The extension therefore marks Gemini tabs as not auto-discardable; if a turn still finds the tab unloaded, it reloads that tab in the background and waits up to 30 s for it, and if no Gemini tab is open it opens the saved home page in a background tab. The extension log shows `gemini tab: waking it for a turn` and `gemini tab: ready after waking`.

## Contributing

Contributions are welcome. Keep changes small and focused, one topic per change.

### Development setup

1. Install the tools listed in [Requirements](#requirements).
2. Build a debug binary with `cargo build`. A debug build keeps a console window, so `eprintln!` output is visible. The release build (`cargo build --release`) hides the console.
3. Load `extension/` unpacked as described in [How to use](#how-to-use). After editing extension files, click the reload button on the extension card and refresh the Gemini tab.

### Test-driven workflow

1. Write a test that shows the bug or the new behavior, and run it to see it fail. Pure Rust logic gets an inline `#[cfg(test)]` test next to the code. Extension logic goes in `extension/lib/gemini.js` with a test in `tests/unit/`. Anything that crosses HTTP or WebSocket gets a test in `tests/integration/`.
2. Make the smallest change that turns it green.
3. Run `powershell -File tools/test-all.ps1`. Every stage must pass, so you know nothing old broke.

### Before you submit a change

- `tools/test-all.ps1` passes, with no new compiler warnings.
- If you change the server ↔ extension protocol, update `tests/support/fake-extension.mjs` and add an integration test for the new behavior.
- Keep `chrome.*`, DOM and WebSocket code thin. Put logic you want tested into `extension/lib/gemini.js`, which Node can load.
- Any change to `extension/content.js` is tried by hand on a live Gemini Enterprise tab. The automated tests do not cover the page's DOM.
- Docs follow the code. If you rename a tray menu item, an endpoint, or a setup step, update this README and `docs/harness-templates.md` in the same change.

### Ground rules

- Never commit a real pairing token, screenshot, or chat log from your tenant. Use the placeholder `gemb_YOUR_PAIRING_TOKEN` in docs.
- `tools/` scripts stay zero-dependency (plain Node, no `package.json`).
- Do not add size, memory, or speed figures to the docs unless you measured them and say how.
- Bug reports help most when they include the ge-bridge version, browser and version, the log from the Control Center, and what the extension pill showed.

## Repo layout

```text
Cargo.toml, Cargo.lock
build.rs                embeds assets/ge-bridge.ico as the exe and tray icon
assets/
  icon.svg              master icon
  ge-bridge.ico         generated by tools/make-icons.ps1
src/
  main.rs               tray app entry point (ge-bridge.exe)
  bin/ge-bridge-cli.rs  console CLI (ge-bridge-cli.exe)
  lib.rs, config.rs, state.rs, link.rs, server.rs, images.rs, integrations.rs, tray.rs, logfile.rs
  stream.rs             prompt rendering, tool-call parsing, image tool detection
  recovery.rs           refusal and policy-block detection, image rescue prompt
  logview.rs            readable log lines for ge-bridge-cli logs
  notify.rs             tray notifications
  bot_images.rs         saves generated images into the asking bot's folder
  acp/                  ge-bridge-cli acp: rpc.rs (stdio JSON-RPC), agent.rs (sessions, tool loop, approvals),
                        llm.rs (client for the local server), tools.rs (file/command tools, folder confinement),
                        mcp.rs (MCP stdio client), types.rs
  dashboard.html        embedded into the exe
extension/
  manifest.json, background.js, content.js, popup.html, popup.js
  lib/gemini.js         pure, unit-tested logic shared by background.js and content.js
  icons/                generated by tools/make-icons.ps1
tests/
  unit/                 extension lib tests (node:test)
  integration/          end-to-end tests against an isolated exe (node:test)
  support/              test server launcher and fake extension
tools/
  test-all.ps1          runs every test layer
  make-icons.ps1        regenerates every raster icon from assets/icon.svg (needs ImageMagick)
docs/
  harness-templates.md
```
