use crossterm::event::Event as CEvent;
use tokio::sync::mpsc;

use openlibertas_core::backend::ChatEvent;
use openlibertas_core::backend::Model;
use openlibertas_core::mcp::McpTool;

#[derive(Debug, Clone)]
pub enum Event {
    Input(CEvent),
    ChatEvent(ChatEvent),
    ModelsLoaded(Result<Vec<Model>, String>),
    McpToolsLoaded(Result<Vec<McpTool>, String>),
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

    pub fn attach_chat_stream(&self, mut stream_rx: mpsc::UnboundedReceiver<ChatEvent>) {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            while let Some(event) = stream_rx.recv().await {
                if tx.send(Event::ChatEvent(event)).is_err() {
                    return;
                }
            }
        });
    }

    pub fn sender(&self) -> mpsc::UnboundedSender<Event> {
        self.tx.clone()
    }
}
