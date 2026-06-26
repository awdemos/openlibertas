# Design: Land the In-Progress Refactor

## Context

The OpenLibertas codebase currently has an uncommitted work-in-progress that adds two major pieces of work:

1. **TUI modularization** — `crates/openlibertas-tui/src/app.rs` and `src/ui.rs` are being split into focused submodules (`app/`, `ui/`).
2. **Permission system** — A new permission approval flow for dangerous tools (shell, file writes, edits, etc.) with session persistence, diff previews, and interactive TUI panel.

The workspace compiles and the existing test suite passes except for three credential/keyring tests that fail because no secret service is available in this environment. A handful of `cargo clippy` warnings and `cargo fmt` drift remain.

## Goals

- Finish splitting `app.rs`/`ui.rs` into focused submodules so each file has one clear responsibility.
- Complete the permission system so it is wired end-to-end: tool executor → permission service → TUI panel → user response → persisted session grants.
- Get the workspace back to green:
  - `cargo test` passes with no new failures.
  - `cargo clippy --all-targets --all-features` produces no warnings.
  - `cargo fmt --check` passes.
- Update `CHANGELOG.md` and help text to reflect the new permission feature.

## Non-Goals

- No unrelated refactoring of `multi_provider.rs`, `session/mod.rs`, or other large modules.
- No new user-facing features beyond what is already in flight.
- No changes to the credential/keyring implementation. The three failing tests are an environment issue, not a code bug.

## Section 2: App/UI Modularization

### Module Layout

The new module layout is already started. Each submodule owns one concern:

**`crates/openlibertas-tui/src/app/`**

- `state.rs` — `App` state enums/structs: `Screen`, `Overlay`, `ConnectionStatus`, `ModelState`, `SearchState`.
- `ui_state.rs` — UI-only state mutations: theme selection, agent/avatar menu navigation, command palette.
- `models.rs` — Model selection helpers (`select_next_model`, `select_prev_model`, `select_current_model`).
- `search_nav.rs` — In-conversation search navigation (`search_next`, `search_prev`, `scroll_to_match`).
- `sessions.rs` — Session list/filter/load/delete helpers.
- `mcp.rs` — MCP panel state helpers.
- `completion.rs` — Autocomplete cycling helpers.

**`crates/openlibertas-tui/src/ui/`**

- `messages.rs` — Message list rendering, scrolling, timestamps, tool-call rendering.
- `models.rs` — Model selection screen rendering.
- `ui.rs` — Top-level `draw()`, chat layout, header/status/input, popup dispatch.

### Cleanup

- Remove any leftover methods from `app.rs` that now belong in a submodule.
- Run `cargo fmt` and fix the formatting drift in `permission.rs` and `ui.rs`.
- Fix the `vec_init_then_push` clippy warning in `panels/permission.rs`.

## Section 3: Permission System Completion

The permission plumbing is mostly in place. Remaining work is verification and minor hardening:

- **Tool executor** (`agent_tools/executor.rs`) creates `PermissionRequest` with `session_id`, diff preview, etc.
- **Permission service** (`core/src/permission.rs`) persists session grants to `permissions.toml` and supports `AutoApprove`/`Deny` policies and timeouts.
- **TUI wiring** (`main.rs`) receives `Event::PermissionRequest`, shows `Overlay::Permission`, and handles `a`/`s`/`d` responses plus `Esc` → deny.
- **Panel** (`panels/permission.rs`) renders tool/action/command/parameters/path/diff and key hints.

### Hardening

- Ensure `PermissionState::save()` failures are logged but do not crash the app.
- Verify that the permission timeout does not block the UI event loop longer than necessary.
- Confirm that denying a permission request surfaces a clear skipped/error message in the chat history.

## Section 4: Quality Gates

Before committing:

- `cargo test` — must pass (the 3 keyring failures are environment-only and already exist; no new failures).
- `cargo clippy --all-targets --all-features` — must produce no warnings.
- `cargo fmt --check` — must pass.

### Known Clippy Fixes

- `agent_turn.rs`: `too_many_arguments` — suppress with `#[allow(clippy::too_many_arguments)]` since it is an existing external-facing signature.
- `config.rs`: derive `Default` for `PermissionPolicy`.
- `multi_provider.rs`: use struct init instead of field reassignment.
- `panels/permission.rs`: use `vec![]` macro.

## Section 5: Documentation

- Update `CHANGELOG.md` under `[Unreleased]` with the permission system entry.
- Update `/help` text in `ui.rs` if new permission keys need documenting.
- Add a short note in `AGENTS.md` about `permission_policy` and `auto_approve_tools` config fields.

## Success Criteria

- All uncommitted changes are committed with a clean git status.
- `cargo test`, `cargo clippy --all-targets --all-features`, and `cargo fmt --check` pass.
- The permission panel renders correctly and `a`/`s`/`d`/`Esc` behave as documented.
- `CHANGELOG.md` and `AGENTS.md` are updated.
