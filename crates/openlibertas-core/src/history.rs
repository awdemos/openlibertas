use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use tracing::warn;

pub const MAX_HISTORY_ENTRIES: usize = 1000;

#[derive(Clone)]
pub struct HistoryStore {
    path: PathBuf,
}

impl HistoryStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> Vec<String> {
        let file = match fs::File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
            Err(e) => {
                warn!("Failed to open history file: {}", e);
                return Vec::new();
            }
        };

        let reader = BufReader::new(file);
        let mut entries = Vec::new();
        for (line_num, line) in reader.lines().enumerate() {
            let line = match line {
                Ok(l) => l,
                Err(e) => {
                    warn!("Failed to read history line {}: {}", line_num + 1, e);
                    continue;
                }
            };
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<serde_json::Value>(&line) {
                Ok(serde_json::Value::String(s)) => {
                    entries.push(s);
                }
                Ok(other) => {
                    warn!(
                        "Skipping non-string history entry at line {}: {:?}",
                        line_num + 1,
                        other
                    );
                }
                Err(e) => {
                    warn!(
                        "Skipping invalid JSON in history at line {}: {}",
                        line_num + 1,
                        e
                    );
                }
            }
        }
        entries
    }

    pub fn append(&self, entry: &str) -> anyhow::Result<()> {
        // Deduplication: skip if same as last entry
        if let Ok(Some(last)) = self.read_last_entry() {
            if last == entry {
                return Ok(());
            }
        }

        // Ensure parent directory exists
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;

        let json_line = serde_json::to_string(entry)?;
        writeln!(file, "{json_line}")?;

        // Trim if exceeds limit
        self.trim_to_limit()?;

        Ok(())
    }

    fn read_last_entry(&self) -> anyhow::Result<Option<String>> {
        let contents = fs::read_to_string(&self.path)?;
        let last_line = contents.lines().last();
        match last_line {
            Some(line) if !line.trim().is_empty() => {
                let val: serde_json::Value = serde_json::from_str(line)?;
                match val {
                    serde_json::Value::String(s) => Ok(Some(s)),
                    _ => Ok(None),
                }
            }
            _ => Ok(None),
        }
    }

    fn trim_to_limit(&self) -> anyhow::Result<()> {
        let contents = fs::read_to_string(&self.path)?;
        let lines: Vec<&str> = contents.lines().collect();
        if lines.len() > MAX_HISTORY_ENTRIES {
            let skip = lines.len() - MAX_HISTORY_ENTRIES;
            let trimmed: Vec<&str> = lines.into_iter().skip(skip).collect();
            let new_contents = trimmed.join("\n");
            // Ensure trailing newline
            fs::write(&self.path, new_contents + "\n")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_empty_when_file_missing() {
        let store = HistoryStore::new(PathBuf::from("/nonexistent/path/history.jsonl"));
        let entries = store.load();
        assert!(entries.is_empty());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let path = tmp_dir.path().join("history.jsonl");
        let store = HistoryStore::new(path.clone());

        store.append("hello").unwrap();
        store.append("world").unwrap();

        let entries = store.load();
        assert_eq!(entries, vec!["hello", "world"]);
    }

    #[test]
    fn dedup_skips_consecutive_duplicates() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let path = tmp_dir.path().join("history.jsonl");
        let store = HistoryStore::new(path);

        store.append("hello").unwrap();
        store.append("hello").unwrap();
        store.append("world").unwrap();

        let entries = store.load();
        assert_eq!(entries, vec!["hello", "world"]);
    }

    #[test]
    fn skips_invalid_json_lines() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let path = tmp_dir.path().join("history.jsonl");
        fs::write(&path, "\"valid\"\nnot json\n\"also valid\"\n").unwrap();

        let store = HistoryStore::new(path);
        let entries = store.load();
        assert_eq!(entries, vec!["valid", "also valid"]);
    }

    #[test]
    fn trims_oldest_when_exceeds_limit() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let path = tmp_dir.path().join("history.jsonl");
        let store = HistoryStore::new(path);

        for i in 0..MAX_HISTORY_ENTRIES + 5 {
            store.append(&format!("entry {}", i)).unwrap();
        }

        let entries = store.load();
        assert_eq!(entries.len(), MAX_HISTORY_ENTRIES);
        assert_eq!(entries[0], "entry 5");
        assert_eq!(
            entries.last().unwrap(),
            &format!("entry {}", MAX_HISTORY_ENTRIES + 4)
        );
    }
}
