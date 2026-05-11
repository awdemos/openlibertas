pub mod audio;
pub mod client;
pub mod error;
pub mod state;

pub use audio::{
    compute_stats, default_input_device_name, list_input_devices, AudioStats, InputDeviceInfo,
};
pub use state::VoiceState;

use audio::{encode_wav, AudioPlayer, AudioRecorder, Recording};
use client::ElevenLabsClient;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Default ElevenLabs voice ID (Rachel).
const DEFAULT_VOICE_ID: &str = "21m00Tcm4TlvDq8ikWAM";

/// Configuration for the voice system.
#[derive(Debug, Clone)]
pub struct VoiceConfig {
    pub api_key: Option<crate::config::SecretString>,
    pub voice_id: String,
    pub input_device: Option<String>,
}

impl VoiceConfig {
    pub fn new(api_key: Option<crate::config::SecretString>, voice_id: Option<String>) -> Self {
        Self {
            api_key,
            voice_id: voice_id.unwrap_or_else(|| DEFAULT_VOICE_ID.to_string()),
            input_device: None,
        }
    }

    pub fn with_input_device(mut self, device: Option<String>) -> Self {
        self.input_device = device;
        self
    }
}

const MIN_RECORDING_MS: u128 = 300;
const MAX_RECORDING_MS: u128 = 10000;
/// Silence threshold: peak amplitude below this is considered silence.
/// 0.001 = 0.1% of full scale, lenient enough for quiet mics.
pub const SILENCE_THRESHOLD: f32 = 0.001;

/// Manages the voice chat lifecycle: recording, STT, TTS, and playback.
pub struct VoiceManager {
    pub enabled: bool,
    pub state: VoiceState,
    pub config: VoiceConfig,
    recorder: Option<AudioRecorder>,
    stream: Option<cpal::Stream>,
    recording_start: Option<std::time::Instant>,
    player: Option<AudioPlayer>,
    tts_queue: Vec<String>,
    tts_playing: bool,
    pub cancel_flag: Arc<AtomicBool>,
    /// True if the current recording was started with push-to-talk (Ctrl+Space).
    /// Used to distinguish from Space toggle mode so the release handler
    /// only stops push-to-talk recordings.
    pub push_to_talk_active: bool,
    /// Monotonically increasing counter to track recording generations.
    /// Each start_recording() increments this. Transcription events carry
    /// the generation so stale transcriptions from cancelled/timeout recordings
    /// can be safely discarded.
    recording_generation: u64,
    /// True while a recording is in progress. Prevents double-start and
    /// provides a reliable way to check recording status independent of state.
    recording_in_progress: bool,
}

impl std::fmt::Debug for VoiceManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VoiceManager")
            .field("enabled", &self.enabled)
            .field("state", &self.state)
            .field("config", &self.config)
            .finish()
    }
}

pub use error::VoiceError;

impl VoiceManager {
    pub fn new(api_key: Option<crate::config::SecretString>, voice_id: Option<String>) -> Self {
        Self {
            enabled: false,
            state: VoiceState::Idle,
            config: VoiceConfig::new(api_key, voice_id),
            recorder: None,
            stream: None,
            recording_start: None,
            player: None,
            tts_queue: Vec::new(),
            tts_playing: false,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            push_to_talk_active: false,
            recording_generation: 0,
            recording_in_progress: false,
        }
    }

