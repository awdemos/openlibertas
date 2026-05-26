use crossterm::event::Event as CEvent;
use tokio::sync::mpsc;

use openlibertas_core::domain::McpServerStatus;
use openlibertas_core::domain::Model;
use openlibertas_core::mcp::McpServerDiagnostics;
use openlibertas_core::mcp::McpTool;
use openlibertas_core::soul::WireMessage;
use std::collections::HashMap;

/// Tuple of (tools, server statuses, server descriptions).
pub type McpToolsResult = Result<
    (
        Vec<McpTool>,
        HashMap<String, McpServerStatus>,
        HashMap<String, String>,
    ),
    String,
>;

#[derive(Debug, Clone)]
pub enum Event {
    Input(CEvent),
    Agent(WireMessage),
    ModelsLoaded(Result<Vec<Model>, String>),
    McpToolsLoaded(McpToolsResult),
    McpDiagnosticsLoaded(HashMap<String, McpServerDiagnostics>),
    McpHealthCheck(Result<HashMap<String, bool>, String>),
    McpToolTest(Result<String, String>),
    VoiceTranscription(String, u64),
    VoicePlaybackComplete,
    VoiceError(String, u64),
    BackendHealthCheck(Result<(), String>),
    PermissionRequest(openlibertas_core::permission::PermissionRequest),
}

pub struct EventStream {
    tx: mpsc::UnboundedSender<Event>,
}

impl EventStream {
    pub fn new() -> (Self, mpsc::UnboundedReceiver<Event>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let tx_clone = tx.clone();

        std::thread::spawn(move || {
            while let Ok(event) = crossterm::event::read() {
                if tx_clone.send(Event::Input(event)).is_err() {
                    break;
                }
            }
        });

        (Self { tx }, rx)
    }

    pub fn attach_wire_receiver(&self, mut wire_rx: mpsc::UnboundedReceiver<WireMessage>) {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            while let Some(msg) = wire_rx.recv().await {
                if tx.send(Event::Agent(msg)).is_err() {
                    return;
                }
            }
        });
    }

    pub fn sender(&self) -> mpsc::UnboundedSender<Event> {
        self.tx.clone()
    }
}
