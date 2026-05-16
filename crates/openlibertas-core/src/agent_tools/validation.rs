use anyhow::{Result, anyhow};

/// Validates that `value` is in the `allowed` list (case-insensitive).
/// Returns an error with `error_template` if not found.
pub fn allowlist(value: &str, allowed: &[&str], error_template: &str) -> Result<()> {
    let normalized = value.to_lowercase();
    if !allowed.contains(&normalized.as_str()) {
        return Err(anyhow!(error_template.replace("{value}", value)));
    }
    Ok(())
}

/// Validates that `value` does not contain any forbidden substring (case-insensitive).
/// Returns an error with `error_template` if a forbidden pattern is found.
pub fn forbidden(value: &str, patterns: &[&str], error_template: &str) -> Result<()> {
    let normalized = value.trim().to_lowercase();
    for pattern in patterns {
        if normalized.contains(pattern) {
            return Err(anyhow!(
                error_template.replace("{pattern}", pattern).replace("{value}", value)
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_accepts_allowed_value() {
        assert!(allowlist("status", &["status", "diff", "log"], "not allowed: {value}").is_ok());
    }

    #[test]
    fn allowlist_rejects_unknown_value() {
        let result = allowlist("rm", &["status", "diff", "log"], "not allowed: {value}");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("rm"));
    }

    #[test]
    fn allowlist_is_case_insensitive() {
        assert!(allowlist("STATUS", &["status"], "err").is_ok());
    }

    #[test]
    fn forbidden_accepts_safe_value() {
        assert!(forbidden("echo hello", &["rm -rf", "mkfs"], "forbidden: {pattern}").is_ok());
    }

    #[test]
    fn forbidden_rejects_bad_value() {
        let result = forbidden("rm -rf /", &["rm -rf", "mkfs"], "forbidden: {pattern}");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("rm -rf"));
    }

    #[test]
    fn forbidden_is_case_insensitive() {
        let result = forbidden("MKFS", &["mkfs"], "forbidden: {pattern}");
        assert!(result.is_err());
    }
}