    pub fn toggle(&mut self) -> bool {
        self.enabled = !self.enabled;
        if self.enabled {
            self.state = VoiceState::Ready;
        } else {
            self.cancel();
            self.state = VoiceState::Idle;
        }
        self.enabled
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn state(&self) -> VoiceState {
        self.state
    }

    pub fn start_recording(&mut self, push_to_talk: bool) -> Result<u64, VoiceError> {
        if self.recording_in_progress {
            tracing::debug!("start_recording called while already recording - ignoring");
            return Ok(self.recording_generation);
        }
        self.reset_cancel();
        self.recording_generation = self.recording_generation.wrapping_add(1);
        let generation = self.recording_generation;
        let device_name = self.config.input_device.clone();
        let mut recorder = match &device_name {
            Some(name) => AudioRecorder::with_device(name.clone()),
            None => AudioRecorder::new(),
        };
        let stream = recorder.start()?;
        self.push_to_talk_active = push_to_talk;
        self.recorder = Some(recorder);
        self.stream = Some(stream);
        self.recording_start = Some(std::time::Instant::now());
        self.recording_in_progress = true;
        self.state = VoiceState::Recording;
        tracing::info!(
            device = ?device_name,
            push_to_talk,
            generation,
            "Voice recording started"
        );
        Ok(generation)
    }

    pub fn stop_recording(&mut self) -> Result<(u64, Vec<u8>), VoiceError> {
        let generation = self.recording_generation;
        self.stream = None;
        let recorder = self.recorder.take();
        let _elapsed = self.recording_start.map(|s| s.elapsed());
        self.recording_start = None;
        self.push_to_talk_active = false;
        self.recording_in_progress = false;
        self.state = VoiceState::ProcessingStt;

        let recording = match recorder {
            Some(r) => r
                .stop()
                .map_err(|e| VoiceError::AudioError(format!("Failed to stop recording: {e}")))?,
            None => {
                tracing::warn!("stop_recording called with no active recorder");
                return Ok((generation, Vec::new()));
            }
        };

        let stats = compute_stats(
            &recording.samples,
            recording.sample_rate,
            recording.channels,
        );
        tracing::info!(
            generation,
            duration_ms = stats.duration_ms,
            peak = stats.peak_amplitude,
            rms = stats.rms_amplitude,
            samples = recording.samples.len(),
            silence = stats.is_silence,
            "Recording stopped"
        );

        if recording.samples.is_empty() {
            return Ok((generation, Vec::new()));
        }

        if audio::is_silence(&recording.samples, SILENCE_THRESHOLD) {
            tracing::info!(generation, "Recording rejected: silence detected");
            return Ok((generation, Vec::new()));
        }

        let mut samples = recording.samples;
        audio::normalize_audio(&mut samples, 0.75);

        let samples = if recording.channels > 1 {
            audio::stereo_to_mono(&samples, recording.channels)
        } else {
            samples
        };

        let recording = Recording {
            samples,
            sample_rate: recording.sample_rate,
            channels: 1,
        };

        let wav = encode_wav(&recording)?;
        Ok((generation, wav))
    }

    pub fn has_min_recording_duration(&self) -> bool {
        match self.recording_start {
            Some(start) => start.elapsed().as_millis() >= MIN_RECORDING_MS,
            None => false,
        }
    }

    pub fn has_max_recording_duration(&self) -> bool {
        match self.recording_start {
            Some(start) => start.elapsed().as_millis() >= MAX_RECORDING_MS,
            None => false,
        }
    }

    pub fn current_generation(&self) -> u64 {
        self.recording_generation
    }

    pub fn is_recording_silent(&self) -> bool {
        match &self.recorder {
            Some(recorder) => {
                let buffer = recorder.buffer();
                let silent = match buffer.lock() {
                    Ok(samples) => audio::is_silence(&samples, 0.01),
                    Err(_) => true,
                };
                silent
            }
            None => true,
        }
    }

    /// Get audio statistics for the current recording buffer.
    pub fn recording_stats(&self) -> Option<audio::AudioStats> {
        self.recorder.as_ref().map(|recorder| {
            let buffer = recorder.buffer();
            let samples = match buffer.lock() {
                Ok(s) => s.clone(),
                Err(_) => Vec::new(),
            };
            audio::compute_stats(&samples, recorder.sample_rate(), recorder.channels())
        })
    }

    pub fn cancel(&mut self) {
        self.cancel_flag.store(true, Ordering::Relaxed);
        self.stream = None;
        self.recorder = None;
        self.recording_start = None;
        self.push_to_talk_active = false;
        self.recording_in_progress = false;
        // Increment generation so any in-flight transcriptions from this
        // cancelled recording are discarded when they arrive.
        self.recording_generation = self.recording_generation.wrapping_add(1);
        if let Some(ref player) = self.player {
            player.stop();
        }
        self.tts_queue.clear();
        self.tts_playing = false;
        if self.enabled {
            self.state = VoiceState::Ready;
        } else {
            self.state = VoiceState::Idle;
        }
    }

    /// Reset the cancel flag so new operations (recording, playback) can proceed.
    pub fn reset_cancel(&mut self) {
        self.cancel_flag.store(false, Ordering::Relaxed);
    }

    pub fn queue_speech(&mut self, text: String) -> Option<String> {
        if self.tts_playing {
            self.tts_queue.push(text);
            None
        } else {
            self.reset_cancel();
            self.tts_playing = true;
            self.state = VoiceState::Playing;
            Some(text)
        }
    }

    /// Clear the TTS queue and reset playing state.
    pub fn clear_tts_queue(&mut self) {
        self.tts_queue.clear();
        self.tts_playing = false;
    }

    pub fn next_queued_speech(&mut self) -> Option<String> {
        if self.tts_queue.is_empty() {
            self.tts_playing = false;
            if self.enabled {
                self.state = VoiceState::Ready;
            } else {
                self.state = VoiceState::Idle;
            }
            None
        } else {
            Some(self.tts_queue.remove(0))
        }
    }

    pub fn play_audio(&mut self, audio_bytes: Vec<u8>) -> Result<(), VoiceError> {
        if audio_bytes.is_empty() {
            return Ok(());
        }

        if self.player.is_none() {
            self.player = Some(AudioPlayer::new()?);
        }

        self.state = VoiceState::Playing;
        let player = self.player.as_ref().ok_or(VoiceError::AudioError(
            "No audio player available".to_string(),
        ))?;
        player.play(audio_bytes)?;

        Ok(())
    }

    pub async fn synthesize_speech(&self, text: &str) -> Result<Vec<u8>, VoiceError> {
        let api_key = self
            .config
            .api_key
            .as_ref()
            .ok_or(VoiceError::MissingApiKey)?;
        let client = ElevenLabsClient::new(api_key.clone(), self.config.voice_id.clone())?;
        client.text_to_speech(text).await
    }

    pub async fn transcribe_audio(&self, audio_bytes: Vec<u8>) -> Result<String, VoiceError> {
        if audio_bytes.is_empty() {
            return Ok(String::new());
        }
        let api_key = self
            .config
            .api_key
            .as_ref()
            .ok_or(VoiceError::MissingApiKey)?;
        let client = ElevenLabsClient::new(api_key.clone(), self.config.voice_id.clone())?;
        client.transcribe(audio_bytes).await
    }
}

pub async fn stt_transcribe(
    api_key: Option<crate::config::SecretString>,
    audio_bytes: Vec<u8>,
) -> Result<String, VoiceError> {
    if audio_bytes.is_empty() {
        return Ok(String::new());
    }
    let api_key = api_key.ok_or(VoiceError::MissingApiKey)?;
    let client = ElevenLabsClient::new(api_key, DEFAULT_VOICE_ID.to_string())?;
    client.transcribe(audio_bytes).await
}

pub async fn tts_synthesize(
    api_key: Option<crate::config::SecretString>,
    voice_id: String,
    text: &str,
) -> Result<Vec<u8>, VoiceError> {
    let api_key = api_key.ok_or(VoiceError::MissingApiKey)?;
    let client = ElevenLabsClient::new(api_key, voice_id)?;
    client.text_to_speech(text).await
}

pub fn play_audio_blocking(audio_bytes: Vec<u8>) -> Result<(), VoiceError> {
    if audio_bytes.is_empty() {
        return Ok(());
    }
    AudioPlayer::play_blocking(audio_bytes)
}

pub fn play_audio_cancellable(
    audio_bytes: Vec<u8>,
    cancel_flag: Arc<AtomicBool>,
) -> Result<(), VoiceError> {
    if audio_bytes.is_empty() {
        return Ok(());
    }
    let player = AudioPlayer::new()?;
    player.play(audio_bytes)?;
    while player.is_playing() && !cancel_flag.load(Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    player.stop();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_manager_toggle() {
        let mut vm = VoiceManager::new(None, None);
        assert!(!vm.is_enabled());
        assert_eq!(vm.state(), VoiceState::Idle);

        let enabled = vm.toggle();
        assert!(enabled);
        assert!(vm.is_enabled());
        assert_eq!(vm.state(), VoiceState::Ready);

        let enabled = vm.toggle();
        assert!(!enabled);
        assert!(!vm.is_enabled());
        assert_eq!(vm.state(), VoiceState::Idle);
    }

    #[test]
    fn voice_manager_cancel() {
        let mut vm = VoiceManager::new(None, None);
        vm.toggle();
        vm.cancel();
        assert_eq!(vm.state(), VoiceState::Ready);

        vm.toggle();
        vm.cancel();
        assert_eq!(vm.state(), VoiceState::Idle);
    }

    #[test]
    fn voice_config_default_voice_id() {
        let config = VoiceConfig::new(None, None);
        assert_eq!(config.voice_id, "21m00Tcm4TlvDq8ikWAM");
    }

    #[test]
    fn voice_config_custom_voice_id() {
        let config = VoiceConfig::new(None, Some("custom-id".to_string()));
        assert_eq!(config.voice_id, "custom-id");
    }
}
