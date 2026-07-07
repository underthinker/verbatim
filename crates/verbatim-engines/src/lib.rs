//! Engine layer: `TranscriptionEngine` and `PolishEngine` traits, the shared
//! audio/text types, the engine registry, and fake implementations for tests.
//!
//! Real implementations (whisper.cpp, sherpa-onnx, llama.cpp) are feature-gated
//! and land during M1 wire-up; see ARCHITECTURE.md sections 4.2 and 4.3.

pub mod fake;
#[cfg(feature = "llama-cpp")]
mod llama;
pub mod model;
mod polish;
mod registry;
mod transcribe;
mod types;
#[cfg(feature = "whisper-cpp")]
mod whisper;

#[cfg(feature = "llama-cpp")]
pub use llama::LlamaPolishEngine;
pub use polish::PolishEngine;
pub use registry::EngineRegistry;
pub use transcribe::TranscriptionEngine;
pub use types::*;
#[cfg(feature = "whisper-cpp")]
pub use whisper::WhisperCppEngine;
