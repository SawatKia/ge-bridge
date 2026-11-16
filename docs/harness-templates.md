# Harness config templates

Replace `gemb_YOUR_PAIRING_TOKEN` in every template with the token copied from the tray menu (Quick Integrations submenu). Never commit a real token.

Model ids come from the browser tab at runtime. The ids below (`gemini-4-argon`, `gemini-3.8-flash`) are placeholders. Call `GET /v1/models` to see the real list. A model id may carry a reasoning suffix, for example `gemini-4-argon:high`.

Check your organization's IT policy before automating an enterprise tenant's web UI. Keep the Gemini tab in a non-minimized window so Chrome does not throttle it.

## Cheat sheet

OpenAI-style harnesses need the `/v1` suffix. Anthropic-style harnesses append `/v1/messages` themselves, so a base URL that includes `/v1` gives a 404.

| Harness | Protocol | Base URL | Model selection |
| --- | --- | --- | --- |
| Hermes Agent | OpenAI Chat Completions | `http://127.0.0.1:8787/v1` | Reads `/v1/models` |
| Claude Code | Anthropic Messages | `http://127.0.0.1:8787` (no `/v1`) | Any `claude-*` name is routed to the tray's default model |
| Continue.dev | OpenAI | `http://127.0.0.1:8787/v1` | Explicit model list in `config.yaml` |
| Cline / Roo Code | OpenAI | `http://127.0.0.1:8787/v1` | Dropdown from `GET /v1/models` |
| Aider | OpenAI via LiteLLM | `http://127.0.0.1:8787/v1` | Prefix the slug with `openai/` |
| Zed | OpenAI | `http://127.0.0.1:8787/v1` | `available_models` list in settings |
| OpenWebUI, SDKs | OpenAI | `http://127.0.0.1:8787/v1` | Reads `/v1/models` |

ACP clients (OpenMausBot `customAcp`, Zed's custom agent servers) run `ge-bridge-cli.exe acp` as the agent command. See "OpenMausBot as an ACP agent" in the README.

## 1. Hermes Agent

On native Windows, Hermes reads `%LOCALAPPDATA%\hermes\config.yaml` and `%LOCALAPPDATA%\hermes\.env` (on WSL2, `~/.hermes/`). Set `base_url` in `config.yaml` and keep the token in `.env`.

`%LOCALAPPDATA%\hermes\.env`:

```bash
GE_BRIDGE_API_KEY=gemb_YOUR_PAIRING_TOKEN
```

`%LOCALAPPDATA%\hermes\config.yaml`:

```yaml
model:
  provider: custom
  base_url: http://127.0.0.1:8787/v1
  key_env: GE_BRIDGE_API_KEY
  default: gemini-3.8-flash
```

Interactive alternative: run `hermes model` in PowerShell, pick Custom endpoint, enter `http://127.0.0.1:8787/v1`, paste the token, and choose Chat Completions.

## 2. Claude Code

Claude Code speaks the Anthropic Messages protocol. Set `ANTHROPIC_BASE_URL` without `/v1`. Blank `ANTHROPIC_API_KEY` so it does not fall back to a cloud key. Any `claude-*` model name is routed to the tray's default model.

`%USERPROFILE%\.claude\settings.json`:

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

One-off PowerShell session:

```powershell
$env:ANTHROPIC_BASE_URL = "http://127.0.0.1:8787"
$env:ANTHROPIC_AUTH_TOKEN = "gemb_YOUR_PAIRING_TOKEN"
$env:ANTHROPIC_API_KEY = ""
claude
```

## 3. Continue.dev

`%USERPROFILE%\.continue\config.yaml`:

```yaml
name: ge-bridge
version: 1.0.0
schema: v1
models:
  - name: Model A (high reasoning)
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

  - name: Model B (fast)
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

## 4. Cline / Roo Code / Kilo Code

In the extension settings, set:

- API Provider: OpenAI Compatible
- Base URL: `http://127.0.0.1:8787/v1`
- API Key: `gemb_YOUR_PAIRING_TOKEN`
- Model ID: pick one from the dropdown, or type an id with a suffix such as `gemini-4-argon:max`

## 5. Aider

Aider routes custom OpenAI-compatible servers through LiteLLM when the model name has an `openai/` prefix.

`%USERPROFILE%\.aider.conf.yml`:

```yaml
openai-api-base: http://127.0.0.1:8787/v1
openai-api-key: gemb_YOUR_PAIRING_TOKEN
model: openai/gemini-4-argon
weak-model: openai/gemini-3.8-flash
editor-model: openai/gemini-3.8-flash
stream: true
```

## 6. Zed

Zed connects over the OpenAI HTTP endpoint. Store the token in Zed's Assistant Panel, Configure, OpenAI API Key.

`%APPDATA%\Zed\settings.json`:

```json
{
  "language_models": {
    "openai": {
      "api_url": "http://127.0.0.1:8787/v1",
      "available_models": [
        {
          "name": "gemini-4-argon",
          "display_name": "Model A (ge-bridge)",
          "max_tokens": 1000000
        },
        {
          "name": "gemini-3.8-flash",
          "display_name": "Model B (ge-bridge)",
          "max_tokens": 1000000
        }
      ]
    }
  }
}
```

## 7. OpenWebUI, LobeChat, SDKs

OpenWebUI: Admin Panel, Settings, Connections, OpenAI API.

- URL: `http://127.0.0.1:8787/v1`
- Key: `gemb_YOUR_PAIRING_TOKEN`

If OpenWebUI runs in Docker Desktop on Windows, use `http://host.docker.internal:8787/v1` as the URL.

Python OpenAI SDK, with a per-request model and reasoning suffix:

```python
from openai import OpenAI

client = OpenAI(
    base_url="http://127.0.0.1:8787/v1",
    api_key="gemb_YOUR_PAIRING_TOKEN",
)

# List models synced from the Gemini Enterprise tab
print([m.id for m in client.models.list().data])

stream = client.chat.completions.create(
    model="gemini-4-argon:high",
    messages=[{"role": "user", "content": "Design a ring buffer in Rust."}],
    stream=True,
)
for chunk in stream:
    delta = chunk.choices[0].delta
    if getattr(delta, "reasoning_content", None):
        print(delta.reasoning_content, end="", flush=True)
    if delta.content:
        print(delta.content, end="", flush=True)
```

## Running two harnesses at once

Open your Gemini Enterprise URL in two tabs. When one tab is busy, the server sends the next request to the idle tab. The integration tests (`tests/integration/`) check this against a fake extension, but it has not been tried with live Gemini tabs.
