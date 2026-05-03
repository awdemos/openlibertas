# OpenLibertas

A terminal-based AI chat client with multi-provider support, MCP tools, session management, and intuitive TUI navigation.

## Features

- **Multi-Provider Support** - Connect to any OpenAI-compatible endpoint (llama.cpp, Ollama, vLLM, Kimi, GLM) for local or remote inference
- **MCP Tool Integration** - Discover and use tools from Model Context Protocol servers (web search, browser automation, docker, etc.)
- **Autonomous Agents** - 15 agent personas for different tasks: coding, research, orchestration, code review, and more. Agents use tools autonomously to complete multi-step workflows.
- **Session Management** - Save, load, and manage chat sessions with auto-generated names (model + timestamp)
- **File Context** - Attach files inline with `@path/to/file` syntax
- **Agent Personas** - 15 specialized agent personalities loaded from `personas/*.md` files
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
supports_tools = true

[[providers]]
name = "llamacpp"
base_url = "http://localhost:8080/v1"
api_key = "sk-local"
enabled = true
supports_tools = true

max_tokens = 4096
```

Environment variables override config values:
- `OPENLIBERTAS_URL` - Override base URL
- `OPENLIBERTAS_API_KEY` - Override API key
- `OPENLIBERTAS_MODEL` - Default model
- `OPENLIBERTAS_MAX_TOKENS` - Max tokens per request

## Local Models with llama.cpp

OpenLibertas works with any OpenAI-compatible API, including [llama.cpp server](https://github.com/ggml-org/llama.cpp/blob/master/examples/server/README.md).

### Setup

1. **Download a GGUF model** (Qwen3, Llama3, etc.)
2. **Start llama.cpp server with tool support:**

```bash
./llama-server \
  -m Qwen3-8B-Instruct-Q6_K.gguf \
  --host 0.0.0.0 --port 8080 \
  --jinja \
  --ctx-size 8192 \
  --n-gpu-layers -1
```

- `--jinja` is **required** for tool-aware templating
- For Qwen models, download the `chat_template.jinja` from HuggingFace if needed
- See [llama.cpp function calling docs](https://github.com/ggml-org/llama.cpp/blob/master/docs/function_calling.md)

3. **Add to your config:**

```toml
[[providers]]
name = "llamacpp"
base_url = "http://localhost:8080/v1"
api_key = "sk-local"
enabled = true
supports_tools = true
```

### `supports_tools`

Set `supports_tools = false` for providers that don't support function calling (basic GGUFs without `--jinja`, older Ollama models, etc.). When false, OpenLibertas will **not** send the `tools` array, avoiding errors.

```toml
[[providers]]
name = "basic-local"
base_url = "http://localhost:11434/v1"
enabled = true
supports_tools = false
```

## Agents

OpenLibertas includes a multi-agent system with 15 specialized personas. Each persona has a distinct system prompt that shapes how the agent approaches tasks.

### How It Works

1. Open the agent panel with `/agents`
2. Select a persona (General, Coding, Research, Captain, Artisan, etc.)
3. Enable agent status
4. Send your request normally
5. The LLM will use tools as needed, analyze results, and continue until the task is complete
6. Status shows in the header: `[Agents: ● 3/10 Persona]`
7. Maximum 10 iterations per task (configurable, prevents infinite loops)
8. Cancel anytime with `Esc`

### Agent Personas

| Persona | Role | Best For |
|---------|------|----------|
| **General** | Default assistant | Everyday questions, general tasks |
| **Coding** | Software engineer | Code review, debugging, implementation |
| **Research** | Research assistant | Deep investigation, analysis, summaries |
| **Creative** | Creative writer | Brainstorming, writing, design |
| **Captain** | Orchestrator | Complex multi-step projects, delegation |
| **Artisan** | Deep worker | Focused implementation, detailed tasks |
| **Sage** | Consultant | Code review, architecture advice, critique |
| **Pathfinder** | External search | Documentation lookup, API research |
| **Seeker** | Code explorer | Navigating large codebases, finding patterns |
| **Witness** | Document analyst | PDF/image analysis, visual verification |
| **Strategist** | Planner | Pre-implementation planning, risk analysis |
| **Examiner** | Reviewer | Plan validation, quality assurance |
| **Steward** | Task manager | Todo tracking, progress monitoring |
| **Visionary** | Architect | Long-term planning, technical strategy |
| **Operative** | Executor | Well-defined tasks, precise implementation |

Personas are loaded from `personas/*.md` files at runtime. Each file's first line is the name (`# Name`), and the rest is the system prompt. Add your own by creating a new `.md` file in the `personas/` directory.

### Agent Use Cases

- **Research**: "Search for recent Rust async runtime benchmarks, then summarize the findings"
- **File Operations**: "Read Cargo.toml, check the dependencies, then suggest updates"
- **Multi-step Workflows**: "Find all TODO comments in the codebase, then create a summary document"
- **Debugging**: "Check the last 50 lines of the application log, identify any errors, and suggest fixes"
- **Code Review** (Sage): "Review this PR for security issues and performance bottlenecks"
- **Planning** (Strategist): "Plan the migration from sync to async for this module"

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
| `/help` | Show keyboard shortcuts panel |
| `/tools` | Toggle MCP tools panel |
| `/model <name>` | Switch to specific model |
| `/models` | Open model selection menu |
| `/clear` | Clear current conversation |
| `/new` | Start new empty session |
| `/save [name]` | Save session (auto-names if no name given) |
| `/load <name>` | Load saved session |
| `/sessions` | Open session manager popup |
| `/delete <name>` | Delete saved session |
| `/export <file>` | Export chat to markdown |
| `/mcp` | Toggle MCP servers panel |
| `/agents` | Open agent configuration panel |
| `/poke` | Toggle poke mode (send [POKE] on click) |
| `/quit` | Quit |

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
[LLAMACPP]
  Qwen3-8B-Instruct-Q6_K
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
│   │   │   ├── prompt.rs         # Agent persona system prompt loader
│   │   │   ├── search.rs         # In-conversation text search
│   │   │   ├── state.rs          # Last model persistence
│   │   │   └── store.rs          # Session save/load (JSON)
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
├── personas/                     # Agent persona markdown files
│   ├── general.md
│   ├── coding.md
│   ├── research.md
│   ├── creative.md
│   ├── captain.md
│   ├── artisan.md
│   ├── sage.md
│   ├── pathfinder.md
│   ├── seeker.md
│   ├── witness.md
│   ├── strategist.md
│   ├── examiner.md
│   ├── steward.md
│   ├── visionary.md
│   └── operative.md
└── README.md
```

## License

MIT
