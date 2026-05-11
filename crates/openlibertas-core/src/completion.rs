//! Context-aware completion system for OpenLibertas input.
//!
//! Provides completions for:
//! - Slash commands (`/help`, `/model`, etc.)
//! - File paths (`@path/to/file`)
//! - Model names (`/model <name>`)

use crate::commands::{command_description, SLASH_COMMANDS};
use crate::domain::Model;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The type of completion being requested.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CompletionType {
    /// Completing a slash command (e.g. `/he` → `/help`)
    SlashCommand,
    /// Completing a file path after `@` (e.g. `@src/ma` → `@src/main.rs`)
    FilePath,
    /// Completing a model name after `/model ` (e.g. `/model qwe` → `/model qwen`)
    Model,
}

/// A single completion candidate.
#[derive(Debug, Clone, PartialEq)]
pub struct CompletionItem {
    /// The text to insert when accepted.
    pub label: String,
    /// Optional description shown next to the label.
    pub description: String,
    /// The kind of completion (used for styling).
    pub kind: CompletionType,
}

impl CompletionItem {
    pub fn new(
        label: impl Into<String>,
        description: impl Into<String>,
        kind: CompletionType,
    ) -> Self {
        Self {
            label: label.into(),
            description: description.into(),
            kind,
        }
    }
}

/// Cached file list with TTL to avoid repeated directory scans.
struct FileCache {
    files: Vec<String>,
    cached_at: Instant,
    cwd: String,
}

impl FileCache {
    const TTL: Duration = Duration::from_secs(2);

    fn is_fresh(&self) -> bool {
        self.cached_at.elapsed() < Self::TTL
    }
}

/// Engine that produces completion candidates based on input context.
pub struct CompletionEngine {
    file_cache: Arc<Mutex<Option<FileCache>>>,
}

impl Default for CompletionEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl CompletionEngine {
    pub fn new() -> Self {
        Self {
            file_cache: Arc::new(Mutex::new(None)),
        }
    }

    /// Determine what kind of completion, if any, is appropriate at the given
    /// cursor position inside `buffer`.
    pub fn detect_completion_type(buffer: &str, cursor_pos: usize) -> Option<CompletionType> {
        let pos = cursor_pos.min(buffer.len());
        let before_cursor = &buffer[..pos];

        // File path completion: typing @something
        // Find the last @ before cursor that isn't followed by whitespace
        if let Some(at_pos) = before_cursor.rfind('@') {
            let after_at = &before_cursor[at_pos + 1..];
            // Make sure there's no space between @ and cursor
            if !after_at.contains(' ') && !after_at.contains('\t') {
                return Some(CompletionType::FilePath);
            }
        }

        // Model completion: buffer starts with "/model "
        if before_cursor.starts_with("/model ") && !before_cursor.contains("  ") {
            return Some(CompletionType::Model);
        }

        // Slash command completion: buffer starts with / and no space yet
        if before_cursor.starts_with('/') && !before_cursor.contains(' ') {
            return Some(CompletionType::SlashCommand);
        }

        None
    }

    /// Get the current prefix/filter for the detected completion type.
    ///
    /// Returns the substring between the trigger character and the cursor.
    pub fn completion_prefix(buffer: &str, cursor_pos: usize, kind: CompletionType) -> String {
        let pos = cursor_pos.min(buffer.len());
        let before_cursor = &buffer[..pos];

        match kind {
            CompletionType::FilePath => {
                if let Some(at_pos) = before_cursor.rfind('@') {
                    before_cursor[at_pos + 1..].to_string()
                } else {
                    String::new()
                }
            }
            CompletionType::Model => {
                // Everything after "/model "
                if before_cursor.starts_with("/model ") {
                    before_cursor[7..].to_string()
                } else {
                    String::new()
                }
            }
            CompletionType::SlashCommand => before_cursor.to_string(),
        }
    }

    /// Compute the replacement range (start_byte, end_byte) for a completion.
    ///
    /// For file paths, this replaces from `@` to cursor.
    /// For slash commands, replaces from start to cursor.
    /// For models, replaces from after `/model ` to cursor.
    pub fn replacement_range(
        buffer: &str,
        cursor_pos: usize,
        kind: CompletionType,
    ) -> (usize, usize) {
        let pos = cursor_pos.min(buffer.len());
        let before_cursor = &buffer[..pos];

        match kind {
            CompletionType::FilePath => {
                let start = before_cursor.rfind('@').unwrap_or(0);
                (start + 1, pos)
            }
            CompletionType::Model => {
                let start = if buffer.starts_with("/model ") {
                    7
                } else {
                    pos
                };
                (start, pos)
            }
            CompletionType::SlashCommand => (0, pos),
        }
    }

