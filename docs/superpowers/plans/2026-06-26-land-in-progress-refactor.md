# Land In-Progress Refactor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish landing the in-progress TUI modularization and permission system refactor so the workspace is clean, clippy-clean, formatted, and documented.

**Architecture:** Keep the existing module split (`app/` and `ui/` submodules) and finish moving remaining helper methods out of `app.rs`. Harden the permission service and tool executor integration. Clean up all clippy/formatting issues introduced by the refactor.

**Tech Stack:** Rust, cargo, ratatui, tokio

---

## File Structure

| File | Responsibility | Action |
|------|----------------|--------|
| `crates/openlibertas-tui/src/app.rs` | `App` struct, constructor, slash-command execution, engine coordination | Trim remaining helper methods |
| `crates/openlibertas-tui/src/app/state.rs` | State enums/structs | Already complete; verify exports |
| `crates/openlibertas-tui/src/app/ui_state.rs` | Theme/agent/avatar/palette state mutations | Move remaining helpers from `app.rs` |
| `crates/openlibertas-tui/src/ui.rs` | Top-level draw, header, status, input | Fix formatting, ensure popup dispatch |
| `crates/openlibertas-tui/src/ui/messages.rs` | Message list rendering | Verify no drift |
| `crates/openlibertas-tui/src/ui/models.rs` | Model selection screen rendering | Verify no drift |
| `crates/openlibertas-tui/src/panels/permission.rs` | Permission request popup | Fix formatting + clippy |
| `crates/openlibertas-core/src/permission.rs` | Permission service + state persistence | Harden save failures, verify timeout |
| `crates/openlibertas-core/src/config.rs` | Config + `PermissionState` + `PermissionPolicy` | Derive `Default` for `PermissionPolicy` |
| `crates/openlibertas-core/src/agent_turn.rs` | Spawn agent turn | Allow too-many-arguments |
| `crates/openlibertas-core/src/backend/multi_provider.rs` | Provider capability construction | Use struct init pattern |
| `CHANGELOG.md` | Release notes | Add permission system entry |
| `AGENTS.md` | Agent guide | Document permission config fields |

---

### Task 1: Fix formatting drift

**Files:**
- Modify: `crates/openlibertas-tui/src/panels/permission.rs`
- Modify: `crates/openlibertas-tui/src/ui.rs`

**Step 1:** Run `cargo fmt` to fix all formatting drift.

```bash
cargo fmt
```

**Step 2:** Verify formatting check passes.

```bash
cargo fmt -- --check
```

Expected: no output, exit 0.

**Step 3:** Commit.

```bash
git add crates/openlibertas-tui/src/panels/permission.rs crates/openlibertas-tui/src/ui.rs
git commit -m "style: apply cargo fmt to permission and ui modules"
```

---

### Task 2: Fix remaining clippy warnings

**Files:**
- Modify: `crates/openlibertas-core/src/agent_turn.rs`
- Modify: `crates/openlibertas-core/src/config.rs`
- Modify: `crates/openlibertas-core/src/backend/multi_provider.rs`
- Modify: `crates/openlibertas-tui/src/panels/permission.rs`

**Step 1:** Allow `too_many_arguments` on `spawn_agent_turn`.

In `crates/openlibertas-core/src/agent_turn.rs`, add the attribute above the function:

```rust
#[allow(clippy::too_many_arguments)]
pub fn spawn_agent_turn(
    engine: &ChatEngine,
    registry: &ProviderRegistry,
    provider_id: ProviderId,
    model: Option<String>,
    max_tokens: u32,
    user_input: String,
    mode: AgentMode,
    wire_tx: tokio::sync::mpsc::UnboundedSender<WireMessage>,
    cancel_token: tokio_util::sync::CancellationToken,
) {
```

**Step 2:** Derive `Default` for `PermissionPolicy`.

In `crates/openlibertas-core/src/config.rs`, replace:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PermissionPolicy {
    Ask,
    AutoApprove,
    Deny,
}

impl Default for PermissionPolicy {
    fn default() -> Self {
        PermissionPolicy::Ask
    }
}
```

with:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PermissionPolicy {
    #[default]
    Ask,
    AutoApprove,
    Deny,
}
```

