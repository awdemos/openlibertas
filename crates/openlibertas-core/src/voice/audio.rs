/// Audio capture and playback utilities.
///
/// This module provides:
/// - `AudioRecorder`: captures microphone input using cpal
/// - `AudioPlayer`: plays back MP3/WAV audio using rodio
///
/// Both are designed to work within an async tokio context.
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::io::Cursor;
use std::sync::{Arc, Mutex};
use tracing::{debug, error, info, warn};

/// Information about an available audio input device.
#[derive(Debug, Clone)]
pub struct InputDeviceInfo {
    pub name: String,
    pub is_default: bool,
    pub sample_rate: u32,
    pub channels: u16,
    pub sample_format: String,
}

pub fn list_input_devices() -> Result<Vec<InputDeviceInfo>, crate::voice::VoiceError> {
    let host = cpal::default_host();
    let default_device = host.default_input_device();
    let default_name = default_device.as_ref().and_then(|d| d.name().ok());
    info!(
        "Enumerating audio input devices, default: {:?}",
        default_name
    );

    let mut devices = Vec::new();
    match host.input_devices() {
        Ok(device_iter) => {
            for device in device_iter {
                let name = match device.name() {
                    Ok(n) => n,
                    Err(e) => {
                        warn!("Failed to get device name: {}", e);
                        continue;
                    }
                };
                let is_default = Some(name.as_str()) == default_name.as_deref();
                let config = match device.default_input_config() {
                    Ok(c) => c,
                    Err(e) => {
                        warn!("Failed to get config for '{}': {}", name, e);
                        continue;
                    }
                };
                debug!(
                    "Input device: {} (default={}, sr={}, ch={}, fmt={:?})",
                    name,
                    is_default,
                    config.sample_rate().0,
                    config.channels(),
                    config.sample_format()
                );
                devices.push(InputDeviceInfo {
                    name,
                    is_default,
                    sample_rate: config.sample_rate().0,
                    channels: config.channels(),
                    sample_format: format!("{:?}", config.sample_format()),
                });
            }
        }
        Err(e) => {
            error!("Failed to enumerate input devices: {}", e);
            return Err(crate::voice::VoiceError::AudioError(format!(
                "Failed to enumerate input devices: {e}"
            )));
        }
    }

    // Sort: default first, then by name
    devices.sort_by(|a, b| {
        if a.is_default && !b.is_default {
            std::cmp::Ordering::Less
        } else if !a.is_default && b.is_default {
            std::cmp::Ordering::Greater
        } else {
            a.name.cmp(&b.name)
        }
    });

    info!("Found {} input devices", devices.len());
    Ok(devices)
}

pub fn default_input_device_name() -> Result<String, crate::voice::VoiceError> {
    let host = cpal::default_host();
    let name = host
        .default_input_device()
        .and_then(|d| d.name().ok())
        .ok_or_else(|| {
            error!("No default input device available");
            crate::voice::VoiceError::AudioError("No default input device available".to_string())
        })?;
    info!("Default input device: {}", name);
    Ok(name)
}

/// Audio statistics for diagnostics.
#[derive(Debug, Clone)]
pub struct AudioStats {
    pub duration_ms: u64,
    pub peak_amplitude: f32,
    pub rms_amplitude: f32,
    pub sample_rate: u32,
    pub channels: u16,
    pub sample_count: usize,
    pub is_silence: bool,
}

pub fn compute_stats(samples: &[f32], sample_rate: u32, channels: u16) -> AudioStats {
    if samples.is_empty() {
        return AudioStats {
            duration_ms: 0,
            peak_amplitude: 0.0,
            rms_amplitude: 0.0,
            sample_rate,
            channels,
            sample_count: 0,
            is_silence: true,
        };
    }

    let mut peak = 0.0f32;
    let mut sum_squares = 0.0f64;
    for &s in samples {
        let abs = s.abs();
        if abs > peak {
            peak = abs;
        }
        sum_squares += (s as f64).powi(2);
    }

    let rms = (sum_squares / samples.len() as f64).sqrt() as f32;
    let duration_ms = if sample_rate == 0 || channels == 0 {
        0
    } else {
        let samples_per_ms = (sample_rate as u64 * channels as u64) / 1000;
        (samples.len() as u64)
            .checked_div(samples_per_ms)
            .unwrap_or(0)
    };

    AudioStats {
        duration_ms,
        peak_amplitude: peak,
        rms_amplitude: rms,
        sample_rate,
        channels,
        sample_count: samples.len(),
        is_silence: peak < 0.005,
    }
}

pub struct Recording {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,
}

impl Recording {
    pub fn duration_ms(&self) -> u64 {
        if self.sample_rate == 0 || self.channels == 0 {
            return 0;
        }
        let total_samples = self.samples.len() as u64;
        let samples_per_ms = (self.sample_rate as u64 * self.channels as u64) / 1000;
        if samples_per_ms == 0 {
            return 0;
        }
        total_samples / samples_per_ms
    }

