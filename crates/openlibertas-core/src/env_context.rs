use std::process::Command;

/// Runtime environment context injected into system prompts.
/// Mirrors kimi-cli's BuiltinSystemPromptArgs for workspace awareness.
#[derive(Debug, Clone)]
pub struct EnvContext {
    /// Absolute path of the current working directory.
    pub work_dir: String,
    /// Directory listing of the current working directory (capped).
    pub work_dir_ls: String,
    /// Operating system kind, e.g. 'linux', 'macos', 'windows'.
    pub os: String,
    /// Shell executable path, e.g. '/bin/bash'.
    pub shell: String,
}

impl EnvContext {
    /// Detect the current environment.
    pub fn detect() -> Self {
        let work_dir = std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "unknown".to_string());

        let work_dir_ls = Self::list_directory(&work_dir);

        let os = std::env::consts::OS.to_string();

        let shell = std::env::var("SHELL")
            .or_else(|_| std::env::var("ComSpec"))
            .unwrap_or_else(|_| "unknown".to_string());

        Self {
            work_dir,
            work_dir_ls,
            os,
            shell,
        }
    }

    /// Format as a markdown section suitable for appending to a system prompt.
    pub fn to_prompt_section(&self) -> String {
        format!(
            "\n\n## Environment Context\n\n\
             - **Working directory:** `{}`\n\
             - **Shell:** `{}`\n\
             - **OS:** `{}`\n\n\
             ### Working Directory Contents\n\n\
             ```\n{}```",
            self.work_dir, self.shell, self.os, self.work_dir_ls
        )
    }

    /// List directory contents, capped to ~40 lines to avoid prompt bloat.
    fn list_directory(path: &str) -> String {
        let output = Command::new("ls")
            .args(["-la", "--color=never", path])
            .output();

        match output {
            Ok(o) if o.status.success() => {
                let text = String::from_utf8_lossy(&o.stdout);
                let lines: Vec<&str> = text.lines().collect();
                if lines.len() > 40 {
                    let mut truncated: Vec<&str> = lines.into_iter().take(40).collect();
                    truncated.push("... (truncated)");
                    truncated.join("\n")
                } else {
                    text.to_string()
                }
            }
            _ => "[directory listing unavailable]".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_context_detects_current_dir() {
        let ctx = EnvContext::detect();
        assert!(!ctx.work_dir.is_empty());
        assert!(!ctx.os.is_empty());
    }

    #[test]
    fn to_prompt_section_contains_fields() {
        let ctx = EnvContext {
            work_dir: "/path/to/project".to_string(),
            work_dir_ls: "total 0".to_string(),
            os: "linux".to_string(),
            shell: "/bin/bash".to_string(),
        };
        let section = ctx.to_prompt_section();
        assert!(section.contains("/path/to/project"));
        assert!(section.contains("linux"));
        assert!(section.contains("/bin/bash"));
        assert!(section.contains("total 0"));
    }
}
