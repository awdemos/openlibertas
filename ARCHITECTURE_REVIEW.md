# OpenLibertas Comprehensive Architecture Review

**Date:** 2025-05-10
**Total LOC:** 11,949
**Test Status:** 137 passed, 0 failed

---

## 1. Test Coverage

### Current State: GOOD in core, ABSENT in UI layer

| Component | Test Modules | Coverage |
|-----------|-------------|----------|
| Core library | 26 files with `#[cfg(test)]` | Strong |
| TUI layer | 3 files (app.rs, markdown.rs, theme.rs) | Minimal |
| Server binary | 0 | None |
| Voice module | audio.rs, client.rs, mod.rs, state.rs | Good |
| Tools | git, shell, tmux, search, web, filesystem, meta | Good |
| Backend | registry.rs, backend.rs | Basic |

**Findings:**
- 137 tests all pass (0.45s runtime)
- Only 2 functions use `test_` naming convention — most tests are in integration-style blocks
- **Critical gap:** No TUI rendering tests, no event loop tests, no end-to-end tests
- The `#[cfg(test)]` blocks are present in 33 modules but actual test function count is low

**Recommendations:**
1. Add `crossterm`/`ratatui` test harness for UI snapshot testing
2. Add integration tests for the full chat flow (mock backend + event stream)
3. Add property-based tests for message serialization/deserialization
4. Consider `insta` crate for UI snapshot regression testing

---

## 2. Benchmarking

### Current State: NON-EXISTENT

No `benches/` directory, no `criterion` dependency, no performance baselines.

**Recommendations:**
1. Add `criterion` benchmarks for:
   - Message rendering pipeline (markdown → ratatui spans)
   - Backend response streaming throughput
   - Conversation context compaction
   - Tool call parsing (the tool_parser.rs regex logic)
2. Add memory usage benchmarks for long conversation sessions (context window growth)
3. Profile voice STT/TTS latency — ElevenLabs round-trip is a UX bottleneck

---

## 3. Deployment & Distribution

### Current State: MANUAL ONLY

| Aspect | Status |
|--------|--------|
| CI/CD (GitHub Actions) | ❌ None |
| Automated testing on PR | ❌ None |
| Release builds / binaries | ❌ None |
| Docker image | ❌ None |
| Cross-compilation | ❌ None |
| crates.io publish | ❌ None |
| Homebrew / AUR / etc | ❌ None |

**Findings:**
- Users must build from source (`cargo build --release`)
- No `.github/workflows/` directory at all
- Release profile enables LTO and opt-level=3 (good)
- Binary name `openlibertas` is clean

**Recommendations:**
1. **GitHub Actions workflow** (`.github/workflows/ci.yml`):
   ```yaml
   jobs:
     test:
       strategy:
         matrix:
           os: [ubuntu-latest, macos-latest]
           rust: [stable]
       steps:
         - uses: actions/checkout@v4
         - uses: dtolnay/rust-toolchain@stable
         - run: cargo test --all-features
         - run: cargo clippy --all-targets --all-features -- -D warnings
         - run: cargo fmt --check
   ```
2. **Release workflow** with `cargo-dist` for cross-platform binaries
3. **Dockerfile** for the server variant:
   ```dockerfile
   FROM rust:1.85 AS builder
   WORKDIR /app
   COPY . .
   RUN cargo build --release -p openlibertas-server
   FROM debian:bookworm-slim
   COPY --from=builder /app/target/release/openlibertas-server /usr/local/bin/
   EXPOSE 3000
   CMD ["openlibertas-server"]
   ```
4. Consider `cargo-release` for version bump automation

---

## 4. Documentation

### Current State: ADEQUATE for users, SPARSE for developers

| Aspect | Status |
|--------|--------|
| README.md | ✅ Comprehensive (324 lines) |
| Architecture diagram | ✅ ASCII tree in README |
| AGENTS.md | ✅ Exists for agent context |
| Inline doc comments (`///`) | 108 total — sparse in large files |
| Inline comments (`//`) | 160 total — reasonable |
| API docs (`cargo doc`) | Not checked |
| CHANGELOG.md | ❌ None |
| CONTRIBUTING.md | ❌ None |
| Persona documentation | ❌ No per-persona docs beyond the .md files |

