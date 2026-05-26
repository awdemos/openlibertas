use std::sync::Arc;

use crate::backend::registry::ProviderRegistry;
use crate::backend::runtime::{CancellableBackend, RegistryBackend};
use crate::backend::Provider;
use crate::domain::ProviderId;
use crate::engine::{AgentMode, ChatEngine};
use crate::env_context::EnvContext;
use crate::soul::{Agent, ChatAgent, UserInput, WireSender};

/// Spawn an agent turn in a background tokio task.
pub fn spawn_agent_turn(
    engine: &ChatEngine,
    registry: &ProviderRegistry,
    provider_id: ProviderId,
    model: Option<String>,
    max_tokens: u32,
    input_text: String,
    mode: AgentMode,
    wire: WireSender,
    cancel_token: tokio_util::sync::CancellationToken,
) {
    let ui_messages_before = engine.chat().messages.clone();

    let mut agent_engine = ChatEngine::new()
        .with_context_window(engine.chat().compactor.context_window)
        .with_env_context(EnvContext::detect());

    if let Some(prompt) = engine.system_prompt() {
        agent_engine = agent_engine.with_system_prompt(prompt);
    }
    if let Some(prompt) = engine.agent_prompt() {
        agent_engine = agent_engine.with_agent_prompt(prompt);
    }

    agent_engine.set_tool_format(engine.tool_format());
    agent_engine.chat_mut().messages = ui_messages_before;

    if let Some(client) = engine.tools().client() {
        agent_engine
            .tool_executor_mut()
            .set_client(Some(client.clone()));
        agent_engine.tools_mut().set_client(Some(client.clone()));
        agent_engine
            .tools_mut()
            .set_available_tools(engine.tools().available_tools().to_vec());
        agent_engine
            .tools_mut()
            .set_server_statuses(engine.tools().server_statuses().clone());
        agent_engine
            .tools_mut()
            .set_tool_server_map(engine.tools().tool_server_map().clone());
        agent_engine
            .tools_mut()
            .set_diagnostics(engine.tools().diagnostics().clone());
    }

    agent_engine.set_permission_service(engine.permission_service().clone());
    agent_engine.set_session_id(engine.session_id().to_string());

    let registry_backend = RegistryBackend::new(registry.clone(), provider_id);
    let provider: Arc<dyn Provider> = Arc::new(CancellableBackend::new(
        Arc::new(registry_backend),
        cancel_token,
    ));

    let model = model.unwrap_or_default();

    let agent = ChatAgent::new(
        "default",
        "Chat Agent",
        agent_engine,
        provider,
        model,
        max_tokens,
    );

    let input = UserInput {
        text: input_text,
        file_context: vec![],
        mode,
    };

    tokio::spawn(async move {
        let mut agent = agent;
        let _ = agent.run_turn(input, wire).await;
    });
}
