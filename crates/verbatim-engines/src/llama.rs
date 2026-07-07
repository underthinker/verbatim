//! llama.cpp text polish via `llama-cpp-2` (ARCHITECTURE.md 4.3), behind the
//! `llama-cpp` feature. The fakes remain the default test seam.
//!
//! Architectural constraints (spike 4):
//!
//! - **Temperature 0.** Polish must be deterministic and meaning-preserving, so
//!   generation is greedy (`LlamaSampler::greedy`), never sampled.
//! - **Deadline-bounded.** Generation checks the elapsed time before every
//!   *generation* decode; a miss self-rejects with
//!   `PolishRejection::DeadlineMissed` rather than blocking the pipeline (the
//!   caller then injects raw). The one-shot prompt prefill runs to completion
//!   (a single decode call is not preemptible), then the loop-top check rejects
//!   before generating a token if the prefill already blew the deadline.
//! - **Weights stay resident.** The expensive `LlamaModel` is loaded once and
//!   reused; the per-call `LlamaContext` borrows the model, so it is created
//!   fresh each `polish` (cheap relative to loading weights) to keep the engine
//!   struct non-self-referential.
//!
//! The similarity guard is deliberately *not* here: it belongs to the caller
//! (core polish pipeline, ARCHITECTURE.md 4.3), so this engine only generates
//! and self-rejects on the deadline.

use std::num::NonZeroU32;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;

use crate::PolishEngine;
use crate::types::{
    Backend, EngineError, EngineId, EngineOptions, ModelHandle, PolishOutcome, PolishProfile,
    PolishRejection,
};

/// Context window for polish. Dictation utterances are short; the prompt is the
/// system template + few-shot + raw transcript, which fits comfortably.
const N_CTX: u32 = 4096;

/// Hard cap on generated tokens: a polished dictation is never longer than a few
/// hundred tokens, so this bounds a runaway generation independent of the
/// deadline (belt and suspenders).
const MAX_OUTPUT_TOKENS: usize = 1024;

/// Stop marker for greedy generation. `build_prompt` frames the transcript as a
/// `Raw:` / `Polished:` completion; a greedy model routinely finishes the answer
/// then continues the pattern with a new `\nRaw:` turn (no EOG). We cut at the
/// first such marker so that hallucinated continuation never lands in the
/// injected text. Kept in lockstep with `build_prompt`'s `Raw:` framing.
const STOP_MARKER: &str = "\nRaw:";

/// The llama.cpp backend must be initialised exactly once per process; it owns
/// global ggml state. Held in a `OnceLock` so multiple engines share it.
static BACKEND: OnceLock<LlamaBackend> = OnceLock::new();

fn backend() -> Result<&'static LlamaBackend, EngineError> {
    // `get_or_init` cannot return a Result, so init eagerly and fall back to a
    // stored-none check. A failed backend init is fatal for the real engine.
    if BACKEND.get().is_none() {
        match LlamaBackend::init() {
            Ok(backend) => {
                // Ignore the race loser: another thread won, its backend is fine.
                let _ = BACKEND.set(backend);
            }
            Err(err) => {
                return Err(EngineError::ModelLoad(format!(
                    "llama backend init failed: {err}"
                )));
            }
        }
    }
    BACKEND
        .get()
        .ok_or_else(|| EngineError::ModelLoad("llama backend unavailable".to_owned()))
}

/// A resident llama.cpp polish engine.
#[derive(Default)]
pub struct LlamaPolishEngine {
    model: Option<LlamaModel>,
    threads: Option<usize>,
}

impl LlamaPolishEngine {
    pub fn new() -> Self {
        Self::default()
    }
}

impl PolishEngine for LlamaPolishEngine {
    fn id(&self) -> EngineId {
        EngineId::LlamaCpp
    }

