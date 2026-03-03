use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};

/// List connected input devices with human-readable names.
///
/// Cross-references cpal's capture-capable input devices with `/proc/asound/cards`
/// to show friendly card descriptions (e.g. "Logicool BRIO") instead of raw ALSA
/// PCM names (e.g. "front:CARD=BRIO,DEV=0"). Playback-only cards (HDMI, etc.)
/// are excluded because they don't appear in cpal's input device list.
/// Falls back to raw cpal names on non-Linux or if procfs is unavailable.
pub fn list_input_devices() -> Result<Vec<String>> {
    let host = cpal::default_host();
    let input_names: Vec<String> = host
        .input_devices()
        .context("listing input devices")?
        .filter_map(|d| d.name().ok())
        .collect();

    // Try to resolve friendly names from /proc/asound/cards
    if let Ok(cards_content) = std::fs::read_to_string("/proc/asound/cards") {
        // Build card_id → description map
        // Format: " 0 [BRIO           ]: USB-Audio - Logicool BRIO"
        let card_map: std::collections::HashMap<String, String> = cards_content
            .lines()
            .filter_map(|line| {
                let bracket_start = line.find('[')?;
                let bracket_end = line.find(']')?;
                let card_id = line[bracket_start + 1..bracket_end].trim().to_string();
                let dash = line.find(" - ")?;
                let description = line[dash + 3..].trim().to_string();
                if description.is_empty() { None } else { Some((card_id, description)) }
            })
            .collect();

        // Extract CARD=xxx from cpal input device names, deduplicate, resolve to descriptions
        let mut seen = std::collections::HashSet::new();
        let mut friendly: Vec<String> = Vec::new();
        for name in &input_names {
            if let Some(card_id) = extract_card_id(name) {
                if seen.insert(card_id.clone()) {
                    if let Some(desc) = card_map.get(&card_id) {
                        friendly.push(desc.clone());
                    } else {
                        friendly.push(name.clone());
                    }
                }
            }
        }
        if !friendly.is_empty() {
            return Ok(friendly);
        }
    }

    // Fallback: raw cpal names
    Ok(input_names)
}

/// Extract the card identifier from an ALSA PCM name (e.g. "front:CARD=BRIO,DEV=0" → "BRIO").
fn extract_card_id(pcm_name: &str) -> Option<String> {
    let start = pcm_name.find("CARD=")? + 5;
    let rest = &pcm_name[start..];
    let end = rest.find(',').unwrap_or(rest.len());
    Some(rest[..end].to_string())
}

/// Raw audio data in the format expected by whisper: 16kHz mono f32 PCM.
#[derive(Debug, Clone)]
pub struct AudioData {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

impl AudioData {
    /// Resample to 16kHz if needed (simple linear interpolation).
    pub fn resample_to_16khz(&self) -> Vec<f32> {
        if self.sample_rate == 16000 {
            return self.samples.clone();
        }

        let ratio = self.sample_rate as f64 / 16000.0;
        let new_len = (self.samples.len() as f64 / ratio) as usize;
        let mut resampled = Vec::with_capacity(new_len);

        for i in 0..new_len {
            let src_idx = i as f64 * ratio;
            let idx_floor = src_idx.floor() as usize;
            let idx_ceil = (idx_floor + 1).min(self.samples.len() - 1);
            let frac = src_idx - idx_floor as f64;
            let sample =
                self.samples[idx_floor] as f64 * (1.0 - frac) + self.samples[idx_ceil] as f64 * frac;
            resampled.push(sample as f32);
        }

        resampled
    }

    /// Encode as WAV bytes (for API upload).
    pub fn to_wav_bytes(&self) -> Result<Vec<u8>> {
        let samples_16k = self.resample_to_16khz();
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };

        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut writer = hound::WavWriter::new(&mut cursor, spec)
                .context("creating WAV writer")?;
            for &sample in &samples_16k {
                let s = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
                writer.write_sample(s).context("writing WAV sample")?;
            }
            writer.finalize().context("finalizing WAV")?;
        }
        Ok(cursor.into_inner())
    }
}

/// Compute the RMS (root mean square) level of an audio chunk.
pub fn compute_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

/// Audio recorder using cpal.
pub struct AudioRecorder {
    buffer: Arc<Mutex<Vec<f32>>>,
    stream: Option<cpal::Stream>,
    sample_rate: u32,
    rms_sender: tokio::sync::watch::Sender<f32>,
    rms_receiver: tokio::sync::watch::Receiver<f32>,
}

impl AudioRecorder {
    pub fn new() -> Result<Self> {
        let (rms_sender, rms_receiver) = tokio::sync::watch::channel(0.0f32);
        Ok(Self {
            buffer: Arc::new(Mutex::new(Vec::new())),
            stream: None,
            sample_rate: 0,
            rms_sender,
            rms_receiver,
        })
    }

