use reqwest::multipart;
use tracing::{error, info};

const ELEVENLABS_API_BASE: &str = "https://api.elevenlabs.io/v1";
const DEFAULT_TTS_MODEL: &str = "eleven_multilingual_v2";
const DEFAULT_STT_MODEL: &str = "scribe_v1";

/// Client for ElevenLabs voice APIs.
#[derive(Debug, Clone)]
pub struct ElevenLabsClient {
    api_key: crate::config::SecretString,
    voice_id: String,
    http: reqwest::Client,
}

impl ElevenLabsClient {
    pub fn new(api_key: crate::config::SecretString, voice_id: String) -> Result<Self, crate::voice::error::VoiceError> {
        if api_key.expose_secret().is_empty() {
            return Err(crate::voice::error::VoiceError::MissingApiKey);
        }
        Ok(Self {
            api_key,
            voice_id,
            http: reqwest::Client::new(),
        })
    }

    /// Transcribe audio bytes to text using ElevenLabs STT.
    /// Audio should be in WAV, MP3, or other supported format.
    pub async fn transcribe(&self, audio_bytes: Vec<u8>) -> Result<String, crate::voice::error::VoiceError> {
        let url = format!("{}/speech-to-text", ELEVENLABS_API_BASE);
        let audio_len = audio_bytes.len();
        info!("STT request: {} bytes", audio_len);

        let file_part = multipart::Part::bytes(audio_bytes)
            .file_name("audio.wav")
            .mime_str("audio/wav")
            .map_err(|e| {
                error!("STT multipart error: {}", e);
                crate::voice::error::VoiceError::SerializationError(e.to_string())
            })?;

        let model_part = multipart::Part::text(DEFAULT_STT_MODEL.to_string());

        let form = multipart::Form::new()
            .part("file", file_part)
            .part("model_id", model_part);

        let response = self
            .http
            .post(&url)
            .header("xi-api-key", self.api_key.expose_secret())
            .multipart(form)
            .send()
            .await
            .map_err(|e| {
                error!("STT network error: {}", e);
                crate::voice::error::VoiceError::NetworkError(e.to_string())
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            error!("STT HTTP error: {} - {}", status, body);
            return Err(crate::voice::error::VoiceError::SttError(format!(
                "HTTP {}: {}",
                status, body
            )));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| {
                error!("STT JSON parse error: {}", e);
                crate::voice::error::VoiceError::SerializationError(e.to_string())
            })?;

        let text = json.get("text").and_then(|v| v.as_str()).ok_or_else(|| {
            error!("STT response missing 'text' field: {:?}", json);
            crate::voice::error::VoiceError::SttError("Missing 'text' in STT response".to_string())
        })?;

        info!("STT success: '{}'", text);
        Ok(text.to_string())
    }

    pub async fn text_to_speech(&self, text: &str) -> Result<Vec<u8>, crate::voice::error::VoiceError> {
        let url = format!(
            "{}/text-to-speech/{}/stream?output_format=mp3_44100_128",
            ELEVENLABS_API_BASE, self.voice_id
        );
        info!("TTS request: voice_id={}, text_len={}", self.voice_id, text.len());

        let body = serde_json::json!({
            "text": text,
            "model_id": DEFAULT_TTS_MODEL,
        });

        let response = self
            .http
            .post(&url)
            .header("xi-api-key", self.api_key.expose_secret())
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                error!("TTS network error: {}", e);
                crate::voice::error::VoiceError::NetworkError(e.to_string())
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            error!("TTS HTTP error: {} - {}", status, body);
            return Err(crate::voice::error::VoiceError::TtsError(format!(
                "HTTP {}: {}",
                status, body
            )));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| {
                error!("TTS body read error: {}", e);
                crate::voice::error::VoiceError::NetworkError(e.to_string())
            })?;

        info!("TTS success: {} bytes", bytes.len());
        Ok(bytes.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_creation_requires_api_key() {
        let result = ElevenLabsClient::new(crate::config::SecretString::new("".to_string()), "voice-id".to_string());
        assert!(matches!(
            result,
            Err(crate::voice::error::VoiceError::MissingApiKey)
        ));
    }

    #[test]
    fn client_creation_success() {
        let client = ElevenLabsClient::new(crate::config::SecretString::new("test-key".to_string()), "voice-id".to_string()).unwrap();
        assert_eq!(client.voice_id, "voice-id");
    }

    #[test]
    fn stt_response_parsing() {
        let json = serde_json::json!({"text": "Hello world"});
        let text = json.get("text").and_then(|v| v.as_str()).unwrap();
        assert_eq!(text, "Hello world");
    }
}
