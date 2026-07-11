# OpenLibertas — Agent Guide

A terminal-based AI chat client in Rust. Multi-provider, MCP tools, session management.

## Workspace Structure

```
crates/
  openlibertas-core/    # Shared: backend, config, commands, domain, store, search, export, mcp
  openlibertas-tui/     # Binary: ratatui frontend, event loop (main entrypoint)
  openlibertas-server/  # HTTP server variant
personas/               # Agent persona markdown files (*.md)
```

## Commands

```bash
# Build release binary
cargo build --release
# Binary: target/release/openlibertas

# Run tests (all crates)
cargo test

# Run just the TUI crate
cargo run -p openlibertas-tui

# Run just core tests
cargo test -p openlibertas-core
```

No formatter/linter configs checked in — use `cargo fmt` and `cargo clippy` with defaults.

## Architecture Notes

### Single Backend, All Providers

`OpenAiBackend` (in `core/src/backend.rs`) is the **only** backend implementation. It speaks generic OpenAI-compatible API and is reused for every provider (OpenAI, Anthropic, Kimi, Ollama, llama.cpp, etc.). Differences between providers are handled via config (`base_url`, `api_key`, `supports_tools`), not code.

If adding a new local inference method (vLLM, TGI, etc.), you do **not** need a new backend — just point `base_url` at it.

### Provider Config

Config lives at `~/.config/openlibertas/config.toml`:

```toml
[[providers]]
name = "llamacpp"
base_url = "http://localhost:8080/v1"
api_key = "sk-local"
enabled = true
supports_tools = true   # set false for models that don't support function calling
```

The `supports_tools` flag is critical — when `false`, the backend strips the `tools` array from requests, avoiding errors from basic GGUFs.

### Permission Configuration

Two top-level config fields control the permission system:

```toml
permission_policy = "ask"        # "ask", "auto_approve", or "deny"
auto_approve_tools = ["read_file", "search"]
```

- `permission_policy` — `"ask"` opens the interactive panel for dangerous tools (shell, file writes/edits/deletes). `"auto_approve"` allows all dangerous tools. `"deny"` blocks them.
- `auto_approve_tools` — tool names that never require approval, even in `"ask"` mode.

Session-level approvals granted with `s` in the permission panel are persisted to the data directory and survive app restarts.

### Event Loop

`main.rs` runs a tokio async loop:
1. Draw UI (`ui::draw`)
2. Wait for events (120ms timeout) via `EventStream`
3. Handle input key events → update `App` state
4. Handle `ChatEvent::Text/ToolCall/Done/Error` → update chat messages
5. Agent loop triggers automatically when `agents.status == Idle` and user sends a message

### Agent Loop

When agents are enabled (`/agents` panel → Status: Enabled):
1. User message sent with agent system prompt (based on selected persona)
2. LLM streams response
3. If tool calls emitted → execute via MCP → append tool results → send again
4. Repeat until no tool calls or max iterations reached

Personas are loaded dynamically from `personas/*.md` at runtime. Each file: first line is `# Name`, rest is the system prompt. The 15 built-in personas are comprehensive multi-agent scaffolding prompts:

- **Orchestrator** — Central coordinator with intent classification and delegation tables
- **Coding** — Implementation specialist with verification requirements
- **Research** — Systematic investigator with source evaluation
- **Creative** — Creative strategist with ideation workflows
- **Captain** — Task coordinator with dependency mapping
- **Artisan** — Deep autonomous worker with end-to-end implementation
- **Sage** — Read-only consultant with structured review criteria
- **Pathfinder** — External search with source hierarchy
- **Seeker** — Codebase explorer with symbol tracing
- **Witness** — Document analyst with precise observation
- **Strategist** — Pre-planning consultant with risk assessment
- **Examiner** — Plan reviewer with severity-based findings
- **Steward** — Task tracker with progress monitoring
- **Visionary** — Strategic architect with roadmap design
- **Operative** — Precise executor with verification workflows

### Tool Calling Flow

```
backend.chat() sends ChatRequest with tools[]
  → SSE stream returns content + tool_call deltas
  → finish_stream() attaches tool_calls to last Assistant message
  → pending_tool_calls executed via MCP client
  → build_tool_result_messages() appends role:"tool" messages
  → Next chat() includes tool results
```

Critical: `finish_stream()` **must** attach tool_calls to the Assistant message in chat history, or the API will reject the subsequent tool result messages as orphaned.

### UI Code Organization

All drawing is in `ui.rs`. Key functions:
- `draw()` — routes to `draw_models()` or `draw_chat()`
- `draw_chat()` — splits screen into header, messages, input; renders popups as overlays
- Popups: tools, MCP, sessions, themes, help, agents, command palette
- Each popup uses `centered_rect()` + `Clear` widget + themed `Block`

### State Management

`App` struct holds all mutable state:
- `chat: ChatState` — messages, scroll, streaming, spinner
- `input: InputState` — buffer, cursor, history, autocomplete
- `models: ModelState` — available models, current selection
- `panels: PanelState` — boolean flags for each popup
- `agents: AgentState` — status, iteration count, persona
- `mcp: McpState` — client, available tools, pending calls, results

## Adding Features

### New Slash Command

1. Add to `SLASH_COMMANDS` array in `core/src/commands.rs`
2. Add variant to `SlashCommand` enum
3. Add parse arm in `SlashCommand::parse()`
4. Add description in `command_description()`
5. Handle in `App::execute_slash_command()` in `app.rs`
6. Add to help panel text in `ui.rs`
7. Update README.md command table

### New Backend Capability

Since there's only one backend, capabilities are config-driven:
- Add field to `Provider` struct in `core/src/config.rs`
- Thread through `BackendRegistry::new()` to `OpenAiBackend::with_tools()`
- Use in `OpenAiBackend::chat()` or `OpenAiBackend::fetch_models()`

### New Persona

Create `personas/<name>.md`:
```markdown
# MyPersona

System prompt content here...
```

No code changes needed — loaded dynamically at runtime.

## Testing

- Unit tests are inline in each module under `#[cfg(test)]`
- No integration tests — the app is entirely terminal-driven
- To test tool calling: configure a provider with `supports_tools = true` and MCP servers in `~/.config/opencode/mcp.json`
- To test agents: enable agent mode, send a message that should trigger tool use

## Common Gotchas

- **Spinner frame race**: `advance_spinner()` increments `spinner_frame` on every draw cycle. Don't use time-based thresholds — the 120ms event loop timeout makes them unreliable.
- **Panel stacking**: Multiple panels can be open simultaneously (`show_tools && show_help`). Esc closes one at a time in priority order (help → themes → palette → tools/mcp/sessions → model screen).
- **Tool call dedup**: `pending_tool_calls` is cleared before each new request and after `ChatEvent::Done`. If not cleared, the same tool gets executed multiple times.
- **Persona loading fails silently**: If `personas/` dir is missing or unreadable, agents fall back to a hardcoded default prompt. Check directory permissions if personas don't appear in the agents panel.

## Deployment

No Dagger module or recognized deployment configuration was found.

General redeploy process:

1. Commit and push changes to the default branch.
2. Trigger the relevant CI/CD pipeline or run the documented deploy command.
3. If the project is served via GitHub Pages, the site redeploys automatically after the push.
