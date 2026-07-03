use crate::types::{
    AudioBuffer, EngineError, EngineId, EngineOptions, ModelHandle, TranscribeOptions, Transcript,
};

/// Batch speech-to-text engine (ARCHITECTURE.md 4.2).
///
/// Streaming is deliberately absent from the v1 trait: batch beat the latency
/// budget (spike 3). A `StreamingTranscriptionEngine` extension trait is
/// reserved post-v1.
pub trait TranscriptionEngine: Send + Sync {
    fn id(&self) -> EngineId;

    /// Load a model and keep it resident; engines stay loaded after first use
    /// and unload on idle or memory pressure (spike 3: load is cheap but not free).
    fn load(&mut self, model: &ModelHandle, opts: &EngineOptions) -> Result<(), EngineError>;

    fn unload(&mut self);

    fn is_loaded(&self) -> bool;

    /// Transcribe a complete utterance of 16 kHz mono f32 audio.
    fn transcribe(
        &self,
        audio: &AudioBuffer,
        opts: &TranscribeOptions,
    ) -> Result<Transcript, EngineError>;
}
