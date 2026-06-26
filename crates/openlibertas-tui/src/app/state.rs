use openlibertas_core::domain::{Model, ProviderId};
use openlibertas_core::search;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Screen {
    Models,
    Chat,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConnectionStatus {
    Connected,
    Disconnected,
    Checking,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Overlay {
    None,
    Tools,
    Mcp,
    Sessions,
    Palette,
    Themes,
    Help,
    Agents,
    AvatarMenu,
    Permission,
}

pub struct SearchState {
    pub matches: Vec<search::SearchMatch>,
    pub index: usize,
    pub active: bool,
}

pub struct ModelState {
    pub models: Vec<Model>,
    pub selected: usize,
    pub current: Option<String>,
    pub provider: ProviderId,
    pub search: String,
}