**Findings:**
- `ui.rs` (1,577 lines): Only ~15 doc comments
- `app.rs` (1,097 lines): Only ~10 doc comments
- `engine.rs` (790 lines): Only ~5 doc comments
- Most complex logic (tool calling loop, event handling, markdown rendering) lacks module-level docs

**Recommendations:**
1. Add module-level documentation (`//!`) to all major modules explaining purpose and invariants
2. Document the agent loop state machine (Disabled → Idle → Active)
3. Document the voice recording lifecycle with state diagram
4. Add CHANGELOG.md following Keep a Changelog format
5. Add architecture decision records (ADRs) for key decisions:
   - Single backend (OpenAiBackend) design
   - Event loop architecture (120ms timeout)
   - MCP config from opencode.json (external dependency)
   - Voice via ElevenLabs (vs local Whisper)

---

## 5. Code Quality

### Current State: GOOD, with minor issues

**Metrics:**
- `unwrap()`: 92 calls
- `expect()`: 2 calls
- `panic!`: 0 calls
- `unsafe`: 1 block (Ctrl+Z signal handling — justified)
- `TODO/FIXME/HACK/XXX`: 0 comments
- Clippy warnings: 8 (all in TUI, all auto-fixable)
- Format violations: Present in backend.rs, server/main.rs

**unwrap() Distribution:**
| Module | Count | Risk Level |
|--------|-------|-----------|
| backend.rs | 10 | Medium — HTTP response parsing |
| voice/ | 8 | Medium — audio I/O |
| server/main.rs | 2 | Low — config defaults |
| TUI main.rs | 0 | ✅ None |
| app.rs | ~5 | Low — UI state |

**Findings:**
- The `server_url` field in `ElevenLabsClient` is dead code (clippy warning, ignored)
- Server's `ok()` helper uses `unwrap()` on JSON serialization — could fail on non-Serialize types
- Voice STT endpoint in server uses `unwrap()` on multipart parsing
- No `#![deny(clippy::unwrap_used)]` lint enabled

**Recommendations:**
1. Enable stricter lints in `lib.rs`:
   ```rust
   #![warn(clippy::unwrap_used)]
   #![warn(clippy::expect_used)]
   #![warn(clippy::panic)]
   ```
2. Replace `unwrap()` in server with proper error handling
3. Add `cargo fmt --check` to CI
4. Fix the 8 clippy warnings (auto-fixable)
5. Remove dead `server_url` field or implement custom server support

---

## 6. Security

### Current State: BASIC, needs hardening

| Aspect | Status |
|--------|--------|
| Dependency audit (cargo-audit) | ❌ Not run |
| Secret handling (API keys) | ⚠️ Stored in plaintext TOML |
| MCP server execution | ⚠️ Runs arbitrary commands from config |
| Shell tool execution | ⚠️ Direct command execution |
| Input sanitization | ✅ Basic |
| HTTPS/TLS | ✅ reqwest uses rustls |

**Findings:**
- API keys stored in `~/.config/openlibertas/config.toml` (standard but plaintext)
- MCP config loaded from `~/.config/opencode/opencode.json` — external dependency
- Shell tool allows arbitrary command execution (intentional but dangerous)
- No sandboxing for tool execution
- No audit log for agent actions

**Recommendations:**
1. Add `cargo-audit` to CI pipeline
2. Implement config file permissions check (warn if world-readable)
3. Add confirmation prompts for destructive tools (shell, filesystem delete)
4. Add agent action audit log
5. Consider `seccomp` or namespace sandboxing for shell tool
6. Implement config encryption at rest for API keys (keyring crate)

---

## 7. Architecture Observations

### Strengths

1. **Clean crate separation**: Core/TUI/Server with clear boundaries
2. **Single backend design**: `OpenAiBackend` reused for all providers via config
3. **Event-driven TUI**: Unified `EventStream` merges keyboard, chat, MCP, and timer events
4. **Tool registry abstraction**: MCP tools and built-in tools unified behind same interface
5. **Config-driven provider system**: Adding new providers requires zero code changes
6. **Persona system**: Runtime-loaded markdown files, no recompilation needed

### Concerns

