use reqwest::multipart;

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
    pub fn new(api_key: crate::config::SecretString, voice_id: String) -> Result<Self, crate::voice::VoiceError> {
        if api_key.expose_secret().is_empty() {
            return Err(crate::voice::VoiceError::MissingApiKey);
        }
        Ok(Self {
            api_key,
            voice_id,
            http: reqwest::Client::new(),
        })
    }

    /// Transcribe audio bytes to text using ElevenLabs STT.
    /// Audio should be in WAV, MP3, or other supported format.
    pub async fn transcribe(&self, audio_bytes: Vec<u8>) -> Result<String, crate::voice::VoiceError> {
        let url = format!("{}/speech-to-text", ELEVENLABS_API_BASE);

        let file_part = multipart::Part::bytes(audio_bytes)
            .file_name("audio.wav")
            .mime_str("audio/wav")
            .map_err(|e| crate::voice::VoiceError::SerializationError(e.to_string()))?;

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
            .map_err(|e| crate::voice::VoiceError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(crate::voice::VoiceError::SttError(format!(
                "HTTP {}: {}",
                status, body
            )));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| crate::voice::VoiceError::SerializationError(e.to_string()))?;

        let text = json.get("text").and_then(|v| v.as_str()).ok_or_else(|| {
            crate::voice::VoiceError::SttError("Missing 'text' in STT response".to_string())
        })?;

        Ok(text.to_string())
    }

    pub async fn text_to_speech(&self, text: &str) -> Result<Vec<u8>, crate::voice::VoiceError> {
        let url = format!(
            "{}/text-to-speech/{}/stream?output_format=mp3_44100_128",
            ELEVENLABS_API_BASE, self.voice_id
        );

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
            .map_err(|e| crate::voice::VoiceError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(crate::voice::VoiceError::TtsError(format!(
                "HTTP {}: {}",
                status, body
            )));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| crate::voice::VoiceError::NetworkError(e.to_string()))?;

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
            Err(crate::voice::VoiceError::MissingApiKey)
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
