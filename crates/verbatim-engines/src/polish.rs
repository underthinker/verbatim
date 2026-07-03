use std::time::Duration;

use crate::types::{
    EngineError, EngineId, EngineOptions, ModelHandle, PolishOutcome, PolishProfile,
};

/// LLM text-polish engine with a resident context (ARCHITECTURE.md 4.3).
///
/// Implementations run at temperature 0 and must respect the deadline: a miss
/// returns `PolishOutcome::Rejected` rather than blocking the pipeline.
pub trait PolishEngine: Send + Sync {
    fn id(&self) -> EngineId;

    fn load(&mut self, model: &ModelHandle, opts: &EngineOptions) -> Result<(), EngineError>;

    fn unload(&mut self);

    fn is_loaded(&self) -> bool;

    /// Polish a raw transcript under `profile`, racing `deadline`.
    ///
    /// The similarity guard belongs to the caller (core polish pipeline);
    /// engines only generate and may self-reject on deadline.
    fn polish(
        &self,
        raw: &str,
        profile: &PolishProfile,
        deadline: Duration,
    ) -> Result<PolishOutcome, EngineError>;
}
