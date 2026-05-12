//! OpenLibertas core library: backend, config, commands, domain, tools, voice, MCP.

#![cfg_attr(not(test), warn(clippy::unwrap_used))]
#![cfg_attr(not(test), warn(clippy::expect_used))]
#![cfg_attr(not(test), warn(clippy::panic))]

pub mod agent_loop;
pub mod backend;
pub mod commands;
pub mod completion;
pub mod config;
pub mod conversation;
pub mod domain;
pub mod engine;
pub mod env_context;
pub mod export;
pub mod history;
pub mod mcp;
pub mod model_scanner;
pub mod prompt;
pub mod search;
pub mod state;
pub mod store;
pub mod tool_format;
pub mod tool_registry;
pub mod tools;
pub mod voice;

pub use agent_loop::{AgentLoop, LoopAction, PersonaResolver};
pub use backend::Backend;
pub use commands::{CommandResult, ModelSwitchResult, SlashCommand};
pub use config::{Config, Provider, SecretString};
pub use domain::{
    ChatEvent, FunctionDefinition, Message, Model, ProviderId, Role, ToolCall, ToolDefinition,
};
pub use tool_format::ToolFormat;
