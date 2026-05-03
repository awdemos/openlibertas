# OpenLibertas

A terminal-based AI chat client with multi-provider support, MCP tools, session management, and intuitive TUI navigation.

## Features

- **Multi-Provider Support** - Connect to local (Ollama, lm-studio) and remote (OpenAI, Anthropic, Kimi, GLM) LLM providers simultaneously
- **MCP Tool Integration** - Discover and use tools from Model Context Protocol servers (web search, browser automation, docker, etc.)
- **Session Management** - Save, load, and manage chat sessions with auto-generated names (model + timestamp)
- **File Context** - Attach files inline with `@path/to/file` syntax
- **Provider-Aware Prompts** - Automatic system prompt selection based on provider (Kimi, Anthropic, GPT, local)
- **Input History** - Navigate previous inputs with Up/Down arrows
- **Mouse Support** - Scroll chat output with mouse wheel
- **Slash Commands** - Tab-autocompleted commands for all operations
- **Connection Status** - Real-time indicator showing backend health
- **Auto-Scroll** - Follows streaming responses, stops when you scroll up

## Installation

```bash
cargo install --path .
```

Or build from source:

```bash
git clone https://github.com/openlibertas/openlibertas
cd openlibertas
cargo build --release
```

Binary will be at `target/release/openlibertas`.

## Configuration

Config file: `~/.config/openlibertas/config.toml`

```toml
[[providers]]
name = "local"
base_url = "http://127.0.0.1:11435/v1"
api_key = "sk-local"
enabled = true

[[providers]]
name = "kimi"
base_url = "https://api.kimi.com/coding/v1"
api_key = "your-api-key"
enabled = true

[[providers]]
name = "openai"
base_url = "https://api.openai.com/v1"
api_key = "your-api-key"
enabled = true

max_tokens = 4096
```

Environment variables override config values:
- `OPENLIBERTAS_URL` - Override base URL
- `OPENLIBERTAS_API_KEY` - Override API key
- `OPENLIBERTAS_MODEL` - Default model
- `OPENLIBERTAS_MAX_TOKENS` - Max tokens per request

## MCP Servers

OpenLibertas reads MCP server configuration from `~/.config/opencode/opencode.json`:

```json
{
  "mcp": {
    "websearch": {
      "type": "remote",
      "url": "https://mcp.exa.ai/mcp?tools=web_search_exa",
      "enabled": true
    },
    "docker": {
      "type": "local",
      "command": ["npx", "-y", "@swartdraak/docker-mcp-server"],
      "enabled": true
    }
  }
}
```

## Usage

Launch: `openlibertas`

Startup skips the model menu if you have a previous session. Goes straight to chat.

### Chat Input

- **Type normally** - Send messages to the AI
- **`@path/to/file`** - Attach file content inline (e.g., `@src/main.rs`)
- **`/`** - Shows all slash commands
- **`/s<Tab>`** - Autocomplete to `/save`, `/sessions`, etc.
- **Up/Down** - Navigate input history
- **PageUp/PageDown** - Scroll chat output
- **Mouse scroll** - Scroll chat output
- **Enter** - Send message
- **Esc** - Close panels / go to model selection
- **Ctrl+Q** - Quit

### Slash Commands

| Command | Description |
|---------|-------------|
| `/help`, `/h` | Show available commands |
| `/tools`, `/t` | Toggle MCP tools panel |
| `/model <name>`, `/m` | Switch to specific model |
| `/models` | Open model selection menu |
| `/clear`, `/c` | Clear current conversation |
| `/new`, `/n` | Start new empty session |
| `/save [name]`, `/s` | Save session (auto-names if no name given) |
| `/load <name>`, `/l` | Load saved session |
| `/sessions` | Open session manager popup |
| `/delete <name>`, `/d` | Delete saved session |
| `/export <file>`, `/e` | Export chat to markdown |
| `/mcp` | Toggle MCP servers panel |
| `/quit`, `/q` | Quit |

### Session Names

Auto-generated: `{model}-{YYYY-MM-DD-HHMM}`

Example: `qwen2.5-coder-14b-2025-05-02-0015`

Saved to: `~/.local/share/openlibertas/*.json`

### Model Selection Screen

- **↑/↓** - Navigate models
- **Enter** - Select model
- **q** - Quit

Models from all configured providers are listed with provider prefix:
```
[local] qwen2.5-coder-14b
[kimi] kimi-for-coding
[openai] gpt-4
```

## Architecture

```
openlibertas/
├── src/
│   ├── main.rs       # Event loop, runtime wiring
│   ├── app.rs        # App state, slash commands, history
│   ├── backend.rs    # HTTP client, SSE streaming, tool calls
│   ├── config.rs     # Config loading (TOML + env)
│   ├── event.rs      # Unified event stream (input + chat)
│   ├── mcp.rs        # MCP client, JSON-RPC, tool discovery
│   ├── prompt.rs     # Provider-specific system prompts
│   ├── state.rs      # Last model persistence
│   ├── store.rs      # Session save/load (JSON)
│   ├── terminal.rs   # TerminalGuard RAII, panic hook
│   └── ui.rs         # Ratatui rendering
├── prompts/          # Provider-specific system prompts
│   ├── default.txt
│   ├── anthropic.txt
│   ├── kimi.txt
│   ├── gpt.txt
│   └── local.txt
└── Cargo.toml
```

## License

MIT
