//! The live input level the overlay waveform is fed from (ARCHITECTURE.md 4.1).
//!
//! Compiled only under `--features cpal-audio`. Opening the default input
//! device needs a real microphone and, on macOS, a Microphone permission grant,
//! so this gates behind `VERBATIM_AUDIO_E2E=1` and never runs on headless CI.
//!
//! The RMS maths is unit-tested in `verbatim-core`; what is proven here is the
//! wiring the unit tests cannot see - that the worker thread publishes a level
//! while a real stream is running, and clears it once the recording ends.
#![cfg(feature = "cpal-audio")]

use std::thread::sleep;
use std::time::Duration;

use verbatim_platform::AudioCapture;
use verbatim_platform::audio::CpalAudioCapture;

fn e2e_enabled() -> bool {
    std::env::var_os("VERBATIM_AUDIO_E2E").is_some_and(|v| v == "1")
}

#[test]
fn a_live_capture_reports_a_level_and_clears_it_on_stop() {
    if !e2e_enabled() {
        eprintln!("skipping: set VERBATIM_AUDIO_E2E=1 with a microphone available");
        return;
    }

    let mut capture = CpalAudioCapture::new();
    assert_eq!(
        capture.input_level(),
        0.0,
        "an idle capture reports silence, not a stale level"
    );

    capture.start().expect("default input device opens");

    // Let the worker drain the ring a few times (it ticks every 15 ms).
    let mut levels = Vec::new();
    for _ in 0..20 {
        sleep(Duration::from_millis(25));
        levels.push(capture.input_level());
    }

    let buffer = capture.stop().expect("capture stops cleanly");
    assert!(!buffer.samples.is_empty(), "a live capture yields samples");

    assert!(
        levels
            .iter()
            .all(|rms| rms.is_finite() && *rms >= 0.0 && *rms <= 1.0),
        "levels stay a bounded RMS in [0, 1]: {levels:?}"
    );
    assert!(
        levels.iter().any(|rms| *rms > 0.0),
        "a real microphone always carries some self-noise, so at least one \
         tick must report a level above zero: {levels:?}"
    );
    assert_eq!(
        capture.input_level(),
        0.0,
        "a finished recording must not leave the overlay's last bar standing"
    );
}
