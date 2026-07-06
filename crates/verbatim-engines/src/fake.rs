//! Fake engines: the deterministic test seam for everything above the engine
//! layer (ENGINEERING.md section 4, E2E smoke).

use std::time::Duration;

use crate::model::{DownloadError, ModelDownloader, ModelSpec, ProgressSink};
use crate::types::{
    AudioBuffer, EngineError, EngineId, EngineOptions, LanguageTag, ModelHandle, PolishOutcome,
    PolishProfile, PolishRejection, Segment, TranscribeOptions, Transcript,
};
use crate::{PolishEngine, TranscriptionEngine};

/// A deterministic `ModelDownloader`: emits a fixed number of progress ticks up
/// to the model's size, then yields a fake on-disk handle - no network, no disk.
/// Onboarding drives this so the download step is testable (UX.md 6 step 4);
/// `fail_after_tick` exercises the interrupted-download path (E8).
pub struct FakeModelDownloader {
    ticks: u64,
    fail_after_tick: Option<u64>,
}

impl Default for FakeModelDownloader {
    fn default() -> Self {
        Self {
            ticks: 4,
            fail_after_tick: None,
        }
    }
}

impl FakeModelDownloader {
    /// Fail partway through, after emitting `tick` progress callbacks (E8).
    pub fn failing_after(tick: u64) -> Self {
        Self {
            ticks: 4,
            fail_after_tick: Some(tick),
        }
    }
}

impl ModelDownloader for FakeModelDownloader {
    fn download(
        &self,
        spec: &ModelSpec,
        progress: &ProgressSink,
    ) -> Result<ModelHandle, DownloadError> {
        let total = spec.size_bytes;
        for tick in 1..=self.ticks {
            if self.fail_after_tick == Some(tick - 1) {
                return Err(DownloadError::Transport(
                    "simulated interruption".to_owned(),
                ));
            }
            let received = (total * tick) / self.ticks;
            progress(received, total);
        }
        Ok(ModelHandle {
            path: format!("/fake/models/{}.bin", spec.id).into(),
        })
    }
}

/// A `TranscriptionEngine` that returns a fixed transcript.
pub struct FakeTranscriptionEngine {
    transcript: Transcript,
    loaded: bool,
}

impl FakeTranscriptionEngine {
    pub fn new(transcript: Transcript) -> Self {
        Self {
            transcript,
            loaded: false,
        }
    }

    /// Convenience: a single-segment transcript saying `text`.
    pub fn speaking(text: &str) -> Self {
        Self::new(Transcript {
            segments: vec![Segment {
                text: text.to_owned(),
                t0: 0.0,
                t1: 1.0,
                confidence: 1.0,
            }],
            language: LanguageTag::from("en"),
        })
    }
}

impl TranscriptionEngine for FakeTranscriptionEngine {
    fn id(&self) -> EngineId {
        EngineId::Fake
    }

    fn load(&mut self, _model: &ModelHandle, _opts: &EngineOptions) -> Result<(), EngineError> {
        self.loaded = true;
        Ok(())
    }

    fn unload(&mut self) {
        self.loaded = false;
    }

    fn is_loaded(&self) -> bool {
        self.loaded
    }

    fn transcribe(
        &self,
        _audio: &AudioBuffer,
        _opts: &TranscribeOptions,
    ) -> Result<Transcript, EngineError> {
        if !self.loaded {
            return Err(EngineError::NotLoaded);
        }
        Ok(self.transcript.clone())
    }
}

/// What a `FakePolishEngine` does with every input.
pub enum FakePolishBehavior {
    /// Return the raw text unchanged.
    Echo,
    /// Return this fixed text.
    Fixed(String),
    /// Reject with this reason (deadline miss, similarity guard, ...).
    Reject(PolishRejection),
}

/// A `PolishEngine` with scripted behavior.
pub struct FakePolishEngine {
    behavior: FakePolishBehavior,
    loaded: bool,
}

impl FakePolishEngine {
    pub fn new(behavior: FakePolishBehavior) -> Self {
        Self {
            behavior,
            loaded: false,
        }
    }
}

impl PolishEngine for FakePolishEngine {
    fn id(&self) -> EngineId {
        EngineId::Fake
    }

    fn load(&mut self, _model: &ModelHandle, _opts: &EngineOptions) -> Result<(), EngineError> {
        self.loaded = true;
        Ok(())
    }

    fn unload(&mut self) {
        self.loaded = false;
    }

    fn is_loaded(&self) -> bool {
        self.loaded
    }

    fn polish(
        &self,
        raw: &str,
        _profile: &PolishProfile,
        _deadline: Duration,
    ) -> Result<PolishOutcome, EngineError> {
        if !self.loaded {
            return Err(EngineError::NotLoaded);
        }
        Ok(match &self.behavior {
            FakePolishBehavior::Echo => PolishOutcome::Polished {
                text: raw.to_owned(),
            },
            FakePolishBehavior::Fixed(text) => PolishOutcome::Polished { text: text.clone() },
            FakePolishBehavior::Reject(reason) => PolishOutcome::Rejected { reason: *reason },
        })
    }
}
