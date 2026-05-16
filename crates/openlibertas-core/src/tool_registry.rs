//! Compatibility re-exports: `crate::tool_registry` still resolves to the split types.

pub use crate::agent_tools::{
    extract_key_argument, tool_needs_approval, MessageAssembler, ToolExecutor, ToolRegistry,
};
