//! NVIDIA Parakeet transcription via sherpa-onnx (`sherpa-rs`), behind the
//! `sherpa-onnx` feature. The fakes remain the default test seam; this is the
//! second real FFI surface after whisper (Phase C, PRD 136).
//!
//! Mirrors `WhisperCppEngine` on the two spike-3 findings, adapted to the
//! sherpa-onnx transducer API:
//!
//! - **Keep the model resident.** The `TransducerRecognizer` is held after
//!   `load` and reused until `unload`. sherpa's `transcribe` takes `&mut self`,
//!   so the resident recognizer lives behind a `Mutex` to satisfy the shared
//!   `&self` trait method without leaking the sherpa type across the boundary.
//! - **Automatic backend fallback.** The platform GPU provider is tried first
//!   (CoreML on macOS, CUDA elsewhere), then CPU, so a GPU that fails to
//!   initialise degrades to a working transcription rather than a hard failure.
//!
//! A Parakeet transducer is four files, not one: sherpa needs the encoder,
//! decoder, and joiner ONNX graphs plus a `tokens.txt`. `ModelHandle::path` is
//! therefore the model *directory*; the canonical sherpa export file names are
//! resolved inside it (preferring the full-precision graphs, falling back to
//! the `int8` variants).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use sherpa_rs::transducer::{TransducerConfig, TransducerRecognizer};

use crate::TranscriptionEngine;
use crate::types::{
    AudioBuffer, Backend, EngineError, EngineId, EngineOptions, LanguageTag, ModelHandle, Segment,
    TranscribeOptions, Transcript,
};

/// A resident sherpa-onnx Parakeet transcription engine.
#[derive(Default)]
pub struct SherpaOnnxEngine {
    // `Mutex` gives interior mutability for sherpa's `&mut self` transcribe
    // while the trait exposes `&self`, and keeps the engine `Sync`.
    recognizer: Mutex<Option<TransducerRecognizer>>,
}

impl SherpaOnnxEngine {
    pub fn new() -> Self {
        Self::default()
    }
}

impl TranscriptionEngine for SherpaOnnxEngine {
    fn id(&self) -> EngineId {
        EngineId::SherpaOnnx
    }

    fn load(&mut self, model: &ModelHandle, opts: &EngineOptions) -> Result<(), EngineError> {
        let files = ParakeetFiles::resolve(&model.path)?;

        // Provider policy mirrors whisper's backend fallback: a pinned backend
        // is honoured, `None` means "best available", so try the platform GPU
        // then CPU. sherpa has no Vulkan provider; a pinned Vulkan degrades to
        // CPU with a warning rather than failing the load.
        let providers: &[&str] = match opts.backend {
            Some(Backend::Cpu) => &["cpu"],
            Some(Backend::Metal) => &["coreml"],
            Some(Backend::Cuda) => &["cuda"],
            Some(Backend::Vulkan) => {
                tracing::warn!("sherpa-onnx has no Vulkan provider; using CPU");
                &["cpu"]
            }
            None if cfg!(target_os = "macos") => &["coreml", "cpu"],
            None => &["cuda", "cpu"],
        };

        let num_threads = opts
            .threads
            .map(|t| t.min(i32::MAX as usize) as i32)
            .unwrap_or(0);

        let mut last_error = None;
        for &provider in providers {
            let config = TransducerConfig {
                encoder: files.encoder.to_string_lossy().into_owned(),
                decoder: files.decoder.to_string_lossy().into_owned(),
                joiner: files.joiner.to_string_lossy().into_owned(),
                tokens: files.tokens.to_string_lossy().into_owned(),
                sample_rate: crate::PIPELINE_SAMPLE_RATE_HZ as i32,
                num_threads,
                provider: Some(provider.to_owned()),
                debug: false,
                ..TransducerConfig::default()
            };
            match TransducerRecognizer::new(config) {
                Ok(recognizer) => {
                    tracing::info!(provider, model = %model.path.display(), "parakeet model loaded");
                    *self.recognizer.lock().map_err(|_| poisoned())? = Some(recognizer);
                    return Ok(());
                }
                Err(err) => {
                    tracing::warn!(
                        provider,
                        %err,
                        "parakeet load attempt failed; trying next provider"
                    );
                    last_error = Some(err.to_string());
                }
            }
        }

        Err(EngineError::ModelLoad(last_error.unwrap_or_else(|| {
            "no compute backend available".to_owned()
        })))
    }

    fn unload(&mut self) {
        // A poisoned lock still means "no usable recognizer"; clear regardless.
        if let Ok(mut guard) = self.recognizer.lock() {
            *guard = None;
        }
    }

    fn is_loaded(&self) -> bool {
        self.recognizer
            .lock()
            .map(|guard| guard.is_some())
            .unwrap_or(false)
    }