    /// Build completion items for the given context.
    pub fn complete(
        &self,
        buffer: &str,
        cursor_pos: usize,
        models: &[Model],
    ) -> Vec<CompletionItem> {
        let kind = match Self::detect_completion_type(buffer, cursor_pos) {
            Some(k) => k,
            None => return Vec::new(),
        };

        let prefix = Self::completion_prefix(buffer, cursor_pos, kind);
        let prefix_lower = prefix.to_lowercase();

        match kind {
            CompletionType::SlashCommand => self.complete_slash_commands(&prefix_lower),
            CompletionType::FilePath => self.complete_file_paths(&prefix_lower),
            CompletionType::Model => self.complete_models(&prefix_lower, models),
        }
    }

    fn complete_slash_commands(&self, prefix: &str) -> Vec<CompletionItem> {
        SLASH_COMMANDS
            .iter()
            .filter(|cmd| cmd.to_lowercase().starts_with(prefix))
            .map(|cmd| {
                CompletionItem::new(
                    cmd.to_string(),
                    command_description(cmd).to_string(),
                    CompletionType::SlashCommand,
                )
            })
            .collect()
    }

    fn complete_file_paths(&self, prefix: &str) -> Vec<CompletionItem> {
        let files = self.list_files();
        let prefix_lower = prefix.to_lowercase();

        files
            .into_iter()
            .filter(|f| f.to_lowercase().starts_with(&prefix_lower))
            .take(50)
            .map(|f| CompletionItem::new(f.clone(), String::new(), CompletionType::FilePath))
            .collect()
    }

    fn complete_models(&self, prefix: &str, models: &[Model]) -> Vec<CompletionItem> {
        let prefix_lower = prefix.to_lowercase();

        models
            .iter()
            .filter(|m| m.id.to_lowercase().contains(&prefix_lower))
            .take(20)
            .map(|m| {
                let desc = format!(
                    "{} — {}",
                    m.provider.as_str(),
                    if m.supports_tools {
                        "tools"
                    } else {
                        "no tools"
                    }
                );
                CompletionItem::new(m.id.clone(), desc, CompletionType::Model)
            })
            .collect()
    }

    /// List files from the current working directory.
    /// Uses `git ls-files` if inside a git repo, otherwise walks the directory.
    /// Results are cached for 2 seconds.
    fn list_files(&self) -> Vec<String> {
        let mut cache = self.file_cache.lock().unwrap_or_else(|e| e.into_inner());

        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        if let Some(ref c) = *cache {
            if c.is_fresh() && c.cwd == cwd {
                return c.files.clone();
            }
        }

        let files = Self::scan_files();

        *cache = Some(FileCache {
            files: files.clone(),
            cached_at: Instant::now(),
            cwd,
        });

        files
    }

    fn scan_files() -> Vec<String> {
        // Try git ls-files first
        if let Ok(output) = std::process::Command::new("git")
            .args(["ls-files", "--cached", "--others", "--exclude-standard"])
            .output()
        {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let mut files: Vec<String> = stdout
                    .lines()
                    .filter(|l| !l.is_empty())
                    .map(|l| l.to_string())
                    .collect();
                files.sort();
                return files;
            }
        }

        // Fallback: directory walk (non-recursive for speed, just current dir)
        let mut files = Vec::new();
        if let Ok(entries) = std::fs::read_dir(".") {
            for entry in entries.filter_map(Result::ok) {
                if let Ok(meta) = entry.metadata() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if meta.is_dir() {
                        files.push(format!("{}/", name));
                    } else {
                        files.push(name);
                    }
                }
            }
        }
        files.sort();
        files
    }
}

