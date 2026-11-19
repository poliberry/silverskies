# AI assistant

AI is **optional and off by default**. Both apps work fully without it: forecast summaries and quick answers are written on your computer from the forecast data.

With AI turned on you get:

- **AI summaries** of the forecast (Silver Skies: **Summarize** on the summary card, or automatically when a forecast loads).
- **AI answers** to questions typed in Silver Skies' search bar, under the local answer.
- **The assistant** (Ctrl+J in both apps; `:assistant` in Pro): a chat window that answers questions about your saved places' forecasts and alerts.

## Setting it up

Settings → AI (Silver Skies) or Settings → AI assistant (Pro):

| Provider | Base URL (default) | Key | Model |
|---|---|---|---|
| Claude (Anthropic) | `https://api.anthropic.com` | Required | Defaults to a current Claude model |
| OpenAI | `https://api.openai.com/v1` | Required | A model ID from your account |
| OpenRouter | `https://openrouter.ai/api/v1` | Required | e.g. `anthropic/claude-opus-5-5` |
| OpenCode Go | `https://opencode.ai/inference/go/openai/v1` | Required | e.g. `kimi-k3` |
| OpenCode Console (pay-as-you-go) | `https://opencode.ai/inference/openai/v1` | Required | e.g. `kimi-k3` |
| Ollama (local) | `http://localhost:11434/v1` | Optional | e.g. `llama3.2` |
| Other OpenAI-compatible | Your endpoint (LM Studio, a company gateway…) | As needed | What your endpoint expects |

Leave the base URL blank to use the provider's standard address. The key field has **Show/Hide** so you can check or copy it. The settings page shows **Ready** once everything needed is filled in.

For OpenCode, the apps send the headers OpenCode expects (an app user agent and a stable `x-opencode-session` per conversation).

## Privacy

Each request sends **the forecast digests of your saved places** (current conditions, hourly and daily forecast, active alerts) and your question, to the provider you chose, with your key. Nothing else is sent: no location history, no other files, no telemetry. With Ollama, nothing leaves your computer.

## Using the assistant

The window has your provider and model at the top (with a green dot when ready), starter questions when empty, chat bubbles with formatted replies (bold, lists), a typing indicator, and a message box: **Enter** sends, **Shift+Enter** adds a line. **New chat** starts over; **Settings** jumps to the AI settings.

> AI can make mistakes. Check important details against official forecasts, and during severe weather follow warnings from your national weather service and local authorities.
