//! Input-level maths shared by the silence check and the overlay waveform
//! (ARCHITECTURE.md 4.1).
//!
//! ARCHITECTURE.md specifies Silero VAD here. It is deliberately not used: the
//! hotkey bounds the utterance, so the only two consumers of a speech verdict
//! are the "didn't catch anything" flash (UX.md 2) and the overlay waveform,
//! and both are answered by frame energy. An ONNX runtime plus a shipped model
//! would buy end-of-speech tail detection, which push-to-talk and toggle make
//! redundant.
//!
//! ponytail: energy gate, not a real VAD. Swap in Silero if a future surface
//! needs to distinguish speech from steady non-speech noise (a fan, music).

/// Frame length for the energy gate. 20 ms is the usual VAD frame: long enough
/// to average out a glottal pulse, short enough that one loud syllable in an
/// otherwise silent buffer still lands inside a single frame.
const FRAME_MS: usize = 20;

/// Frame energy below which a whole recording counts as silence, in RMS over
/// f32 samples in [-1.0, 1.0]. -40 dBFS: comfortably above the self-noise of a
/// real microphone (around -60 dBFS) and far below any speech, which sustains
/// -26 dBFS or louder even when someone mumbles a room away from the mic.
const SILENCE_PEAK_RMS: f32 = 0.01;

/// Root-mean-square amplitude of one frame. Empty input is silent, not NaN.
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_squares: f32 = samples.iter().map(|s| s * s).sum();
    (sum_squares / samples.len() as f32).sqrt()
}

/// The loudest 20 ms frame in the buffer, by RMS.
///
/// Peak-of-frames, not RMS-of-buffer: a two-second recording holding one short
/// "yes" averages down to near-silence over its whole length, and rejecting it
/// would discard the user's actual dictation.
pub fn peak_frame_rms(samples: &[f32], sample_rate_hz: u32) -> f32 {
    if samples.is_empty() || sample_rate_hz == 0 {
        return 0.0;
    }
    let frame = (sample_rate_hz as usize * FRAME_MS / 1000).max(1);
    samples.chunks(frame).map(rms).fold(0.0, f32::max)
}

/// Whether a captured buffer carries no speech energy at all: a muted mic, a
/// dead device, or a hotkey tapped by accident. The caller returns softly to
/// Idle (UX.md 2), never an error dialog.
pub fn is_silence(samples: &[f32], sample_rate_hz: u32) -> bool {
    peak_frame_rms(samples, sample_rate_hz) < SILENCE_PEAK_RMS
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    const RATE: u32 = 16_000;

    /// A sine of the given amplitude; RMS is amplitude / sqrt(2).
    fn tone(amplitude: f32, samples: usize) -> Vec<f32> {
        (0..samples)
            .map(|i| amplitude * (TAU * 200.0 * i as f32 / RATE as f32).sin())
            .collect()
    }

    #[test]
    fn empty_and_zeroed_buffers_are_silence() {
        assert!(is_silence(&[], RATE));
        assert!(is_silence(&vec![0.0; RATE as usize], RATE));
    }

    #[test]
    fn a_zero_sample_rate_is_silence_rather_than_a_divide_by_zero() {
        assert!(is_silence(&tone(0.5, 1000), 0));
    }

    #[test]
    fn speech_level_audio_is_not_silence() {
        // -17 dBFS RMS: an ordinary speaking level.
        assert!(!is_silence(&tone(0.2, RATE as usize), RATE));
    }

    #[test]
    fn microphone_self_noise_is_silence() {
        // -60 dBFS, an order of magnitude under the gate.
        assert!(is_silence(&tone(0.0014, RATE as usize), RATE));
    }

    /// The reason the gate is peak-of-frames and not RMS-of-buffer: a single
    /// "yes" inside a long pause averages down under the gate, yet the buffer
    /// plainly contains the user's dictation. Rejecting it would discard words
    /// they actually said.
    #[test]
    fn one_short_utterance_in_a_long_silence_is_not_silence() {
        let mut samples = vec![0.0; RATE as usize * 8];
        let burst = tone(0.2, RATE as usize / 50); // 20 ms, one frame
        samples[..burst.len()].copy_from_slice(&burst);

        assert!(
            rms(&samples) < SILENCE_PEAK_RMS,
            "whole-buffer RMS averages under the gate: {}",
            rms(&samples)
        );
        assert!(!is_silence(&samples, RATE), "but a frame carries speech");
    }

    #[test]
    fn the_gate_sits_between_the_two_boundary_cases() {
        assert!(is_silence(&tone(SILENCE_PEAK_RMS, RATE as usize), RATE));
        assert!(!is_silence(
            &tone(SILENCE_PEAK_RMS * 4.0, RATE as usize),
            RATE
        ));
    }

    #[test]
    fn rms_of_a_sine_is_amplitude_over_root_two() {
        let measured = rms(&tone(1.0, RATE as usize));
        assert!((measured - 1.0 / 2.0_f32.sqrt()).abs() < 1e-3, "{measured}");
    }
}
