use crate::domain::Message;
use crate::store::SessionStore;
use anyhow::Context;

/// Session data returned from load
#[derive(Debug, Clone)]
pub struct LoadedSession {
    pub messages: Vec<Message>,
    pub model: Option<String>,
    pub parent_id: Option<String>,
    pub branch_point: Option<usize>,
}

/// Load a session including model information
pub fn load_session(store: &SessionStore, name: &str) -> anyhow::Result<LoadedSession> {
    let path = store.session_path(name);
    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read session: {name}"))?;
    let session: crate::store::Session = serde_json::from_str(&contents)
        .with_context(|| format!("Failed to parse session: {name}"))?;

    Ok(LoadedSession {
        messages: session.messages,
        model: session.model,
        parent_id: session.parent_id,
        branch_point: session.branch_point,
    })
}
