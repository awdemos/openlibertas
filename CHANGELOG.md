# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Permission system for dangerous tools (shell, file writes, edits, deletes) with interactive TUI panel
- Session-level permission grants persisted to `permissions.toml`
- Diff preview in permission panel for `write_file` and `str_replace_file` operations
- Configurable permission policy: `ask`, `auto_approve`, `deny`
- Configurable `auto_approve_tools` list for trusted tools
- Real-time audio level meter during voice recording (peak amplitude visualization)
- Push-to-talk vs toggle mode separation for voice (Ctrl+Space vs Space)
- Input device enumeration and selection (`/voice_device` slash command)
- Audio diagnostics: `AudioStats` with peak, RMS, duration, silence detection
- Stereo-to-mono conversion for STT compatibility
- Audio normalization (boost to 75% peak) for quiet microphones
- Mouse capture toggle (`/mouse`) with Shift+bypass for terminal selection
- Double-click and right-click to copy messages via OSC 52
- Timestamps on all chat messages and exports (Markdown, Plaintext, JSON)
- Agent personas loaded dynamically from `personas/*.md` files (15 personas)
- MCP client initialization from `~/.config/opencode/opencode.json`
- Voice mode with ElevenLabs STT/TTS integration
- Connection status polling (every 30s) with live indicator
- Input history navigation (Up/Down arrows)
- File context attachment with `@path/to/file` syntax
- Session save/load/delete with auto-generated names
- Chat export in Markdown, JSON, and Plaintext formats
- Slash command system with Tab autocompletion
- Multi-provider backend registry (OpenAI-compatible API)
- Tool calling support with MCP and built-in tools
- Agent loop with max iteration limit and cancel support
- In-conversation text search
- Auto-scroll with manual override

### Changed

- Unified all provider prompts to be terminal-aware (concise, markdown, tool-aware)
- Improved `tool_instructions()` to include actual JSON parameter schemas
- Lowered silence threshold from 0.005 to 0.001 for quiet microphone support
- Removed `buffer.is_empty()` restriction on Ctrl+Space push-to-talk
- Fixed stream/recorder teardown order to prevent race conditions
- Reduced audio level meter multiplier from ×20 to ×2
- Enhanced error handling in tool registry and MCP client
- Optimized release profile with LTO and opt-level=3

### Fixed

- **MCP Initialization**: Fixed illegal trailing comma in `opencode.json` parse failure
- **Voice Key Repeat**: Added `voice_key_held` debounce flag to prevent toggle loop
- **Voice Status**: Cleared `voice_status` when recording auto-cancels at max duration
- **Empty Messages**: Guard to block Enter key during voice recording
- **Text Wrapping**: Added `.wrap(Wrap { trim: true })` and fixed `wrap_markdown`
- **UI Redundancy**: Removed duplicate model name/base URL from header and welcome screen
- **Header Overflow**: Width-aware truncation with ellipsis to prevent text wrapping
- **Terminal Corruption**: Removed all `eprintln!` from voice module (alternate screen conflict)
- **Bad Gateway**: Fixed config pointing to wrong port (11435 → 11436)
- **Mouse Scroll**: Enabled mouse capture by default to prevent passthrough
- **Clippy**: Resolved all warnings across all crates
- **Dead Code**: Removed unused `server_url` field from `ElevenLabsClient`
- **Server Unwraps**: Replaced `unwrap()` with `unwrap_or_else` in SSE streaming

### Security

- Added strict lint configuration (`#![warn(clippy::unwrap_used)]`) to all crates
- Applied `cargo fmt` across entire codebase
- Fixed manual checked division in audio stats computation
- Replaced field reassignment with struct init pattern in tests

## [0.1.0] - 2025-05-10

### Added

- Initial release of OpenLibertas terminal-based AI chat client
- Multi-provider support (OpenAI-compatible endpoints)
- Ratatui-based terminal UI with async event loop
- Session management with JSON persistence
- Basic slash commands (`/clear`, `/save`, `/load`, `/quit`)
- Model selection screen with provider grouping
- Real-time streaming response display

[Unreleased]: https://github.com/awdemos/openlibertas/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/awdemos/openlibertas/releases/tag/v0.1.0
