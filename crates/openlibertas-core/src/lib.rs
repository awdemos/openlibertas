//! OpenLibertas core library: provider, config, commands, domain, agent_tools, voice, MCP.

#![cfg_attr(not(test), warn(clippy::unwrap_used))]
#![cfg_attr(not(test), warn(clippy::expect_used))]
#![cfg_attr(not(test), warn(clippy::panic))]

pub mod agent_loop;
pub mod agent_tools;
pub mod agent_turn;
pub mod backend;
pub mod capability;
pub mod commands;
pub mod completion;
pub mod config;
pub mod credentials;
pub mod domain;
pub mod engine;
pub mod env_context;
pub mod export;
pub mod facade;
pub mod history;
pub mod mcp;
pub mod model_scanner;
pub mod permission;
pub mod prompt;
pub mod runtime;
pub mod search;
pub mod session;
pub mod soul;
pub mod state;
pub mod store;
pub mod tool_format;
pub mod voice;

// Re-exports for backward compatibility — these moved from `tool_registry` to `agent_tools`.
pub use agent_tools::{
    extract_key_argument, tool_needs_approval, MessageAssembler, ToolExecutor, ToolRegistry,
};

#[cfg(test)]
pub mod test_utils;
pub use agent_loop::{run_agent_loop, LoopAction, PersonaResolver};
pub use backend::Provider;
pub use capability::{ProviderCapabilities, ProviderKind};
pub use commands::{CommandResult, ModelSwitchResult, SlashCommand};
pub use config::{Config, PermissionPolicy, PermissionState, ProviderConfig, SecretString};
pub use domain::{
    now_timestamp, BackendEvent, FunctionDefinition, Message, Model, ProviderId, Role, ToolCall,
    ToolDefinition,
};
pub use permission::{
    is_safe_readonly_command, tool_requires_permission, PermissionRequest, PermissionResponse,
    PermissionService,
};
pub use session::SessionManager;
pub use tool_format::ToolFormat;
