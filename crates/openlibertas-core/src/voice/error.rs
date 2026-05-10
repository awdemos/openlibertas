use thiserror::Error;

#[derive(Debug, Clone, Error)]
pub enum VoiceError {
    #[error("No ElevenLabs API key configured. Set ELEVENLABS_API_KEY or add elevenlabs_api_key to config.toml")]
    MissingApiKey,
    #[error("Audio I/O error: {0}")]
    AudioError(String),
    #[error("STT request failed: {0}")]
    SttError(String),
    #[error("TTS request failed: {0}")]
    TtsError(String),
    #[error("Network error: {0}")]
    NetworkError(String),
    #[error("Serialization error: {0}")]
    SerializationError(String),
}
