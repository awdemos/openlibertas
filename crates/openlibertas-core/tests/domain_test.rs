//! Integration tests for domain types: message building, context compaction, and token estimation.

use openlibertas_core::domain::{
    estimate_messages_tokens, Message, Model, ProviderId, Role,
};
use openlibertas_core::session::ContextCompactor;

fn msg(role: Role, content: &str) -> Message {
    Message {
        role,
        content: content.to_string(),
        tool_calls: None,
        tool_call_id: None,
        timestamp: None,
        reasoning_content: None,
        is_prompt: false,
    }
}

#[test]
fn message_token_estimate_positive() {
    let m = msg(Role::User, "hello world");
    let tokens = m.estimate_tokens();
    assert!(tokens > 0);
}

#[test]
fn message_token_estimate_scales_with_content() {
    let short = msg(Role::User, "hi");
    let long = msg(Role::User, &"word ".repeat(200));
    assert!(long.estimate_tokens() > short.estimate_tokens());
}

#[test]
fn estimate_messages_tokens_sums_correctly() {
    let messages = vec![
        msg(Role::User, "hello"),
        msg(Role::Assistant, "world"),
        msg(Role::User, "foo"),
    ];
    let total = estimate_messages_tokens(&messages);
    assert_eq!(total, messages.iter().map(|m| m.estimate_tokens()).sum::<usize>());
}

#[test]
fn message_with_tool_calls_has_higher_estimate() {
    let plain = msg(Role::Assistant, "use the tool");
    let with_tool = Message {
        role: Role::Assistant,
        content: "use the tool".to_string(),
        tool_calls: Some(vec![openlibertas_core::domain::ToolCall {
            id: "call_1".to_string(),
            call_type: "function".to_string(),
            function: openlibertas_core::domain::FunctionCall {
                name: "read_file".to_string(),
                arguments: r#"{"path":"/tmp"}"#.to_string(),
            },
        }]),
        tool_call_id: None,
        timestamp: None,
        reasoning_content: None,
        is_prompt: false,
    };
    assert!(with_tool.estimate_tokens() > plain.estimate_tokens());
}

#[test]
fn context_compactor_does_nothing_when_under_threshold() {
    let mut compactor = ContextCompactor::with_context_window(10000);
    let messages: Vec<Message> = (0..10)
        .map(|i| msg(if i % 2 == 0 { Role::User } else { Role::Assistant }, "x"))
        .collect();

    let result = compactor.compact(&messages);
    assert_eq!(result.len(), messages.len());
    assert_eq!(compactor.compaction_count, 0);
}

#[test]
fn context_compactor_preserves_system_messages() {
    let mut compactor = ContextCompactor::with_context_window(30);
    let messages = vec![
        msg(Role::System, "You are helpful"),
        msg(Role::User, "u1"),
        msg(Role::Assistant, "a1"),
        msg(Role::User, "u2"),
        msg(Role::Assistant, "a2"),
    ];

    let result = compactor.compact(&messages);
    assert!(result.iter().any(|m| m.role == Role::System && m.content == "You are helpful"));
}

#[test]
fn context_compactor_adds_summary_when_dropping() {
    let mut compactor = ContextCompactor::with_context_window(25);
    let messages = vec![
        msg(Role::User, "old user message"),
        msg(Role::Assistant, "old assistant response"),
        msg(Role::User, "new user message"),
        msg(Role::Assistant, "new assistant response"),
    ];

    let result = compactor.compact(&messages);
    let summary = result.iter().find(|m| m.role == Role::System && m.content.contains("compacted"));
    assert!(summary.is_some(), "summary message should be present");
}

#[test]
fn context_compactor_preserves_recent_messages() {
    let mut compactor = ContextCompactor::with_context_window(50);
    let messages = vec![
        msg(Role::User, "drop1"),
        msg(Role::Assistant, "drop2"),
        msg(Role::User, "keep1"),
        msg(Role::Assistant, "keep2"),
    ];

    let result = compactor.compact(&messages);
    assert!(result.iter().any(|m| m.content == "keep1"));
    assert!(result.iter().any(|m| m.content == "keep2"));
}

#[test]
fn model_infer_capabilities_detects_tools() {
    assert!(Model::infer_capabilities("qwen2.5-coder").0);
    assert!(Model::infer_capabilities("llama3-instruct").0);
    assert!(!Model::infer_capabilities("unknown-model").0);
}

#[test]
fn model_infer_capabilities_detects_voice() {
    assert!(Model::infer_capabilities("qwen2.5-omni").1);
    assert!(Model::infer_capabilities("phi-4-multimodal").1);
    assert!(!Model::infer_capabilities("qwen2.5-coder").1);
}

#[test]
fn provider_id_normalizes_case() {
    let id = ProviderId::new("OpenAI");
    assert_eq!(id.as_str(), "openai");
    let id2: ProviderId = "Kimi".into();
    assert_eq!(id2.as_str(), "kimi");
}

#[test]
fn role_display_and_from_str_roundtrip() {
    for role in [Role::User, Role::Assistant, Role::System, Role::Tool] {
        let s = role.to_string();
        let parsed: Role = s.parse().unwrap();
        assert_eq!(role, parsed);
    }
}
