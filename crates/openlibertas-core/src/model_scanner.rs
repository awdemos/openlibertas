use std::fs;
use std::path::Path;

use crate::domain::{Model, ProviderId};

pub fn scan_local_models<P: AsRef<Path>>(dir: P) -> Vec<Model> {
    let mut models = Vec::new();
    let dir = dir.as_ref();

    if !dir.exists() {
        return models;
    }

    let Ok(entries) = fs::read_dir(dir) else {
        return models;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("gguf") {
            if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                let (supports_tools, supports_voice) = Model::infer_capabilities(name);
                models.push(Model {
                    id: name.to_string(),
                    provider: ProviderId::new("local"),
                    supports_tools,
                    supports_voice,
                    local: true,
                });
            }
        }
    }

    models
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn scan_finds_gguf_files() {
        let tmp = tempfile::tempdir().unwrap();
        let mut f = std::fs::File::create(tmp.path().join("test-model.gguf")).unwrap();
        f.write_all(b"fake").unwrap();

        let models = scan_local_models(tmp.path());
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "test-model");
        assert_eq!(models[0].provider.as_str(), "local");
    }

    #[test]
    fn scan_ignores_non_gguf_files() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::File::create(tmp.path().join("readme.txt")).unwrap();

        let models = scan_local_models(tmp.path());
        assert!(models.is_empty());
    }

    #[test]
    fn scan_returns_empty_for_missing_dir() {
        let models = scan_local_models("/nonexistent/path");
        assert!(models.is_empty());
    }
}
