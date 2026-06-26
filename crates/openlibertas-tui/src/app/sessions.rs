use crate::app::App;
use openlibertas_core::commands::find_model;
use openlibertas_core::store::SessionMeta;

impl App {
    pub fn filtered_sessions(&self) -> Vec<SessionMeta> {
        let all = self
            .session_manager
            .as_ref()
            .and_then(|s| s.list_with_meta().ok())
            .unwrap_or_default();

        if self.session_search.is_empty() {
            all
        } else {
            let search = self.session_search.to_lowercase();
            all.into_iter()
                .filter(|meta| {
                    meta.id.to_lowercase().contains(&search)
                        || meta
                            .title
                            .as_ref()
                            .map(|t| t.to_lowercase().contains(&search))
                            .unwrap_or(false)
                        || meta.preview.to_lowercase().contains(&search)
                })
                .collect()
        }
    }

    pub fn session_prev(&mut self) {
        self.session_selected = self.session_selected.saturating_sub(1);
    }

    pub fn session_next(&mut self, count: usize) {
        if count > 0 {
            self.session_selected = (self.session_selected + 1).min(count - 1);
        }
    }

    pub fn load_selected_session(&mut self) -> Option<String> {
        let sessions = self.filtered_sessions();
        let selected = sessions.get(self.session_selected)?;
        let id = selected.id.clone();

        if let Some(ref mut sm) = self.session_manager {
            match sm.load(&id) {
                Ok(session) => {
                    self.engine.chat_mut().messages = session.messages;
                    self.engine.chat_mut().scroll = 0;
                    if let Some(ref model) = session.model {
                        self.models.current = Some(model.clone());
                        if let Some((idx, _matched_name)) = find_model(&self.models.models, model) {
                            self.models.selected = idx;
                            if let Some(m) = self.models.models.get(idx) {
                                self.set_provider(m.provider.clone());
                            }
                        }
                    }
                    self.overlay = crate::app::Overlay::None;
                    self.session_search.clear();
                    self.session_selected = 0;
                    Some(format!("Session '{}' loaded", id))
                }
                Err(e) => Some(format!("Failed to load: {}", e)),
            }
        } else {
            Some("Store not available".to_string())
        }
    }

    pub fn delete_selected_session(&mut self) -> Option<String> {
        let sessions = self.filtered_sessions();
        let selected = sessions.get(self.session_selected)?;
        let id = selected.id.clone();

        if let Some(ref mut sm) = self.session_manager {
            match sm.delete(&id) {
                Ok(_) => {
                    if self.session_selected > 0
                        && self.session_selected >= sessions.len().saturating_sub(1)
                    {
                        self.session_selected -= 1;
                    }
                    Some(format!("Session '{}' deleted", id))
                }
                Err(e) => Some(format!("Failed to delete: {}", e)),
            }
        } else {
            Some("Store not available".to_string())
        }
    }
}
