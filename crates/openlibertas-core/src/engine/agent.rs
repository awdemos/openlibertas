use super::ChatEngine;

impl ChatEngine {
    pub fn start_agent_loop(&mut self) {
        if self.agents.status == super::AgentStatus::Idle {
            self.agents.status = super::AgentStatus::Active;
            self.agents.current_iteration = 0;
        }
    }

    pub fn finish_agent_loop(&mut self) {
        if self.agents.status == super::AgentStatus::Active {
            self.agents.status = super::AgentStatus::Idle;
            self.agents.current_iteration = 0;
        }
    }

    pub fn agent_iteration_exceeded(&self) -> bool {
        self.agents.status == super::AgentStatus::Active
            && self.agents.current_iteration >= self.agents.max_iterations
    }

    pub fn increment_agent_iteration(&mut self) {
        self.agents.current_iteration += 1;
    }

    pub fn isolate_session(&mut self) {
        self.chat.messages.retain(|m| m.role == crate::domain::Role::System);
        self.chat.scroll = 0;
        self.chat.streaming = false;
    }

    pub fn switch_persona(&mut self, persona: impl Into<String>, prompt: impl Into<String>) {
        let persona = persona.into();
        let prompt = prompt.into();
        self.agents.persona = persona.clone();
        self.agent_prompt = Some(prompt);
        self.agents.current_iteration = 0;
    }

    pub fn set_plan_mode(&mut self, mode: super::AgentMode) {
        self.agents.mode = mode;
    }

    pub fn plan_mode(&self) -> super::AgentMode {
        self.agents.mode
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::{AgentStatus, ChatEngine};

    #[test]
    fn agent_loop_transitions() {
        let mut engine = ChatEngine::new();
        engine.agents.status = AgentStatus::Idle;
        engine.start_agent_loop();
        assert_eq!(engine.agents.status, AgentStatus::Active);
        engine.finish_agent_loop();
        assert_eq!(engine.agents.status, AgentStatus::Idle);
    }

    #[test]
    fn agent_loop_exceeded_check() {
        let mut engine = ChatEngine::new();
        engine.agents.status = AgentStatus::Active;
        engine.agents.max_iterations = 3;
        engine.agents.current_iteration = 3;
        assert!(engine.agent_iteration_exceeded());
        engine.agents.current_iteration = 2;
        assert!(!engine.agent_iteration_exceeded());
    }

    #[test]
    fn switch_persona_updates_state() {
        let mut engine = ChatEngine::new();
        engine.switch_persona("Research", "You are a researcher");
        assert_eq!(engine.agents.persona, "Research");
        assert_eq!(
            engine.agent_prompt,
            Some("You are a researcher".to_string())
        );
    }
}
