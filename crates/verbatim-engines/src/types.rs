use std::path::PathBuf;
use std::time::Duration;

use thiserror::Error;

/// Sample rate every pipeline stage assumes (ARCHITECTURE.md 4.1).
pub const PIPELINE_SAMPLE_RATE_HZ: u32 = 16_000;

/// Mono f32 PCM audio as produced by the capture pipeline.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioBuffer {
    pub samples: Vec<f32>,
    pub sample_rate_hz: u32,
}

impl AudioBuffer {
    pub fn duration(&self) -> Duration {
        Duration::from_secs_f64(self.samples.len() as f64 / f64::from(self.sample_rate_hz))
    }
}

/// BCP-47 language tag, e.g. `en` or `de-DE`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LanguageTag(pub String);

impl From<&str> for LanguageTag {
    fn from(tag: &str) -> Self {
        Self(tag.to_owned())
    }
}

/// One timed span of transcribed speech.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub text: String,
    /// Segment start, seconds from utterance start.
    pub t0: f32,
    /// Segment end, seconds from utterance start.
    pub t1: f32,
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Transcript {
    pub segments: Vec<Segment>,
    pub language: LanguageTag,
}

impl Transcript {
    /// The full transcript text, segments joined in order.
    pub fn text(&self) -> String {
        self.segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Stable identifier of an engine implementation; the registry keys on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EngineId {
    WhisperCpp,
    SherpaOnnx,
    LlamaCpp,
    /// Test-only fake engines.
    Fake,
}

/// Compute backend an engine may run on; the registry picks the best available
/// with automatic CPU fallback on init failure (spike 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Backend {
    Metal,
    Cuda,
    Vulkan,
    Cpu,
}

/// A model that has been resolved on disk (downloaded and hash-verified).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelHandle {
    pub path: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EngineOptions {
    /// Pin a backend; `None` lets the engine pick with CPU fallback.
    pub backend: Option<Backend>,
    pub threads: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TranscribeOptions {
    /// `None` means autodetect.
    pub language: Option<LanguageTag>,
}

/// Everything the polish pipeline feeds the LLM besides the raw transcript
/// (ARCHITECTURE.md 4.3). Few-shot examples are load-bearing (spike 4).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PolishProfile {
    pub id: String,
    pub system_prompt: String,
    pub few_shot: Vec<FewShotExample>,
    /// User-confirmed personal dictionary terms.
    pub dictionary: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FewShotExample {
    pub raw: String,
    pub polished: String,
}

/// Polish output: either usable text or a typed rejection that tells the
/// pipeline to inject raw instead. Rejection is not an error (UX.md 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolishOutcome {
    Polished { text: String },
    Rejected { reason: PolishRejection },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolishRejection {
    /// Edit distance from raw exceeded the length-scaled threshold (spike 4).
    SimilarityGuard,
    /// The utterance-length-derived deadline was missed.
    DeadlineMissed,
    /// Engine or model unavailable while polish is enabled (E10).
    EngineUnavailable,
}

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("model failed to load: {0}")]
    ModelLoad(String),
    #[error("no supported compute backend available")]
    NoBackend,
    #[error("engine not loaded")]
    NotLoaded,
    #[error("inference failed: {0}")]
    Inference(String),
}