    /// Get a receiver for the current RMS audio level (updated each audio callback).
    pub fn rms_receiver(&self) -> tokio::sync::watch::Receiver<f32> {
        self.rms_receiver.clone()
    }

    /// Start recording from the default input device.
    pub fn start(&mut self) -> Result<()> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .context("no input device available")?;

        tracing::info!("Using input device: {}", device.name().unwrap_or_default());

        let config = device
            .default_input_config()
            .context("no default input config")?;

        self.sample_rate = config.sample_rate().0;
        tracing::info!(
            "Recording at {} Hz, {} channels",
            self.sample_rate,
            config.channels()
        );

        let buffer = self.buffer.clone();
        let channels = config.channels() as usize;
        let rms_sender = self.rms_sender.clone();

        // Clear previous buffer
        buffer.lock().unwrap().clear();

        let err_fn = |err| {
            tracing::error!("Audio stream error: {}", err);
        };

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &config.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let mut buf = buffer.lock().unwrap();
                    // Convert to mono by averaging channels
                    let mut mono_samples = Vec::with_capacity(data.len() / channels);
                    for chunk in data.chunks(channels) {
                        let mono: f32 = chunk.iter().sum::<f32>() / channels as f32;
                        buf.push(mono);
                        mono_samples.push(mono);
                    }
                    // Compute and send RMS level
                    let rms = compute_rms(&mono_samples);
                    let _ = rms_sender.send(rms);
                },
                err_fn,
                None,
            )?,
            cpal::SampleFormat::I16 => {
                let buffer = self.buffer.clone();
                let rms_sender_i16 = self.rms_sender.clone();
                device.build_input_stream(
                    &config.into(),
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        let mut buf = buffer.lock().unwrap();
                        let mut mono_samples = Vec::with_capacity(data.len() / channels);
                        for chunk in data.chunks(channels) {
                            let mono: f32 =
                                chunk.iter().map(|&s| s as f32 / 32768.0).sum::<f32>()
                                    / channels as f32;
                            buf.push(mono);
                            mono_samples.push(mono);
                        }
                        // Compute and send RMS level
                        let rms = compute_rms(&mono_samples);
                        let _ = rms_sender_i16.send(rms);
                    },
                    err_fn,
                    None,
                )?
            }
            format => anyhow::bail!("Unsupported sample format: {:?}", format),
        };

        stream.play().context("starting audio stream")?;
        self.stream = Some(stream);
        tracing::info!("Recording started");
        Ok(())
    }

    /// Stop recording and return the captured audio data.
    pub fn stop(&mut self) -> Result<AudioData> {
        // Drop the stream to stop recording
        self.stream.take();

        let samples = {
            let mut buf = self.buffer.lock().unwrap();
            std::mem::take(&mut *buf)
        };

        tracing::info!(
            "Recording stopped: {} samples ({:.1}s)",
            samples.len(),
            samples.len() as f64 / self.sample_rate as f64
        );

        Ok(AudioData {
            samples,
            sample_rate: self.sample_rate,
        })
    }

    pub fn is_recording(&self) -> bool {
        self.stream.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_rms_silence() {
        let samples = vec![0.0f32; 100];
        let rms = compute_rms(&samples);
        assert!((rms - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_compute_rms_empty() {
        let rms = compute_rms(&[]);
        assert!((rms - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_compute_rms_known_values() {
        // All samples at 1.0 => RMS = 1.0
        let samples = vec![1.0f32; 100];
        let rms = compute_rms(&samples);
        assert!((rms - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_compute_rms_sine_wave() {
        // RMS of a sine wave of amplitude A = A / sqrt(2)
        let amplitude = 0.5f32;
        let samples: Vec<f32> = (0..10000)
            .map(|i| amplitude * (2.0 * std::f32::consts::PI * i as f32 / 100.0).sin())
            .collect();
        let rms = compute_rms(&samples);
        let expected = amplitude / 2.0f32.sqrt();
        assert!(
            (rms - expected).abs() < 0.01,
            "RMS {} should be close to {}",
            rms,
            expected
        );
    }

    #[test]
    fn test_compute_rms_negative_values() {
        // RMS of [-0.5, 0.5] = sqrt((0.25 + 0.25) / 2) = 0.5
        let samples = vec![-0.5f32, 0.5];
        let rms = compute_rms(&samples);
        assert!((rms - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_rms_receiver_initial_value() {
        let recorder = AudioRecorder::new().unwrap();
        let rx = recorder.rms_receiver();
        assert!(((*rx.borrow()) - 0.0).abs() < f32::EPSILON);
    }
}
