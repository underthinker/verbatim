//! Per-machine polish-deadline calibration (M3 Phase E; ROADMAP M3 criterion 2).
//!
//! The polish deadline is the wall-clock budget the pipeline gives the LLM before
//! it degrades to raw (ARCHITECTURE.md 4.3). A fixed budget is wrong across
//! hardware: a fast Mac finishes in 300 ms, a slow laptop needs a second. The
//! honest knob is the machine's own generation speed - milliseconds per output
//! token - measured once (onboarding, or the polish-quality benchmark) and scaled
//! by a typical polished-dictation length.
//!
//! This module is the pure scaling step: `ms/token` in, a clamped `Duration` out.
//! Measuring `ms/token` needs the real engine and so lives at the call site (the
//! benchmark, and onboarding once the llama engine is wired into the app).
//!
//! ponytail: the onboarding auto-calibrate step is deferred until the daemon
//! wires the real `LlamaPolishEngine` (today it runs `FakePolishEngine`, so a
//! measurement would be meaningless). The polish bench already prints the
//! per-machine deadline; wire that value into `Config::polish_deadline_ms` from
//! the onboarding polish-model step when the real engine lands.

use std::time::Duration;

/// Output tokens in a typical polished 10 s utterance (~25 spoken words at
/// ~1.3 tokens/word plus punctuation, with headroom). The deadline scales off
/// this so calibration targets the length the acceptance criterion measures.
const TYPICAL_OUTPUT_TOKENS: f64 = 64.0;

/// Never degrade to raw faster than this, however fast the measurement looked -
/// a single warm generation can undercount, and a too-tight deadline rejects
/// good polish.
const MIN_DEADLINE: Duration = Duration::from_millis(300);

/// Never wait longer than this: past it the user is staring at nothing, so raw
/// is the better answer even on a slow machine (UX.md 2).
const MAX_DEADLINE: Duration = Duration::from_millis(3000);

/// Turn a measured per-token generation cost into the polish deadline, clamped to
/// the sane range. `ms_per_token` comes from timing a real polish generation and
/// dividing by the tokens produced.
pub fn deadline_from_ms_per_token(ms_per_token: f64) -> Duration {
    if !ms_per_token.is_finite() || ms_per_token <= 0.0 {
        return MIN_DEADLINE;
    }
    let millis = ms_per_token * TYPICAL_OUTPUT_TOKENS;
    Duration::from_millis(millis as u64).clamp(MIN_DEADLINE, MAX_DEADLINE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_rate_scales_to_the_budget() {
        // ~10 ms/token on reference Apple Silicon (plan Phase E) lands near the
        // 700 ms polish budget, not the raw 1500 ms default.
        let d = deadline_from_ms_per_token(10.0);
        assert_eq!(d, Duration::from_millis(640));
    }

    #[test]
    fn absurd_measurements_clamp() {
        assert_eq!(deadline_from_ms_per_token(0.01), MIN_DEADLINE);
        assert_eq!(deadline_from_ms_per_token(1000.0), MAX_DEADLINE);
    }

    #[test]
    fn degenerate_input_is_the_floor_not_a_panic() {
        assert_eq!(deadline_from_ms_per_token(0.0), MIN_DEADLINE);
        assert_eq!(deadline_from_ms_per_token(-5.0), MIN_DEADLINE);
        assert_eq!(deadline_from_ms_per_token(f64::NAN), MIN_DEADLINE);
    }
}