1. **TUI layer bloat**: `ui.rs` (1,577 lines), `app.rs` (1,097 lines), `main.rs` (949 lines) — 3,623 lines for UI alone. Consider splitting:
   - `ui.rs` → `ui/chat.rs`, `ui/panels.rs`, `ui/widgets.rs`
   - `app.rs` → `app/state.rs`, `app/commands.rs`, `app/input.rs`

2. **Server is WIP**: 626 lines but missing critical features:
   - No authentication
   - No rate limiting
   - No WebSocket support (SSE only)
   - No conversation persistence
   - Tool calls not executed (returns tool_call data but doesn't run them)
   - Missing streaming endpoint error handling

3. **MCP dependency on opencode**: Tight coupling to `~/.config/opencode/opencode.json` makes standalone usage harder. Consider native MCP config.

4. **Voice module complexity**: 397 lines in mod.rs + 469 in audio.rs + 163 in client.rs = 1,029 lines. The push-to-talk vs toggle logic has been a source of bugs (multiple fixes in recent sessions).

5. **No metrics/observability**: No tracing in core, no request timing, no error rate tracking.

---

## 8. Performance

### Current State: UNMEASURED

**Potential bottlenecks (unverified):**
1. Message rendering: `ui.rs` re-renders entire message list every frame
2. Markdown parsing: `pulldown-cmark` called on every draw for visible messages
3. Model list fetching: Every 30 seconds unconditionally
4. Context compaction: Linear scan of messages on every build
5. Voice audio: Real-time encoding WAV on stop, blocking the async task

**Recommendations:**
1. Profile render loop with `ratatui`'s debug features
2. Cache parsed markdown spans per message (invalidate on content change only)
3. Add request coalescing for model fetching
4. Benchmark context compaction with 100+ message conversations
5. Move WAV encoding to a blocking task pool (`tokio::task::spawn_blocking`)

---

## 9. Prioritized Action Plan

### Immediate (this week)
1. ✅ Fix formatting (`cargo fmt`)
2. ✅ Fix clippy warnings (8 auto-fixable)
3. ✅ Add `cargo clippy -- -D warnings` to build workflow

### Short-term (this month)
4. Set up GitHub Actions CI (test + clippy + fmt)
5. Add CHANGELOG.md and start versioning
6. Add module-level documentation to `ui.rs`, `app.rs`, `engine.rs`
7. Run `cargo audit` and address any vulnerabilities
8. Add UI layer unit tests (at minimum, test message rendering)

### Medium-term (next quarter)
9. Refactor TUI crate into submodules (`ui/`, `app/`)
10. Complete server implementation (auth, WebSocket, tool execution)
11. Add performance benchmarks
12. Implement config encryption for API keys
13. Add agent action audit log
14. Cross-platform release automation with `cargo-dist`

### Long-term
15. Consider alternative STT providers (local Whisper, faster-whisper)
16. Add plugin system for custom tools
17. WebAssembly target for browser-based UI
18. Telemetry/usage analytics (opt-in)

---

## Appendix: File Size Heatmap

| File | Lines | Notes |
|------|-------|-------|
| `tui/src/ui.rs` | 1,577 | **Split me** — all rendering logic |
| `tui/src/app.rs` | 1,097 | **Split me** — state + commands + input |
| `core/src/engine.rs` | 790 | ChatEngine + AgentState + states |
| `core/src/conversation.rs` | 709 | Message building + compaction |
| `server/src/main.rs` | 626 | WIP — needs completion |
| `core/src/mcp.rs` | 559 | MCP client + JSON-RPC |
| `core/src/commands.rs` | 553 | Slash commands |
| `tui/src/main.rs` | 949 | Event loop |
| `core/src/backend.rs` | 455 | OpenAiBackend + SSE |
| `core/src/voice/mod.rs` | 397 | Voice lifecycle |
| `core/src/voice/audio.rs` | 469 | Audio I/O + device mgmt |
| `core/src/domain.rs` | 330 | Core types |
| `core/src/tool_registry.rs` | 281 | Tool execution |

**Files under 100 lines:** 18 modules — well-factored
**Files over 500 lines:** 7 modules — candidates for splitting

---

*Review generated by rust-dev skill. Run `cargo test` to verify: 137 passing.*
