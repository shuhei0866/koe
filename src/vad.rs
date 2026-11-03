//! Voice activity detection (Silero VAD via whisper.cpp).
//!
//! Drops silence before the audio reaches the recognizer. Whisper tends to
//! hallucinate sentences on silent or noise-only input, and a recording with
//! no speech at all should not reach the recognizer or the AI post-processor.

use anyhow::{Context, Result};
use std::sync::Mutex;
use whisper_rs::{WhisperVadContext, WhisperVadContextParams, WhisperVadParams};

use crate::audio::AudioData;
use crate::config::VadConfig;

const SAMPLE_RATE: u32 = 16_000;

/// Silence inserted between kept speech segments, so words from separate
/// segments are not glued together.
const SEGMENT_GAP_MS: u32 = 100;

/// Upper bound for durations passed to whisper.cpp, which multiplies them by
/// the sample rate in `int` arithmetic.
const MAX_DURATION_MS: u32 = 10_000;

pub struct VoiceActivityDetector {
    ctx: Mutex<WhisperVadContext>,
    threshold: f32,
    min_speech_ms: u32,
    speech_pad_ms: u32,
}

impl VoiceActivityDetector {
    /// Load the VAD model. Returns `Ok(None)` when VAD is disabled.
    pub fn new(config: &VadConfig) -> Result<Option<Self>> {
        if !config.enabled {
            return Ok(None);
        }

        let model_path = shellexpand::tilde(&config.model_path);
        let path = std::path::Path::new(model_path.as_ref());
        if !path.exists() {
            anyhow::bail!(
                "VAD model not found: {}. Download ggml-silero-v5.1.2.bin from https://huggingface.co/ggml-org/whisper-vad",
                path.display()
            );
        }

        let path_str = path.to_str().context("VAD model path is not valid UTF-8")?;
        let ctx = WhisperVadContext::new(path_str, WhisperVadContextParams::default())
            .map_err(|e| anyhow::anyhow!("failed to load VAD model: {}", e))?;
        tracing::info!("VAD model loaded: {}", path.display());

        Ok(Some(Self {
            ctx: Mutex::new(ctx),
            threshold: config.threshold,
            min_speech_ms: config.min_speech_ms,
            speech_pad_ms: config.speech_pad_ms,
        }))
    }

    /// Keep only the speech in `audio`. The result is 16 kHz mono and has no
    /// samples when no speech was detected. Blocking; call from a blocking thread.
    ///
    /// `ignore_leading_ms`: a segment that ends within this many ms of the
    /// start is dropped. The microphone picks up Koe's own start cue, and VAD
    /// takes it for speech.
    pub fn extract_speech(&self, audio: &AudioData, ignore_leading_ms: u32) -> Result<AudioData> {
        let samples = audio.resample_to_16khz();

        let speech_pad_ms = self.speech_pad_ms.min(MAX_DURATION_MS);
        let mut params = WhisperVadParams::new();
        params.set_threshold(self.threshold);
        params.set_min_speech_duration(self.min_speech_ms.min(MAX_DURATION_MS) as i32);
        params.set_speech_pad(speech_pad_ms as i32);

        let segments: Vec<(f32, f32)> = {
            let mut ctx = self
                .ctx
                .lock()
                .map_err(|_| anyhow::anyhow!("VAD context lock poisoned"))?;
            ctx.segments_from_samples(params, &samples)
                .map_err(|e| anyhow::anyhow!("VAD failed: {}", e))?
                .map(|s| (s.start, s.end))
                .collect()
        };
        let segments = drop_leading(&segments, ignore_leading_ms, speech_pad_ms);
        tracing::debug!(
            "VAD segments (s): {}",
            segments
                .iter()
                .map(|(s, e)| format!("{:.2}-{:.2}", s / 100.0, e / 100.0))
                .collect::<Vec<_>>()
                .join(", ")
        );

        Ok(AudioData {
            samples: concat_segments(&samples, &segments, SAMPLE_RATE),
            sample_rate: SAMPLE_RATE,
        })
    }
}

/// Drop segments (centiseconds, padded by `pad_ms` on each side) whose
/// unpadded end falls within the first `ignore_ms`. Drops nothing when
/// `ignore_ms` is 0.
fn drop_leading(segments: &[(f32, f32)], ignore_ms: u32, pad_ms: u32) -> Vec<(f32, f32)> {
    if ignore_ms == 0 {
        return segments.to_vec();
    }
    let cutoff_cs = (ignore_ms + pad_ms) as f32 / 10.0;
    segments
        .iter()
        .copied()
        .filter(|&(_, end)| end > cutoff_cs)
        .collect()
}