/// Update the input buffer with an accepted completion.
pub fn accept_completion(
    buffer: &mut String,
    cursor_pos: &mut usize,
    item: &CompletionItem,
) -> bool {
    let kind = item.kind;
    let (start, end) = CompletionEngine::replacement_range(buffer, *cursor_pos, kind);

    // Replace the range with the completion label
    let before = if start <= buffer.len() {
        &buffer[..start]
    } else {
        &buffer[..*cursor_pos]
    };
    let after = if end <= buffer.len() {
        &buffer[end..]
    } else {
        ""
    };

    let mut new_buffer = String::with_capacity(before.len() + item.label.len() + after.len() + 2);
    new_buffer.push_str(before);

    // For file paths, insert the path and leave cursor at end of path
    // For slash commands, insert command and optionally add space
    // For models, insert model name
    new_buffer.push_str(&item.label);
    new_buffer.push_str(after);

    // For slash commands that don't take arguments, we might want to auto-submit,
    // but we leave that to the caller.
    if kind == CompletionType::SlashCommand {
        // Add a trailing space if the command takes arguments
        let takes_args = matches!(
            item.label.as_str(),
            "/model"
                | "/theme"
                | "/temp"
                | "/save"
                | "/load"
                | "/delete"
                | "/export"
                | "/search"
                | "/edit"
                | "/remove"
                | "/title"
                | "/voice_device"
                | "/avatar"
        );
        if takes_args && !new_buffer.ends_with(' ') {
            new_buffer.push(' ');
        }
    }

    let new_cursor = start + item.label.len();
    *buffer = new_buffer;
    *cursor_pos = new_cursor;

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_slash_command() {
        assert_eq!(
            CompletionEngine::detect_completion_type("/he", 3),
            Some(CompletionType::SlashCommand)
        );
    }

    #[test]
    fn detect_file_path() {
        assert_eq!(
            CompletionEngine::detect_completion_type("check @src/ma", 13),
            Some(CompletionType::FilePath)
        );
    }

    #[test]
    fn detect_model() {
        assert_eq!(
            CompletionEngine::detect_completion_type("/model qwe", 10),
            Some(CompletionType::Model)
        );
    }

    #[test]
    fn no_completion_for_plain_text() {
        assert_eq!(
            CompletionEngine::detect_completion_type("hello world", 11),
            None
        );
    }

    #[test]
    fn no_file_completion_after_space() {
        // "@foo bar" — cursor after bar, the @foo part has a space so no completion
        assert_eq!(
            CompletionEngine::detect_completion_type("@foo bar", 8),
            None
        );
    }

    #[test]
    fn slash_command_prefix() {
        assert_eq!(
            CompletionEngine::completion_prefix("/he", 3, CompletionType::SlashCommand),
            "/he"
        );
    }

    #[test]
    fn file_path_prefix() {
        assert_eq!(
            CompletionEngine::completion_prefix("@src/ma", 7, CompletionType::FilePath),
            "src/ma"
        );
    }

    #[test]
    fn model_prefix() {
        assert_eq!(
            CompletionEngine::completion_prefix("/model qwe", 10, CompletionType::Model),
            "qwe"
        );
    }

    #[test]
    fn accept_slash_command_completion() {
        let mut buffer = "/he".to_string();
        let mut cursor = 3;
        let item = CompletionItem::new("/help", "Show help", CompletionType::SlashCommand);
        accept_completion(&mut buffer, &mut cursor, &item);
        assert_eq!(buffer, "/help");
        assert_eq!(cursor, 5);
    }

    #[test]
    fn accept_file_path_completion() {
        let mut buffer = "check @src/".to_string();
        let mut cursor = 11;
        let item = CompletionItem::new("src/main.rs", "", CompletionType::FilePath);
        accept_completion(&mut buffer, &mut cursor, &item);
        assert_eq!(buffer, "check @src/main.rs");
        assert_eq!(cursor, 18);
    }

    #[test]
    fn accept_model_completion() {
        let mut buffer = "/model qwe".to_string();
        let mut cursor = 10;
        let item = CompletionItem::new("qwen3-8b", "local", CompletionType::Model);
        accept_completion(&mut buffer, &mut cursor, &item);
        assert_eq!(buffer, "/model qwen3-8b");
        assert_eq!(cursor, 15);
    }

    #[test]
    fn complete_slash_commands_filters_by_prefix() {
        let engine = CompletionEngine::new();
        let items = engine.complete("/s", 2, &[]);
        assert!(items.iter().any(|i| i.label == "/save"));
        assert!(items.iter().any(|i| i.label == "/sessions"));
        assert!(!items.iter().any(|i| i.label == "/help"));
    }

    #[test]
    fn complete_models_filters_by_prefix() {
        let engine = CompletionEngine::new();
        let models = vec![
            Model {
                id: "qwen3-8b".to_string(),
                provider: crate::domain::ProviderId::new("local"),
                supports_tools: true,
                supports_voice: false,
                local: true,
            },
            Model {
                id: "gpt-4".to_string(),
                provider: crate::domain::ProviderId::new("openai"),
                supports_tools: true,
                supports_voice: false,
                local: false,
            },
        ];
        let items = engine.complete("/model qwe", 10, &models);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "qwen3-8b");
    }
}