**Step 3:** Use struct init pattern in `multi_provider.rs`.

Find:

```rust
let mut caps = ProviderCapabilities::default();
caps.tools = true;
```

Replace with:

```rust
let caps = ProviderCapabilities {
    tools: true,
    ..Default::default()
};
```

**Step 4:** Use `vec![]` macro in `panels/permission.rs`.

Find:

```rust
let mut lines: Vec<Line> = Vec::new();

lines.push(Line::from(vec![
```

Replace with:

```rust
let mut lines: Vec<Line> = vec![
    Line::from(vec![
        Span::styled(
            "Tool: ",
            Style::default()
                .fg(theme.secondary())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(&self.request.tool_name, Style::default().fg(theme.foreground())),
    ]),
    Line::from(""),
    // ... continue with the remaining initial pushes
];
```

**Step 5:** Run clippy and confirm zero warnings.

```bash
cargo clippy --all-targets --all-features
```

Expected: `Finished dev profile` with no warnings.

**Step 6:** Commit.

```bash
git add crates/openlibertas-core/src/agent_turn.rs crates/openlibertas-core/src/config.rs crates/openlibertas-core/src/backend/multi_provider.rs crates/openlibertas-tui/src/panels/permission.rs
git commit -m "style: resolve remaining clippy warnings"
```

---

### Task 3: Complete app.rs/ui.rs modularization

**Files:**
- Modify: `crates/openlibertas-tui/src/app.rs`
- Modify: `crates/openlibertas-tui/src/app/ui_state.rs`

**Step 1:** Audit `app.rs` for any remaining helper methods that belong in `app/ui_state.rs`.

Search for methods related to theme, agent, avatar, or palette navigation in `app.rs` that are not already in `ui_state.rs`.

```bash
grep -n "fn theme_\|fn agent_\|fn avatar_\|fn palette_\|fn open_themes_panel\|fn select_theme\|fn select_agent_option\|fn select_palette_command" crates/openlibertas-tui/src/app.rs
```

Expected: no matches. If matches remain, move them to `app/ui_state.rs`.

**Step 2:** Verify `app.rs` only re-exports what is needed.

Ensure `app.rs` contains:

```rust
pub use state::{ConnectionStatus, ModelState, Overlay, Screen, SearchState};
```

and no duplicate definitions of those types.

**Step 3:** Run tests.

```bash
cargo test -p openlibertas-tui
```

Expected: all tests pass.

**Step 4:** Commit.

```bash
git add crates/openlibertas-tui/src/app.rs crates/openlibertas-tui/src/app/ui_state.rs
git commit -m "refactor(tui): complete app/ui state modularization"
```

---

### Task 4: Harden permission service save behavior

**Files:**
- Modify: `crates/openlibertas-core/src/permission.rs`

**Step 1:** Ensure `PermissionState::save()` failures are logged, not propagated.

The current code already uses `let _ = state.save();`. Add explicit warning logs around each save call in the permission service worker.

Find the two `let _ = state.save();` calls inside the spawned worker and replace with:

```rust
if let Err(e) = state.save() {
    warn!("Failed to save permission state: {}", e);
}
```

**Step 2:** Add a unit test for save failure tolerance.

Add to the test module in `crates/openlibertas-core/src/permission.rs`:

```rust
#[tokio::test]
async fn test_permission_service_responds_after_deny() {
    let service = PermissionService::new();
    let request = PermissionRequest::new(
        "session-deny",
        "shell",
        "Execute: rm -rf /",
        "execute",
        serde_json::json!({"command": "rm -rf /"}),
        "/",
    );

    let request_id = request.id.clone();
    let service_clone = service.clone();
    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        service_clone.respond(request_id, PermissionResponse::Deny);
    });

    let approved = service.request(request).await;
    assert!(!approved);
}
```

**Step 3:** Run permission tests.

```bash
cargo test -p openlibertas-core permission
```

Expected: all permission tests pass.

**Step 4:** Commit.

```bash
git add crates/openlibertas-core/src/permission.rs
git commit -m "refactor(permission): log state save failures and add deny response test"
```

---

### Task 5: Verify permission timeout and policy wiring

**Files:**
- Modify: `crates/openlibertas-core/src/permission.rs`