    fn transcribe(
        &self,
        audio: &AudioBuffer,
        opts: &TranscribeOptions,
    ) -> Result<Transcript, EngineError> {
        let mut guard = self.recognizer.lock().map_err(|_| poisoned())?;
        let recognizer = guard.as_mut().ok_or(EngineError::NotLoaded)?;

        // The pipeline guarantees 16 kHz mono f32 (ARCHITECTURE.md 4.1), which
        // is what sherpa-onnx consumes; no conversion here.
        let text = recognizer
            .transcribe(audio.sample_rate_hz, &audio.samples)
            .trim()
            .to_owned();

        // sherpa's offline transducer returns one plain string with no segment
        // timing; represent it as a single segment spanning the utterance.
        let segments = if text.is_empty() {
            Vec::new()
        } else {
            let duration = audio.samples.len() as f32 / audio.sample_rate_hz.max(1) as f32;
            vec![Segment {
                text,
                t0: 0.0,
                t1: duration,
                confidence: 1.0,
            }]
        };

        // Parakeet TDT 0.6B is English-only; honour a pinned tag, else "en".
        let language = opts
            .language
            .clone()
            .unwrap_or_else(|| LanguageTag::from("en"));
        Ok(Transcript { segments, language })
    }
}

fn poisoned() -> EngineError {
    EngineError::Inference("recognizer lock poisoned".to_owned())
}

/// The four files a sherpa-onnx Parakeet transducer needs, resolved from the
/// model directory.
struct ParakeetFiles {
    encoder: PathBuf,
    decoder: PathBuf,
    joiner: PathBuf,
    tokens: PathBuf,
}

impl ParakeetFiles {
    fn resolve(dir: &Path) -> Result<Self, EngineError> {
        if !dir.is_dir() {
            return Err(EngineError::ModelLoad(format!(
                "parakeet model path is not a directory: {}",
                dir.display()
            )));
        }
        Ok(Self {
            encoder: resolve_graph(dir, "encoder")?,
            decoder: resolve_graph(dir, "decoder")?,
            joiner: resolve_graph(dir, "joiner")?,
            tokens: resolve_file(dir, &["tokens.txt"])?,
        })
    }
}

/// Resolve a transducer graph, preferring the full-precision export over the
/// `int8` quantised variant (both are valid sherpa Parakeet exports).
fn resolve_graph(dir: &Path, stem: &str) -> Result<PathBuf, EngineError> {
    resolve_file(
        dir,
        &[&format!("{stem}.onnx"), &format!("{stem}.int8.onnx")],
    )
}

fn resolve_file(dir: &Path, names: &[&str]) -> Result<PathBuf, EngineError> {
    names
        .iter()
        .map(|name| dir.join(name))
        .find(|path| path.is_file())
        .ok_or_else(|| {
            EngineError::ModelLoad(format!(
                "parakeet model missing {} in {}",
                names.join(" / "),
                dir.display()
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PIPELINE_SAMPLE_RATE_HZ;
    use std::fs;

    /// A sherpa Parakeet export directory to exercise the real FFI path.
    /// Skipped when unset so the default suite needs no cached model; CI points
    /// this at an exported model dir.
    fn model_from_env() -> Option<PathBuf> {
        std::env::var_os("VERBATIM_PARAKEET_MODEL")
            .map(PathBuf::from)
            .filter(|path| path.is_dir())
    }

    #[test]
    fn resolve_prefers_full_precision_then_int8_and_reports_missing() {
        let dir = std::env::temp_dir().join(format!("verbatim-parakeet-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp model dir");

        // Missing files are reported, not silently accepted.
        assert!(ParakeetFiles::resolve(&dir).is_err());

        // int8-only graph is accepted when the full-precision one is absent.
        for name in [
            "encoder.int8.onnx",
            "decoder.onnx",
            "joiner.onnx",
            "tokens.txt",
        ] {
            fs::write(dir.join(name), b"x").expect("write stub");
        }
        let files = ParakeetFiles::resolve(&dir).expect("all four files present");
        assert!(files.encoder.ends_with("encoder.int8.onnx"));

        // A full-precision graph wins over the int8 variant when both exist.
        fs::write(dir.join("encoder.onnx"), b"x").expect("write stub");
        let files = ParakeetFiles::resolve(&dir).expect("resolves with both variants");
        assert!(files.encoder.ends_with("encoder.onnx"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn loads_with_provider_fallback_and_transcribes() {
        let Some(path) = model_from_env() else {
            eprintln!(
                "skipping parakeet integration test; set VERBATIM_PARAKEET_MODEL to a sherpa export dir"
            );
            return;
        };

        let mut engine = SherpaOnnxEngine::new();
        assert!(!engine.is_loaded());
        assert_eq!(engine.id(), EngineId::SherpaOnnx);

        // Default options pin no backend, so load walks GPU -> CPU.
        engine
            .load(&ModelHandle { path }, &EngineOptions::default())
            .expect("model should load on some provider");
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
