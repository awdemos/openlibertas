# OpenLibertas

A terminal-based AI chat client with multi-provider support, MCP tools, session management, and intuitive TUI navigation.

## Features

- **Multi-Provider Support** - Connect to local (Ollama, lm-studio) and remote (OpenAI, Anthropic, Kimi, GLM) LLM providers simultaneously
- **MCP Tool Integration** - Discover and use tools from Model Context Protocol servers (web search, browser automation, docker, etc.)
- **Autonomous Agents** - Enable agent mode and the LLM will use tools repeatedly to complete multi-step tasks automatically (up to 10 iterations)
- **Session Management** - Save, load, and manage chat sessions with auto-generated names (model + timestamp)
- **File Context** - Attach files inline with `@path/to/file` syntax
- **Provider-Aware Prompts** - Automatic system prompt selection based on provider (Kimi, Anthropic, GPT, local)
- **Input History** - Navigate previous inputs with Up/Down arrows
- **Mouse Support** - Scroll chat output with mouse wheel
- **Slash Commands** - Tab-autocompleted commands for all operations
- **Connection Status** - Real-time indicator showing backend health
- **Auto-Scroll** - Follows streaming responses, stops when you scroll up
- **Keyboard Help** - Press `F1` or `?` for a searchable help panel

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

### Non-Interactive Install (for agents/automation)

```bash
# Clone and build without prompts
git clone https://github.com/openlibertas/openlibertas.git /tmp/openlibertas
cd /tmp/openlibertas
cargo build --release 2>&1 | tail -5

# Install binary to a location in PATH
mkdir -p ~/.local/bin
cp target/release/openlibertas ~/.local/bin/

# Create config directory
mkdir -p ~/.config/openlibertas

# Write minimal config (edit API keys before use)
cat > ~/.config/openlibertas/config.toml << 'EOF'
[[providers]]
name = "local"
base_url = "http://127.0.0.1:11435/v1"
api_key = "sk-local"
enabled = true

max_tokens = 4096
EOF

echo "Install complete. Run: openlibertas"
```

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

## Agents

Agent mode enables the LLM to use tools autonomously to complete multi-step tasks.

### How It Works

1. Enable agent mode with `/agents` (toggle)
2. Send your request normally
3. The LLM will use tools as needed, analyze results, and continue until the task is complete
4. Status shows in the header: `[Agents: ● 3/10]`
5. Maximum 10 iterations per task (prevents infinite loops)
6. Cancel anytime with `Esc`

### Agent Use Cases

- **Research**: "Search for recent Rust async runtime benchmarks, then summarize the findings"
- **File Operations**: "Read Cargo.toml, check the dependencies, then suggest updates"
- **Multi-step Workflows**: "Find all TODO comments in the codebase, then create a summary document"
- **Debugging**: "Check the last 50 lines of the application log, identify any errors, and suggest fixes"

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
- **Esc** - Close panels / cancel streaming / go to model selection
- **F1** or **?** - Toggle help panel
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
| `/agents` | Toggle autonomous agent mode |
| `/poke`, `/p` | Toggle poke mode (send [POKE] on click) |
| `/quit`, `/q` | Quit |

### Session Names

Auto-generated: `{model}-{YYYY-MM-DD-HHMM}`

Example: `qwen2.5-coder-14b-2025-05-02-0015`

Saved to: `~/.local/share/openlibertas/*.json`

### Model Selection Screen

- **↑/↓** - Navigate models
- **Enter** - Select model
- **q** - Quit

Models from all configured providers are grouped by provider:
```
[LOCAL]
  qwen2.5-coder-14b
  llama3.1-8b
[OPENAI]
  gpt-4
  gpt-3.5-turbo
```

## Architecture

```
openlibertas/
├── Cargo.toml                    # Workspace manifest
├── Cargo.lock                    # Locked dependencies
├── crates/
│   ├── openlibertas-core/        # Shared library
│   │   ├── src/
│   │   │   ├── backend.rs        # HTTP client, SSE streaming, tool calls
│   │   │   ├── backend/
│   │   │   │   └── registry.rs   # Multi-provider backend registry
│   │   │   ├── commands.rs       # Slash command parsing and execution
│   │   │   ├── config.rs         # Config loading (TOML + env vars)
│   │   │   ├── conversation.rs   # Message building, file context, tool results
│   │   │   ├── domain.rs         # Core types (Message, Role, ChatEvent, etc.)
│   │   │   ├── export.rs         # Session export (Markdown, JSON, Plaintext)
│   │   │   ├── lib.rs            # Library exports
│   │   │   ├── mcp.rs            # MCP client, JSON-RPC, tool discovery
│   │   │   ├── prompt.rs         # Provider-specific system prompt manager
│   │   │   ├── search.rs         # In-conversation text search
│   │   │   ├── state.rs          # Last model persistence
│   │   │   └── store.rs          # Session save/load (JSON)
│   │   └── prompts/              # Provider-specific system prompts
│   │       ├── default.txt
│   │       ├── anthropic.txt
│   │       ├── kimi.txt
│   │       ├── gpt.txt
│   │       └── local.txt
│   ├── openlibertas-tui/         # Terminal UI application
│   │   └── src/
│   │       ├── main.rs           # Event loop, runtime wiring, agent loop
│   │       ├── app.rs            # App state, slash commands, input handling
│   │       ├── event.rs          # Unified event stream (input + chat + MCP)
│   │       ├── markdown.rs       # Markdown renderer for chat output
│   │       ├── terminal.rs       # TerminalGuard RAII, panic hook
│   │       ├── theme.rs          # Color theme system
│   │       └── ui.rs             # Ratatui rendering, panels, help
│   └── openlibertas-server/      # HTTP server (WIP)
│       └── src/
│           └── main.rs           # Axum server for remote access
└── README.md
```

## License

MIT