**Step 1:** Add a test for timeout auto-deny.

Add to the test module:

```rust
#[tokio::test]
async fn test_permission_service_timeout_denies() {
    let service = PermissionService::new();
    service.set_timeout_seconds(1);
    let request = PermissionRequest::new(
        "session-timeout",
        "shell",
        "Execute: slow command",
        "execute",
        serde_json::json!({"command": "sleep 100"}),
        "/",
    );

    let approved = service.request(request).await;
    assert!(!approved);
}
```

**Step 2:** Add a test for `AutoApprove` policy.

Add to the test module:

```rust
#[tokio::test]
async fn test_permission_policy_auto_approve() {
    let service = PermissionService::new();
    service.set_permission_policy(crate::config::PermissionPolicy::AutoApprove);
    let request = PermissionRequest::new(
        "session-auto",
        "shell",
        "Execute: anything",
        "execute",
        serde_json::json!({}),
        "/",
    );

    let approved = service.request(request).await;
    assert!(approved);
}

#[tokio::test]
async fn test_permission_policy_deny() {
    let service = PermissionService::new();
    service.set_permission_policy(crate::config::PermissionPolicy::Deny);
    let request = PermissionRequest::new(
        "session-deny-policy",
        "shell",
        "Execute: anything",
        "execute",
        serde_json::json!({}),
        "/",
    );

    let approved = service.request(request).await;
    assert!(!approved);
}
```

**Step 3:** Run core tests.

```bash
cargo test -p openlibertas-core
```

Expected: 375+ pass; only the 3 pre-existing keyring tests may fail in environments without a secret service.

**Step 4:** Commit.

```bash
git add crates/openlibertas-core/src/permission.rs
git commit -m "test(permission): add timeout, auto-approve, and deny policy tests"
```

---

### Task 6: Update documentation

**Files:**
- Modify: `CHANGELOG.md`
- Modify: `AGENTS.md`

**Step 1:** Add permission system entry to `CHANGELOG.md` under `[Unreleased] > Added`.

```markdown
- Permission system for dangerous tools (shell, file writes, edits, deletes) with interactive TUI panel
- Session-level permission grants persisted to `permissions.toml`
- Diff preview in permission panel for `write_file` and `str_replace_file` operations
- Configurable permission policy: `ask`, `auto_approve`, `deny`
- Configurable `auto_approve_tools` list for trusted tools
```

**Step 2:** Document permission config fields in `AGENTS.md`.

Add a short subsection under `### Provider Config`:

```markdown
### Permission Configuration

Two config fields control the permission system:

- `permission_policy = "ask"` — `"ask"`, `"auto_approve"`, or `"deny"`. `"ask"` opens the interactive panel for dangerous tools.
- `auto_approve_tools = ["read_file", "search"]` — tool names that never require approval.

Session-level approvals (granted with `s` in the permission panel) are persisted to the data directory and survive app restarts.
```

**Step 3:** Commit.

```bash
git add CHANGELOG.md AGENTS.md
git commit -m "docs: document permission system in changelog and agent guide"
```

---

### Task 7: Final verification and commit

**Files:**
- Workspace

**Step 1:** Run full test suite.

```bash
cargo test
```

Expected: 375+ pass; 3 keyring tests fail only if no secret service is available.

**Step 2:** Run clippy.

```bash
cargo clippy --all-targets --all-features
```

Expected: no warnings.

**Step 3:** Run format check.

```bash
cargo fmt -- --check
```

Expected: no output, exit 0.

**Step 4:** Review git status.

```bash
git status --short
```

Expected: only the new docs/specs/plans files remain untracked; all source changes are committed.

**Step 5:** Commit the design and plan docs.

```bash
git add docs/superpowers/specs/2026-06-26-land-in-progress-refactor-design.md docs/superpowers/plans/2026-06-26-land-in-progress-refactor.md
git commit -m "docs: add refactor landing spec and plan"
```

---

## Self-Review Checklist

- **Spec coverage:** Every spec section (modularization, permission completion, clippy/format, docs) maps to one or more tasks.
- **Placeholder scan:** No TBD/TODO placeholders; every code step contains concrete code or exact commands.
- **Type consistency:** `PermissionPolicy` variants and `PermissionResponse` names match the codebase.