    fn load(&mut self, model: &ModelHandle, opts: &EngineOptions) -> Result<(), EngineError> {
        let backend = backend()?;
        let path = &model.path;

        // Backend policy mirrors whisper (spike 3): a pinned CPU backend offloads
        // nothing; anything else (or `None`) tries full GPU offload then falls
        // back to CPU-only if the GPU context fails to build.
        let gpu_attempts: &[u32] = match opts.backend {
            Some(Backend::Cpu) => &[0],
            Some(_) => &[u32::MAX],
            None => &[u32::MAX, 0],
        };

        let mut last_error = None;
        for &n_gpu_layers in gpu_attempts {
            let params = LlamaModelParams::default().with_n_gpu_layers(n_gpu_layers);
            match LlamaModel::load_from_file(backend, path, &params) {
                Ok(model) => {
                    tracing::info!(n_gpu_layers, model = %path.display(), "llama polish model loaded");
                    self.model = Some(model);
                    self.threads = opts.threads;
                    return Ok(());
                }
                Err(err) => {
                    tracing::warn!(
                        n_gpu_layers,
                        ?err,
                        "llama load attempt failed; trying next backend"
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
        self.model = None;
    }

    fn is_loaded(&self) -> bool {
        self.model.is_some()
    }

    fn polish(
        &self,
        raw: &str,
        profile: &PolishProfile,
        deadline: Duration,
    ) -> Result<PolishOutcome, EngineError> {
        let model = self.model.as_ref().ok_or(EngineError::NotLoaded)?;
        let backend = backend()?;
        let start = Instant::now();

        let prompt = build_prompt(profile, raw);

        // Fresh per-call context (see module docs): weights are resident, this is
        // the cheap part.
        let n_ctx = NonZeroU32::new(N_CTX);
        let mut ctx_params = LlamaContextParams::default().with_n_ctx(n_ctx);
        if let Some(threads) = self.threads {
            let threads = threads.min(i32::MAX as usize) as i32;
            ctx_params = ctx_params
                .with_n_threads(threads)
                .with_n_threads_batch(threads);
        }
        let mut ctx = model
            .new_context(backend, ctx_params)
            .map_err(|err| EngineError::Inference(err.to_string()))?;

        let tokens = model
            .str_to_token(&prompt, AddBos::Always)
            .map_err(|err| EngineError::Inference(err.to_string()))?;
        // Reserve KV room for generation: the prompt plus a full-length output
        // must fit the context, or `ctx.decode` errors mid-generation once the
        // window fills. Reject up front instead.
        if tokens.len() + MAX_OUTPUT_TOKENS >= N_CTX as usize {
            return Err(EngineError::Inference(
                "prompt leaves no room in the polish context window".to_owned(),
            ));
        }

        let mut batch = LlamaBatch::new(N_CTX as usize, 1);
        let last = tokens.len() - 1;
        for (i, token) in tokens.iter().enumerate() {
            batch
                .add(*token, i as i32, &[0], i == last)
                .map_err(|err| EngineError::Inference(err.to_string()))?;
        }
        ctx.decode(&mut batch)
            .map_err(|err| EngineError::Inference(err.to_string()))?;

        // Greedy = temperature 0 (spike 4): deterministic, meaning-preserving.
        let mut sampler = LlamaSampler::greedy();
        let mut output = String::new();
        // One decoder for the whole generation so a multi-byte char split across
        // two token pieces is reassembled correctly.
        let mut decoder = encoding_rs::UTF_8.new_decoder();
        let mut n_cur = batch.n_tokens();
        let mut generated = 0usize;

        loop {
            // Deadline is checked *before* the next decode so a slow machine
            // degrades to raw instead of blocking (ARCHITECTURE.md 4.3).
            if start.elapsed() >= deadline {
                return Ok(PolishOutcome::Rejected {
                    reason: PolishRejection::DeadlineMissed,
                });
            }

            let token = sampler.sample(&ctx, batch.n_tokens() - 1);
            sampler.accept(token);

            if model.is_eog_token(token) {
                break;
            }

            let piece = model
                .token_to_piece(token, &mut decoder, false, None)
                .map_err(|err| EngineError::Inference(err.to_string()))?;
            output.push_str(&piece);

            // Stop if the model rolled past the answer into a new `Raw:` turn:
            // truncate the marker (and everything after) so continuation never
            // reaches the injected text.
            if let Some(idx) = output.find(STOP_MARKER) {
                output.truncate(idx);
                break;
            }

            generated += 1;
            if generated >= MAX_OUTPUT_TOKENS {
                break;
            }

            batch.clear();
            batch
                .add(token, n_cur, &[0], true)
                .map_err(|err| EngineError::Inference(err.to_string()))?;
            n_cur += 1;
            ctx.decode(&mut batch)
                .map_err(|err| EngineError::Inference(err.to_string()))?;
        }

        Ok(PolishOutcome::Polished {
            text: output.trim().to_owned(),
        })
    }
}

/// Assemble the polish prompt from the profile (system template + few-shot +
/// dictionary) and the raw transcript. Prompt *content* (templates, few-shot
/// wording) becomes versioned assets in Phase E; this only defines the framing
/// that turns a `PolishProfile` into a single string the model sees.
fn build_prompt(profile: &PolishProfile, raw: &str) -> String {
    let mut prompt = String::new();
    if !profile.system_prompt.is_empty() {
        prompt.push_str(&profile.system_prompt);
        prompt.push_str("\n\n");
    }
    if !profile.dictionary.is_empty() {
        prompt.push_str("Preserve the exact spelling and casing of these terms: ");
        prompt.push_str(&profile.dictionary.join(", "));
        prompt.push_str(".\n\n");
    }
    for example in &profile.few_shot {
        prompt.push_str("Raw: ");
        prompt.push_str(&example.raw);
        prompt.push_str("\nPolished: ");
        prompt.push_str(&example.polished);
        prompt.push_str("\n\n");
    }
    prompt.push_str("Raw: ");
    prompt.push_str(raw);
    prompt.push_str("\nPolished:");
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::FewShotExample;
    use std::path::PathBuf;

    #[test]
    fn prompt_includes_system_dictionary_fewshot_and_raw() {
        let profile = PolishProfile {
            id: "test".to_owned(),
            system_prompt: "Clean up dictation.".to_owned(),
            few_shot: vec![FewShotExample {
                raw: "um hello".to_owned(),
                polished: "Hello.".to_owned(),
            }],
            dictionary: vec!["PCM".to_owned()],
        };
        let prompt = build_prompt(&profile, "so uh whats a pcm");
        assert!(prompt.contains("Clean up dictation."));
        assert!(prompt.contains("PCM"));
        assert!(prompt.contains("um hello"));
        assert!(prompt.contains("Hello."));
        assert!(prompt.ends_with("Raw: so uh whats a pcm\nPolished:"));
    }

    #[test]
    fn stop_marker_matches_the_fewshot_framing() {
        // The generation loop cuts at STOP_MARKER to drop hallucinated `Raw:`
        // continuations. That only works if the marker is exactly how
        // build_prompt delimits turns; assert the coupling so a reframing that
        // drops the marker fails here instead of leaking garbage to injection.
        let profile = PolishProfile {
            id: "t".to_owned(),
            system_prompt: String::new(),
            few_shot: vec![FewShotExample {
                raw: "a".to_owned(),
                polished: "A.".to_owned(),
            }],
            dictionary: Vec::new(),
        };
        assert!(build_prompt(&profile, "b").contains(STOP_MARKER));
    }

    /// A GGUF polish model to exercise the real FFI path. Skipped when unset so
    /// the default suite needs no cached model; a local feature build points
    /// this at a small instruct model.
    fn model_from_env() -> Option<PathBuf> {
        std::env::var_os("VERBATIM_POLISH_MODEL")
            .map(PathBuf::from)
            .filter(|path| path.exists())
    }

    #[test]
    fn loads_and_polishes_deterministically() {
        let Some(path) = model_from_env() else {
            eprintln!("skipping llama integration test; set VERBATIM_POLISH_MODEL to a GGUF model");
            return;
        };

        let mut engine = LlamaPolishEngine::new();
        assert!(!engine.is_loaded());
        assert_eq!(engine.id(), EngineId::LlamaCpp);

        engine
            .load(&ModelHandle { path }, &EngineOptions::default())
            .expect("polish model should load on some backend");
        assert!(engine.is_loaded());

        let profile = PolishProfile {
            id: "default".to_owned(),
            system_prompt: "Rewrite the raw dictation as clean text. Keep the meaning.".to_owned(),
            few_shot: Vec::new(),
            dictionary: Vec::new(),
        };
        let deadline = Duration::from_secs(30);

        // Temperature 0 => identical output across runs.
        let first = engine.polish("um so like hello there", &profile, deadline);
        let second = engine.polish("um so like hello there", &profile, deadline);
        match (first, second) {
            (Ok(PolishOutcome::Polished { text: a }), Ok(PolishOutcome::Polished { text: b })) => {
                assert_eq!(a, b, "temperature 0 must be deterministic");
                assert!(!a.is_empty());
            }
            other => panic!("expected deterministic polished output, got {other:?}"),
        }

        // A zero deadline must self-reject, never block.
        assert!(matches!(
            engine.polish("hello", &profile, Duration::ZERO),
            Ok(PolishOutcome::Rejected {
                reason: PolishRejection::DeadlineMissed
            })
        ));

        engine.unload();
        assert!(!engine.is_loaded());
    }
}