/// Concatenate the speech segments (start/end in centiseconds) of `samples`,
/// with a short silence between segments.
fn concat_segments(samples: &[f32], segments: &[(f32, f32)], sample_rate: u32) -> Vec<f32> {
    let to_index = |cs: f32| -> usize {
        let idx = (cs.max(0.0) as f64 / 100.0 * sample_rate as f64).round() as usize;
        idx.min(samples.len())
    };
    let gap = (sample_rate * SEGMENT_GAP_MS / 1000) as usize;

    let mut out = Vec::new();
    for &(start, end) in segments {
        let (s, e) = (to_index(start), to_index(end));
        if s >= e {
            continue;
        }
        if !out.is_empty() {
            out.extend(std::iter::repeat_n(0.0, gap));
        }
        out.extend_from_slice(&samples[s..e]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(len: usize) -> Vec<f32> {
        (0..len).map(|i| i as f32).collect()
    }

    #[test]
    fn no_segments_gives_empty_audio() {
        assert!(concat_segments(&ramp(16_000), &[], 16_000).is_empty());
    }

    #[test]
    fn single_segment_is_cut_out() {
        // 0.10s..0.20s at 16 kHz = samples 1600..3200
        let out = concat_segments(&ramp(16_000), &[(10.0, 20.0)], 16_000);
        assert_eq!(out.len(), 1600);
        assert_eq!(out[0], 1600.0);
        assert_eq!(*out.last().unwrap(), 3199.0);
    }

    #[test]
    fn segments_are_joined_with_silence() {
        let out = concat_segments(&ramp(16_000), &[(0.0, 10.0), (50.0, 60.0)], 16_000);
        let gap = 1600; // 100 ms
        assert_eq!(out.len(), 1600 + gap + 1600);
        assert!(out[1600..1600 + gap].iter().all(|&s| s == 0.0));
        assert_eq!(out[1600 + gap], 8000.0);
    }

    #[test]
    fn segments_past_the_end_are_clamped() {
        let out = concat_segments(&ramp(16_000), &[(90.0, 500.0), (600.0, 700.0)], 16_000);
        assert_eq!(out.len(), 1600);
    }

    #[test]
    fn start_cue_segment_is_dropped() {
        // Observed: the start cue detected as 0.00-0.68s with 200 ms padding.
        assert!(drop_leading(&[(0.0, 68.0)], 600, 200).is_empty());
    }

    #[test]
    fn speech_right_after_the_cue_is_kept() {
        // Observed: "はい" detected as 0.00-1.10s; "OK" as 1.59-2.57s.
        let segs = [(0.0, 110.0), (159.0, 257.0)];
        assert_eq!(drop_leading(&segs, 600, 200), segs.to_vec());
    }

    #[test]
    fn nothing_is_dropped_without_a_cue() {
        // A short recording whose padded end is clamped to its length.
        assert_eq!(drop_leading(&[(0.0, 15.0)], 0, 200).len(), 1);
    }

    #[test]
    fn disabled_vad_loads_nothing() {
        let config = VadConfig {
            enabled: false,
            ..VadConfig::default()
        };
        assert!(VoiceActivityDetector::new(&config).unwrap().is_none());
    }

    /// Needs the real model at the default path:
    /// `cargo test vad -- --ignored`
    #[test]
    #[ignore]
    fn real_model_finds_no_speech_in_silence_and_noise() {
        let detector = VoiceActivityDetector::new(&VadConfig::default())
            .unwrap()
            .expect("VAD enabled by default");

        let silence = AudioData {
            samples: vec![0.0; 48_000 * 3],
            sample_rate: 48_000,
        };
        assert!(detector
            .extract_speech(&silence, 0)
            .unwrap()
            .samples
            .is_empty());

        // Deterministic low-level white noise (xorshift), about -40 dBFS.
        let mut x: u32 = 0x1234_5678;
        let noise: Vec<f32> = (0..16_000 * 3)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                (x as f32 / u32::MAX as f32 - 0.5) * 0.02
            })
            .collect();
        let noise = AudioData {
            samples: noise,
            sample_rate: 16_000,
        };
        assert!(detector
            .extract_speech(&noise, 0)
            .unwrap()
            .samples
            .is_empty());
    }

    #[test]
    fn missing_model_is_an_error() {
        let config = VadConfig {
            model_path: "/nonexistent/ggml-silero.bin".to_string(),
            ..VadConfig::default()
        };
        assert!(VoiceActivityDetector::new(&config).is_err());
    }
}
