# OpenLibertas Domain Glossary

> Living document. Terms are resolved during grill-with-docs sessions.

## Core Concepts

### Session
A saved chat interaction between the user and an AI. Persisted to disk as JSON. The user-facing term for what was previously called "conversation" in some parts of the codebase.

- **User commands:** `/save`, `/load`, `/sessions`, `/delete`
- **UI text:** "Session saved", "Load session"
- **Technical type:** `Session` (renamed from `Conversation`)
- **Store:** `SessionStore` (renamed from `ConversationStore`)
- **Module paths:** Still use `crate::conversation` for backward compatibility

---

### Wire Protocol
The in-process communication channel between the agent core and the UI layer. Implemented as unbounded `mpsc` in `soul.rs`. Previously there was a competing bounded-channel design in `wire.rs` which has been removed.

- **Canonical type:** `soul::WireMessage` (unbounded mpsc)
- **Removed:** `wire.rs` bounded channel design (unused, naming collision)

---

### Agent Mode Status
The state of the agent mode toggle in the UI. Tracks whether the user has enabled agents, and if so, whether the agent is idle or actively processing.

- **Type:** `engine::AgentModeStatus`
- **Variants:** `Disabled`, `Idle`, `Active`
- **Formerly:** `AgentStatus` (conflicted with runtime status)

### Agent Turn State
The runtime lifecycle state of a single agent turn. Tracks whether the turn is running, waiting for tool results, stopped, or errored.

- **Type:** `soul::AgentTurnState`
- **Variants:** `Idle`, `Running { turn_id }`, `ProcessingTools { turn_id }`, `Stopped`, `Error(String)`
- **Formerly:** `AgentStatus` (conflicted with UI toggle status)

---

### Chat Agent
The runtime implementation that executes a single turn: sends the request to the backend, streams responses, executes tool calls, and reports results via the wire protocol.

- **Type:** `soul::ChatAgent`
- **Note:** A competing unused design (`runtime::ChatAgent` with `Agent` trait) was removed. The TUI creates `soul::ChatAgent` directly.

### Persona
A system prompt loaded from `personas/*.md` that defines an agent's behavior and role. The technical term for what the UI calls an "agent mode".

- **User-facing:** "agent" (e.g., "Coding agent", "Research agent")
- **Technical:** `persona` field on `AgentState`, `personas/` directory
- **Files:** `personas/*.md` — first line is `# Name`, rest is the system prompt

---

### Provider
The abstraction for LLM calls. All LLM requests pass through a Provider implementation. The trait defines `chat()`, `fetch_models()`, and `health_check()`.

- **Type:** `backend::Provider` (trait)
- **Implementations:** `OpenAiBackend`, `MultiProvider`
- **Registry:** `ProviderRegistry` maps `ProviderConfig` → `dyn Provider`
- **Formerly:** `Backend` trait (conflicted with user's intent)

### Provider Config
User-configured LLM endpoint settings loaded from `config.toml`.

- **Type:** `config::ProviderConfig` (struct)
- **Fields:** `name`, `base_url`, `api_key`, `enabled`, `kind`, `capabilities`, `tool_format`
- **Formerly:** `Provider` (collided with the trait name)

### Backend Event
What the LLM backend returns during a streaming chat: text deltas, tool calls, completion, or errors.

- **Type:** `domain::BackendEvent` (enum)
- **Variants:** `Text`, `ToolCall`, `Done`, `Error`
- **Direction:** Backend → Core
- **Formerly:** `ChatEvent` (ambiguous — sounded like it was from the chat layer, not the backend)

### Wire Message
What the agent core sends to the UI layer over the in-process wire protocol. Mirrors `BackendEvent` but represents the core→UI direction.

- **Type:** `soul::WireMessage` (enum)
- **Variants:** `Text`, `ToolCall`, `Done`, `Error`
- **Direction:** Core → UI
- **Note:** `Event::Agent(WireMessage)` is how the TUI receives these.

### MCP Tool
An external capability exposed via an MCP server. The actual implementation lives outside the agent (in a separate process or server).

- **Type:** `mcp::McpTool` (struct)
- **Configuration:** `~/.config/opencode/mcp.json`
- **Examples:** `read_file`, `web_search`, `bash`

### Tool Definition
The JSON schema sent to the LLM describing what tools are available. The LLM uses this to decide which tool to call.

- **Type:** `domain::ToolDefinition` (struct)
- **Contains:** `name`, `description`, `parameters` (JSON schema)
- **Note:** This is what the LLM sees — not the tool implementation itself.

### Tool Format
How tool definitions are serialized in the chat request. Different LLMs expect different formats.

- **Type:** `tool_format::ToolFormat` (enum)
- **Variants:** `Native` (OpenAI format), `Xml` (llama.cpp XML), `None` (tools disabled)

### Agent Tools
The built-in tool implementations and execution framework that the agent uses. Formerly called just "tools", which was ambiguous.

- **Module:** `agent_tools` (formerly `tools`)
- **Contains:** Tool implementations (`filesystem`, `shell`, `git`, `search`, `web`), execution framework (`executor`, `assembler`, `registry`)
- **Note:** These are the agent's built-in capabilities, distinct from MCP tools which are external.

---

## Status

This glossary is being populated. Terms are added as they are resolved.
