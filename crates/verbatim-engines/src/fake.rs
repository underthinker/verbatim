//! Fake engines: the deterministic test seam for everything above the engine
//! layer (ENGINEERING.md section 4, E2E smoke).

use std::time::Duration;

use crate::types::{
    AudioBuffer, EngineError, EngineId, EngineOptions, LanguageTag, ModelHandle, PolishOutcome,
    PolishProfile, PolishRejection, Segment, TranscribeOptions, Transcript,
};
use crate::{PolishEngine, TranscriptionEngine};

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
