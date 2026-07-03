//! whisper.cpp transcription via `whisper-rs` (ARCHITECTURE.md 4.2,
//! ENGINEERING.md 2), behind the `whisper-cpp` feature. The fakes remain the
//! default test seam.
//!
//! Two spike-3 findings shape this:
//!
//! - **Keep the model resident.** Loading recurs per dictation otherwise; the
//!   `WhisperContext` is held after `load` and reused until `unload`.
//! - **Automatic backend fallback.** Metal (macOS) is tried first, then CPU, so
//!   a GPU that fails to initialise degrades to a working transcription rather
//!   than a hard failure (spike 3: Whisper crashes on some GPU configs).

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::TranscriptionEngine;
use crate::types::{
    AudioBuffer, Backend, EngineError, EngineId, EngineOptions, LanguageTag, ModelHandle, Segment,
    TranscribeOptions, Transcript,
};

/// A resident whisper.cpp transcription engine.
#[derive(Default)]
pub struct WhisperCppEngine {
    context: Option<WhisperContext>,
    threads: Option<usize>,
}

impl WhisperCppEngine {
    pub fn new() -> Self {
        Self::default()
    }
}

impl TranscriptionEngine for WhisperCppEngine {
    fn id(&self) -> EngineId {
        EngineId::WhisperCpp
    }

    fn load(&mut self, model: &ModelHandle, opts: &EngineOptions) -> Result<(), EngineError> {
        let path = model
            .path
            .to_str()
            .ok_or_else(|| EngineError::ModelLoad("model path is not valid UTF-8".to_owned()))?;

        // Backend policy: a pinned backend is honoured exactly; `None` means
        // "best available", so try the GPU then fall back to CPU (spike 3).
        let gpu_attempts: &[bool] = match opts.backend {
            Some(Backend::Cpu) => &[false],
            Some(_) => &[true],
            None => &[true, false],
        };

        let mut last_error = None;
        for &use_gpu in gpu_attempts {
            let mut params = WhisperContextParameters::default();
            params.use_gpu(use_gpu);
            match WhisperContext::new_with_params(path, params) {
                Ok(context) => {
                    tracing::info!(use_gpu, model = %model.path.display(), "whisper model loaded");
                    self.context = Some(context);
                    self.threads = opts.threads;
                    return Ok(());
                }
                Err(err) => {
                    tracing::warn!(
                        use_gpu,
                        ?err,
                        "whisper load attempt failed; trying next backend"
                    );
                    last_error = Some(err);
                }
            }
        }

        Err(EngineError::ModelLoad(
            last_error
                .map(|err| err.to_string())
                .unwrap_or_else(|| "no compute backend available".to_owned()),
        ))
    }

    fn unload(&mut self) {
        self.context = None;
    }

    fn is_loaded(&self) -> bool {
        self.context.is_some()
    }

    fn transcribe(
        &self,
        audio: &AudioBuffer,
        opts: &TranscribeOptions,
    ) -> Result<Transcript, EngineError> {
        let context = self.context.as_ref().ok_or(EngineError::NotLoaded)?;

        // The pipeline guarantees 16 kHz mono f32 (ARCHITECTURE.md 4.1), which
        // is exactly what whisper.cpp consumes; no conversion here.
        let mut state = context
            .create_state()
            .map_err(|err| EngineError::Inference(err.to_string()))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        if let Some(threads) = self.threads {
            params.set_n_threads(threads.min(i32::MAX as usize) as i32);
        }
        // `None` lets whisper autodetect the language.
        let language = opts.language.as_ref().map(|tag| tag.0.as_str());
        params.set_language(language);

        state
            .full(params, &audio.samples)
            .map_err(|err| EngineError::Inference(err.to_string()))?;

        let segment_count = state
            .full_n_segments()
            .map_err(|err| EngineError::Inference(err.to_string()))?;

        let mut segments = Vec::with_capacity(segment_count.max(0) as usize);
        for index in 0..segment_count {
            let text = state
                .full_get_segment_text(index)
                .map_err(|err| EngineError::Inference(err.to_string()))?;
            let t0 = state
                .full_get_segment_t0(index)
                .map_err(|err| EngineError::Inference(err.to_string()))?;
            let t1 = state
                .full_get_segment_t1(index)
                .map_err(|err| EngineError::Inference(err.to_string()))?;

            segments.push(Segment {
                text: text.trim().to_owned(),
                // whisper timestamps are centiseconds from utterance start.
                t0: t0 as f32 / 100.0,
                t1: t1 as f32 / 100.0,
                confidence: segment_confidence(&state, index),
            });
        }

        let detected = opts
            .language
            .clone()
            .unwrap_or_else(|| LanguageTag::from("en"));
        Ok(Transcript {
            segments,
            language: detected,
        })
    }
}

/// Mean token probability for a segment, used as a coarse confidence. Falls back
/// to full confidence when per-token probabilities are unavailable.
fn segment_confidence(state: &whisper_rs::WhisperState, segment: i32) -> f32 {
    let token_count = match state.full_n_tokens(segment) {
        Ok(count) if count > 0 => count,
        _ => return 1.0,
    };
    let mut sum = 0.0f32;
    let mut counted = 0.0f32;
    for token in 0..token_count {
        if let Ok(prob) = state.full_get_token_prob(segment, token) {
            sum += prob;
            counted += 1.0;
        }
    }
    if counted > 0.0 { sum / counted } else { 1.0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PIPELINE_SAMPLE_RATE_HZ;
    use std::path::PathBuf;

    /// A ggml whisper model to exercise the real FFI path. Skipped when unset so
    /// the default suite needs no cached model; CI points this at a tiny model.
    fn model_from_env() -> Option<PathBuf> {
        std::env::var_os("VERBATIM_WHISPER_MODEL")
            .map(PathBuf::from)
            .filter(|path| path.exists())
    }

    #[test]
    fn loads_with_backend_fallback_and_transcribes() {
        let Some(path) = model_from_env() else {
            eprintln!(
                "skipping whisper integration test; set VERBATIM_WHISPER_MODEL to a ggml model"
            );
            return;
        };

        let mut engine = WhisperCppEngine::new();
        assert!(!engine.is_loaded());
        assert_eq!(engine.id(), EngineId::WhisperCpp);

        // Default options pin no backend, so load walks GPU -> CPU (spike 3).
        engine
            .load(&ModelHandle { path }, &EngineOptions::default())
            .expect("model should load on some backend");
        assert!(engine.is_loaded());

        // One second of silence must round-trip the decode path as Ok; the
        // transcript may legitimately be empty.
        let audio = AudioBuffer {
            samples: vec![0.0; PIPELINE_SAMPLE_RATE_HZ as usize],
            sample_rate_hz: PIPELINE_SAMPLE_RATE_HZ,
        };
        let transcript = engine
            .transcribe(&audio, &TranscribeOptions::default())
            .expect("transcription of silence should succeed");
        assert_eq!(transcript.language, LanguageTag::from("en"));

        engine.unload();
        assert!(!engine.is_loaded());
        assert!(
            engine
                .transcribe(&audio, &TranscribeOptions::default())
                .is_err(),
            "transcribe after unload must report NotLoaded"
        );
    }
}
