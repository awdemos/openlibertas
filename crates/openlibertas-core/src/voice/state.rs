/// Represents the current state of the voice chat system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceState {
    /// Voice mode is disabled.
    Idle,
    /// Voice mode is enabled, waiting for user input.
    Ready,
    /// Actively recording audio from the microphone.
    Recording,
    /// Sending recorded audio to STT service.
    ProcessingStt,
    /// Sending LLM response text to TTS service.
    ProcessingTts,
    /// Playing back TTS audio.
    Playing,
    /// An error occurred in the voice pipeline.
    Error,
}

impl VoiceState {
    pub fn label(&self) -> &'static str {
        match self {
            VoiceState::Idle => "Idle",
            VoiceState::Ready => "Ready",
            VoiceState::Recording => "Recording",
            VoiceState::ProcessingStt => "Processing STT",
            VoiceState::ProcessingTts => "Processing TTS",
            VoiceState::Playing => "Playing",
            VoiceState::Error => "Error",
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(
            self,
            VoiceState::Recording
                | VoiceState::ProcessingStt
                | VoiceState::ProcessingTts
                | VoiceState::Playing
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_state_labels() {
        assert_eq!(VoiceState::Idle.label(), "Idle");
        assert_eq!(VoiceState::Recording.label(), "Recording");
        assert_eq!(VoiceState::Playing.label(), "Playing");
    }

    #[test]
    fn voice_state_is_active() {
        assert!(!VoiceState::Idle.is_active());
        assert!(!VoiceState::Ready.is_active());
        assert!(VoiceState::Recording.is_active());
        assert!(VoiceState::ProcessingStt.is_active());
        assert!(VoiceState::ProcessingTts.is_active());
        assert!(VoiceState::Playing.is_active());
        assert!(!VoiceState::Error.is_active());
    }
}