    pub fn max_amplitude(&self) -> f32 {
        self.samples
            .iter()
            .map(|s| s.abs())
            .fold(0.0f32, f32::max)
    }
}

pub struct AudioRecorder {
    buffer: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
    channels: u16,
    device_name: Option<String>,
}

impl AudioRecorder {
    pub fn new() -> Self {
        Self {
            buffer: Arc::new(Mutex::new(Vec::new())),
            sample_rate: 16000,
            channels: 1,
            device_name: None,
        }
    }

    pub fn with_device(device_name: String) -> Self {
        Self {
            buffer: Arc::new(Mutex::new(Vec::new())),
            sample_rate: 16000,
            channels: 1,
            device_name: Some(device_name),
        }
    }

    pub fn buffer(&self) -> Arc<Mutex<Vec<f32>>> {
        Arc::clone(&self.buffer)
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    pub fn clear_buffer(&mut self) {
        if let Ok(mut buf) = self.buffer.lock() {
            buf.clear();
        }
    }

    pub fn start(&mut self) -> Result<cpal::Stream, crate::voice::VoiceError> {
        // Always clear the buffer before starting to prevent any accumulated
        // samples from a previous recording from being included.
        self.clear_buffer();

        let host = cpal::default_host();
        let device = match &self.device_name {
            Some(name) => {
                info!("Looking for input device: {}", name);
                let mut found = None;
                if let Ok(devices) = host.input_devices() {
                    for d in devices {
                        if let Ok(dname) = d.name() {
                            if dname == *name {
                                found = Some(d);
                                break;
                            }
                        }
                    }
                }
                found.ok_or_else(|| {
                    error!("Input device '{}' not found", name);
                    crate::voice::VoiceError::AudioError(format!(
                        "Input device '{name}' not found. Use /voice_device to list available devices."
                    ))
                })?
            }
            None => {
                let dev = host.default_input_device().ok_or_else(|| {
                    error!("No default input device available");
                    crate::voice::VoiceError::AudioError("No input device available".to_string())
                })?;
                if let Ok(name) = dev.name() {
                    info!("Using default input device: {}", name);
                }
                dev
            }
        };

        let config = device.default_input_config().map_err(|e| {
            error!("Failed to get input config: {}", e);
            crate::voice::VoiceError::AudioError(format!("Failed to get input config: {e}"))
        })?;

        self.sample_rate = config.sample_rate().0;
        self.channels = config.channels();
        info!(
            "Recording: sr={}, ch={}, fmt={:?}",
            self.sample_rate,
            self.channels,
            config.sample_format()
        );

        let buffer = Arc::clone(&self.buffer);
        let err_fn = |err| {
            warn!("Audio input stream error: {}", err);
        };

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &config.into(),
                move |data: &[f32], _| {
                    if let Ok(mut buf) = buffer.lock() {
                        buf.extend_from_slice(data);
                    }
                },
                err_fn,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_input_stream(
                &config.into(),
                move |data: &[i16], _| {
                    if let Ok(mut buf) = buffer.lock() {
                        buf.extend(data.iter().map(|&s| s as f32 / i16::MAX as f32));
                    }
                },
                err_fn,
                None,
            ),
            cpal::SampleFormat::U16 => device.build_input_stream(
                &config.into(),
                move |data: &[u16], _| {
                    if let Ok(mut buf) = buffer.lock() {
                        buf.extend(
                            data.iter()
                                .map(|&s| (s as f32 / u16::MAX as f32) * 2.0 - 1.0),
                        );
                    }
                },
                err_fn,
                None,
            ),
            _ => {
                error!("Unsupported sample format: {:?}", config.sample_format());
                return Err(crate::voice::VoiceError::AudioError(format!(
                    "Unsupported sample format: {:?}",
                    config.sample_format()
                )));
            }
        }
        .map_err(|e| {
            error!("Failed to build input stream: {}", e);
            crate::voice::VoiceError::AudioError(format!("Failed to build input stream: {e}"))
        })?;

        stream.play().map_err(|e| {
            error!("Failed to start recording: {}", e);
            crate::voice::VoiceError::AudioError(format!("Failed to start recording: {e}"))
        })?;

        info!("Recording started");
        Ok(stream)
    }

    pub fn stop(&self) -> Result<Recording, crate::voice::VoiceError> {
        let samples = self
            .buffer
            .lock()
            .map_err(|e| {
                error!("Mutex poisoned: {}", e);
                crate::voice::VoiceError::AudioError(format!("Mutex poisoned: {e}"))
            })?
            .clone();
        let stats = compute_stats(&samples, self.sample_rate, self.channels);
        info!(
            "Recording stopped: {} samples, {} ms, peak={:.3}, rms={:.3}, silence={}",
            stats.sample_count,
            stats.duration_ms,
            stats.peak_amplitude,
            stats.rms_amplitude,
            stats.is_silence
        );
        Ok(Recording {
            samples,
            sample_rate: self.sample_rate,
            channels: self.channels,
        })
    }
}

