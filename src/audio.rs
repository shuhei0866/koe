use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};

/// List connected input devices with human-readable names.
///
/// Reads `/proc/asound/cards` to get friendly card descriptions (e.g. "Logicool BRIO")
/// instead of raw ALSA PCM names (e.g. "front:CARD=BRIO,DEV=0").
/// Only currently connected cards appear in procfs, so disconnected devices are excluded.
/// Falls back to cpal device names on non-Linux or if procfs is unavailable.
pub fn list_input_devices() -> Result<Vec<String>> {
    if let Ok(cards) = std::fs::read_to_string("/proc/asound/cards") {
        let devices: Vec<String> = cards
            .lines()
            .filter_map(|line| {
                let dash = line.find(" - ")?;
                let name = line[dash + 3..].trim();
                if name.is_empty() { None } else { Some(name.to_string()) }
            })
            .collect();
        if !devices.is_empty() {
            return Ok(devices);
        }
    }
    // Fallback: raw cpal names
    let host = cpal::default_host();
    let devices: Vec<String> = host
        .input_devices()
        .context("listing input devices")?
        .filter_map(|d| d.name().ok())
        .collect();
    Ok(devices)
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

/// Audio recorder using cpal.
pub struct AudioRecorder {
    buffer: Arc<Mutex<Vec<f32>>>,
    stream: Option<cpal::Stream>,
    sample_rate: u32,
}

impl AudioRecorder {
    pub fn new() -> Result<Self> {
        Ok(Self {
            buffer: Arc::new(Mutex::new(Vec::new())),
            stream: None,
            sample_rate: 0,
        })
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
                    for chunk in data.chunks(channels) {
                        let mono: f32 = chunk.iter().sum::<f32>() / channels as f32;
                        buf.push(mono);
                    }
                },
                err_fn,
                None,
            )?,
            cpal::SampleFormat::I16 => {
                let buffer = self.buffer.clone();
                device.build_input_stream(
                    &config.into(),
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        let mut buf = buffer.lock().unwrap();
                        for chunk in data.chunks(channels) {
                            let mono: f32 =
                                chunk.iter().map(|&s| s as f32 / 32768.0).sum::<f32>()
                                    / channels as f32;
                            buf.push(mono);
                        }
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
