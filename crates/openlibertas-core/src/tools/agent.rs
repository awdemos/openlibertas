use crate::tools::BuiltinTool;
use anyhow::Result;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct SwitchPersonaArgs {
    persona: String,
    reason: String,
    isolate: Option<bool>,
}

/// Switch to a different agent persona mid-conversation.
/// This allows the active agent to delegate work to a specialist.
pub fn switch_persona(args: Value) -> Result<String> {
    let args: SwitchPersonaArgs = serde_json::from_value(args)?;
    let _ = args.isolate;
    Ok(format!(
        "Switched to '{}' persona. Reason: {}. \
        The new agent will continue from here with the appropriate expertise.",
        args.persona, args.reason
    ))
}

pub fn switch_persona_tool() -> BuiltinTool {
    crate::define_tool!(
        "switch_persona",
        "Switch to a different agent persona to delegate work to a specialist. \
        Use this ONCE when the current task requires expertise that another persona provides. \
        For example, switch to 'Seeker' for codebase exploration, 'Sage' for code review, \
        'Strategist' for planning, or 'Artisan' for deep implementation. \
        CRITICAL: Do NOT call this tool repeatedly. Switch once, then proceed with the task. \
        After switching, the new persona will continue the conversation with the appropriate context.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "persona": {
                    "type": "string",
                    "description": "The name of the persona to switch to. Available personas: Orchestrator, Coding, Research, Creative, Captain, Artisan, Sage, Pathfinder, Seeker, Witness, Strategist, Examiner, Steward, Visionary, Operative"
                },
                "reason": {
                    "type": "string",
                    "description": "Brief explanation of why you're delegating to this persona and what you expect them to do."
                },
                "isolate": {
                    "type": "boolean",
                    "description": "If true, start a fresh conversation context for the new persona. The new agent will not see previous tool calls and results, avoiding confusion. Recommended when switching to a completely different task."
                }
            },
            "required": ["persona", "reason"]
        }),
        switch_persona
    )
}

/// Spawn a parallel sub-agent with a different persona to work on a sub-task.
/// Results are returned to the parent agent for synthesis.
pub fn spawn_subagent(_args: Value) -> Result<String> {
    // This is a marker tool — the actual sub-agent spawning is handled
    // at the engine/app level where we have access to the backend and state.
    // The tool result will contain the task description, and the app will
    // detect this special tool and spawn the sub-agent.
    Ok("Sub-agent spawn request received. The sub-agent will execute in parallel and results will be provided when complete.".to_string())
}

pub fn spawn_subagent_tool() -> BuiltinTool {
    crate::define_tool!(
        "spawn_subagent",
        "Spawn a parallel sub-agent with a different persona to work on a sub-task. \
        Use this when you need work done in parallel or by a specialist while you continue. \
        The sub-agent receives the task description and optional context, executes independently, \
        and returns results. You can spawn multiple sub-agents in parallel. \
        Available personas for sub-agents: Seeker, Pathfinder, Research, Sage, Strategist, Artisan, Coding",
        serde_json::json!({
            "type": "object",
            "properties": {
                "persona": {
                    "type": "string",
                    "description": "The persona for the sub-agent. Choose based on the task type."
                },
                "task": {
                    "type": "string",
                    "description": "Clear, specific task for the sub-agent. Include success criteria."
                },
                "context": {
                    "type": "string",
                    "description": "Optional context from the parent agent. Include relevant conversation history, files, or decisions the sub-agent needs to know."
                }
            },
            "required": ["persona", "task"]
        }),
        spawn_subagent
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switch_persona_works() {
        let result = switch_persona(serde_json::json!({
            "persona": "Seeker",
            "reason": "Need to find all occurrences of this pattern in the codebase"
        })).unwrap();
        assert!(result.contains("Seeker"));
        assert!(result.contains("Need to find"));
    }

    #[test]
    fn spawn_subagent_works() {
        let result = spawn_subagent(serde_json::json!({
            "persona": "Research",
            "task": "Find recent async runtime benchmarks",
            "context": "We're comparing tokio and async-std for a new project"
        })).unwrap();
        assert!(result.contains("Sub-agent spawn request"));
    }
}