impl Default for AudioRecorder {
    fn default() -> Self {
        Self::new()
    }
}

pub struct AudioPlayer {
    _stream: rodio::OutputStream,
    _stream_handle: rodio::OutputStreamHandle,
    sink: rodio::Sink,
}

impl AudioPlayer {
    pub fn new() -> Result<Self, crate::voice::VoiceError> {
        let (_stream, stream_handle) = rodio::OutputStream::try_default().map_err(|e| {
            error!("No audio output device: {}", e);
            crate::voice::VoiceError::AudioError(format!("No audio output device: {e}"))
        })?;
        let sink = rodio::Sink::try_new(&stream_handle).map_err(|e| {
            error!("Failed to create audio sink: {}", e);
            crate::voice::VoiceError::AudioError(format!("Failed to create audio sink: {e}"))
        })?;
        info!("AudioPlayer created");
        Ok(Self {
            _stream,
            _stream_handle: stream_handle,
            sink,
        })
    }

    pub fn play(&self, audio_bytes: Vec<u8>) -> Result<(), crate::voice::VoiceError> {
        let len = audio_bytes.len();
        self.sink.stop();
        let cursor = Cursor::new(audio_bytes);
        let source = rodio::Decoder::new(cursor).map_err(|e| {
            error!("Failed to decode audio: {}", e);
            crate::voice::VoiceError::AudioError(format!("Failed to decode audio: {e}"))
        })?;
        self.sink.append(source);
        info!("Audio playback started: {} bytes", len);
        Ok(())
    }

    pub fn is_playing(&self) -> bool {
        !self.sink.empty()
    }

    pub fn stop(&self) {
        self.sink.stop();
    }

    pub fn play_blocking(audio_bytes: Vec<u8>) -> Result<(), crate::voice::VoiceError> {
        let player = Self::new()?;
        player.play(audio_bytes)?;
        info!("Audio playback blocking...");
        player.sink.sleep_until_end();
        info!("Audio playback complete");
        Ok(())
    }
}

pub fn is_silence(samples: &[f32], threshold: f32) -> bool {
    if samples.is_empty() {
        return true;
    }
    let max_amplitude = samples
        .iter()
        .map(|s| s.abs())
        .fold(0.0f32, f32::max);
    max_amplitude < threshold
}

pub fn stereo_to_mono(samples: &[f32], channels: u16) -> Vec<f32> {
    let ch = channels.max(1) as usize;
    if ch == 1 {
        return samples.to_vec();
    }
    let frames = samples.len() / ch;
    let mut mono = Vec::with_capacity(frames);
    for frame in 0..frames {
        let mut sum = 0.0f32;
        for c in 0..ch {
            sum += samples[frame * ch + c];
        }
        mono.push(sum / ch as f32);
    }
    mono
}

pub fn normalize_audio(samples: &mut [f32], target_peak: f32) {
    if samples.is_empty() || target_peak <= 0.0 {
        return;
    }
    let max_amp = samples
        .iter()
        .map(|s| s.abs())
        .fold(0.0f32, f32::max);
    if max_amp > 0.0 && max_amp < target_peak {
        let gain = target_peak / max_amp;
        for sample in samples.iter_mut() {
            *sample = (*sample * gain).clamp(-1.0, 1.0);
        }
    }
}

pub fn encode_wav(recording: &Recording) -> Result<Vec<u8>, crate::voice::VoiceError> {
    let mut cursor = Cursor::new(Vec::new());
    let spec = hound::WavSpec {
        channels: recording.channels,
        sample_rate: recording.sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut writer = hound::WavWriter::new(&mut cursor, spec)
        .map_err(|e| crate::voice::VoiceError::AudioError(format!("WAV writer error: {e}")))?;

    for &sample in &recording.samples {
        let clamped = sample.clamp(-1.0, 1.0);
        let int_sample = (clamped * i16::MAX as f32) as i16;
        writer
            .write_sample(int_sample)
            .map_err(|e| crate::voice::VoiceError::AudioError(format!("WAV write error: {e}")))?;
    }

    writer
        .finalize()
        .map_err(|e| crate::voice::VoiceError::AudioError(format!("WAV finalize error: {e}")))?;

    Ok(cursor.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_wav_roundtrip() {
        let samples: Vec<f32> = (0..100).map(|i| (i as f32 / 100.0) * 2.0 - 1.0).collect();
        let recording = Recording {
            samples,
            sample_rate: 16000,
            channels: 1,
        };
        let wav_bytes = encode_wav(&recording).unwrap();
        assert!(!wav_bytes.is_empty());
        // WAV header is at least 44 bytes
        assert!(wav_bytes.len() >= 44);
    }

    #[test]
    fn encode_wav_empty() {
        let recording = Recording {
            samples: vec![],
            sample_rate: 16000,
            channels: 1,
        };
        let wav_bytes = encode_wav(&recording).unwrap();
        assert_eq!(wav_bytes.len(), 44); // Just the header
    }
}
