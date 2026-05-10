use anyhow::Result;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct ThinkArgs {
    thought: String,
}

/// A meta-cognitive tool that allows the model to reason through problems
/// step by step before taking action.
pub fn think(args: Value) -> Result<String> {
    let args: ThinkArgs = serde_json::from_value(args)?;
    Ok(format!(
        "Thinking through the problem:\n{}\n\nNow I'll proceed with the appropriate action.",
        args.thought
    ))
}

pub fn think_tool() -> crate::tools::BuiltinTool {
    crate::define_tool!(
        "think",
        "Use this tool to think through a problem step by step before taking action. \
        This helps break down complex tasks, consider edge cases, and plan your approach. \
        Provide your reasoning in the 'thought' parameter.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "thought": {
                    "type": "string",
                    "description": "Your step-by-step reasoning about the problem. Think through what you need to do, what information you have, what might go wrong, and how you'll approach the solution."
                }
            },
            "required": ["thought"]
        }),
        think
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn think_returns_reasoning() {
        let result =
            think(serde_json::json!({"thought": "I need to analyze this code carefully"})).unwrap();
        assert!(result.contains("I need to analyze this code carefully"));
        assert!(result.contains("Thinking through the problem"));
    }
}
