//! Wire Protocol — typed message boundary between UI and agent layers.
//!
//! A formal in-process communication layer replacing ad-hoc `ChatEvent` streaming.
//! Uses bounded `tokio::sync::mpsc` channels for backpressure.

use std::time::Duration;
use tokio::sync::mpsc;

/// A typed message on the wire between UI and agent layers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireMessage {
    /// Start of a new assistant turn.
    TurnBegin {
        turn_id: String,
        model: String,
    },
    /// Streaming content piece.
    ContentPart {
        content: ContentBlock,
    },
    /// Tool call request.
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
    /// Tool execution result.
    ToolResult {
        id: String,
        name: String,
        result: String,
        success: bool,
    },
    /// Agent status change.
    StatusUpdate {
        status: String,
    },
    /// End of assistant turn.
    TurnEnd {
        turn_id: String,
    },
    /// Error during processing.
    Error {
        message: String,
    },
}

/// A block of content within a `WireMessage::ContentPart`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentBlock {
    /// Plain text content.
    Text(String),
    /// Think / reasoning content (displayed differently).
    Reasoning(String),
    /// Tool use invocation.
    ToolUse {
        id: String,
        name: String,
        arguments: String,
    },
}

/// Errors that can occur on the wire channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireError {
    /// The channel has been closed.
    Closed,
    /// Receive operation timed out.
    Timeout,
    /// The channel is at capacity.
    Full,
}

impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WireError::Closed => write!(f, "wire channel closed"),
            WireError::Timeout => write!(f, "wire channel receive timed out"),
            WireError::Full => write!(f, "wire channel full"),
        }
    }
}

impl std::error::Error for WireError {}

/// Sending half of a `WireChannel`.
#[derive(Debug)]
pub struct WireSender {
    tx: mpsc::Sender<WireMessage>,
}

impl Clone for WireSender {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
        }
    }
}

impl WireSender {
    /// Send a message, waiting if the channel is full.
    pub async fn send(&self, msg: WireMessage) -> Result<(), WireError> {
        self.tx.send(msg).await.map_err(|_| WireError::Closed)
    }

    /// Attempt to send a message without waiting.
    pub fn try_send(&self, msg: WireMessage) -> Result<(), WireError> {
        self.tx.try_send(msg).map_err(|e| match e {
            mpsc::error::TrySendError::Full(_) => WireError::Full,
            mpsc::error::TrySendError::Closed(_) => WireError::Closed,
        })
    }
}

/// Receiving half of a `WireChannel`.
#[derive(Debug)]
pub struct WireReceiver {
    rx: mpsc::Receiver<WireMessage>,
}

impl WireReceiver {
    /// Receive the next message.
    pub async fn recv(&mut self) -> Option<WireMessage> {
        self.rx.recv().await
    }

    /// Receive the next message, timing out after `duration`.
    pub async fn recv_timeout(&mut self, duration: Duration) -> Option<WireMessage> {
        match tokio::time::timeout(duration, self.rx.recv()).await {
            Ok(Some(msg)) => Some(msg),
            Ok(None) | Err(_) => None,
        }
    }

    /// Close the channel, preventing further sends.
    pub fn close(&mut self) {
        self.rx.close();
    }
}

/// Factory for bounded wire channels.
pub struct WireChannel;

impl WireChannel {
    /// Create a new bounded wire channel with the given capacity.
    pub fn new(capacity: usize) -> (WireSender, WireReceiver) {
        let (tx, rx) = mpsc::channel(capacity);
        (WireSender { tx }, WireReceiver { rx })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn send_and_recv_message() {
        let (tx, mut rx) = WireChannel::new(4);
        let msg = WireMessage::StatusUpdate {
            status: "thinking".into(),
        };
        tx.send(msg.clone()).await.unwrap();
        let received = rx.recv().await;
        assert_eq!(received, Some(msg));
    }

    #[tokio::test]
    async fn try_send_when_full() {
        let (tx, _rx) = WireChannel::new(1);
        let msg1 = WireMessage::StatusUpdate {
            status: "one".into(),
        };
        let msg2 = WireMessage::StatusUpdate {
            status: "two".into(),
        };
        tx.try_send(msg1).unwrap();
        let err = tx.try_send(msg2).unwrap_err();
        assert_eq!(err, WireError::Full);
    }

    #[tokio::test]
    async fn recv_timeout_returns_none() {
        let (_tx, mut rx) = WireChannel::new(4);
        let result = rx.recv_timeout(Duration::from_millis(10)).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn close_prevents_sends() {
        let (tx, mut rx) = WireChannel::new(4);
        rx.close();
        let msg = WireMessage::StatusUpdate {
            status: "after close".into(),
        };
        let err = tx.send(msg).await.unwrap_err();
        assert_eq!(err, WireError::Closed);
    }

    #[tokio::test]
    async fn all_variants_roundtrip() {
        let (tx, mut rx) = WireChannel::new(10);

        let messages = vec![
            WireMessage::TurnBegin {
                turn_id: "t1".into(),
                model: "gpt-4".into(),
            },
            WireMessage::ContentPart {
                content: ContentBlock::Text("hello".into()),
            },
            WireMessage::ContentPart {
                content: ContentBlock::Reasoning("thinking...".into()),
            },
            WireMessage::ContentPart {
                content: ContentBlock::ToolUse {
                    id: "tc1".into(),
                    name: "read_file".into(),
                    arguments: r#"{"path": "/tmp/test"}"#.into(),
                },
            },
            WireMessage::ToolCall {
                id: "tc1".into(),
                name: "read_file".into(),
                arguments: r#"{"path": "/tmp/test"}"#.into(),
            },
            WireMessage::ToolResult {
                id: "tc1".into(),
                name: "read_file".into(),
                result: "file contents".into(),
                success: true,
            },
            WireMessage::StatusUpdate {
                status: "Running tools...".into(),
            },
            WireMessage::TurnEnd {
                turn_id: "t1".into(),
            },
            WireMessage::Error {
                message: "something went wrong".into(),
            },
        ];

        for msg in &messages {
            tx.send(msg.clone()).await.unwrap();
        }

        for expected in &messages {
            let received = rx.recv().await.unwrap();
            assert_eq!(&received, expected);
        }
    }
}
