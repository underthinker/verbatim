//! Raw-latency bench for the M1 acceptance criterion: p50 stop-to-text under
//! 800 ms for a 10 s utterance with a resident model (ROADMAP M1, spike 3).
//!
//! Feeds the recorded fixture (`benches/fixtures/`, real speech - spike 3
//! forbids synthesized audio as the graded input) through a resident
//! `WhisperCppEngine` and measures `transcribe` wall time over N iterations.
//! Model load is deliberately outside the timed region (resident-model
//! criterion); a warm-up run absorbs one-time backend initialisation.
//!
//! Environment:
//! - `VERBATIM_WHISPER_MODEL`: ggml model path. Unset: the bench skips (exit
//!   0) so `cargo bench` works on machines without a cached model; set
//!   `VERBATIM_BENCH_REQUIRE=1` to make that a failure (CI).
//! - `VERBATIM_BENCH_ITERATIONS`: sample count (default 20).
//! - `VERBATIM_BENCH_BASELINE`: per-runner baseline name; compares against
//!   `benches/baselines/<name>.json` and fails on a >20% p50 regression
//!   (ENGINEERING.md 6). A missing baseline file warns and passes so the
//!   first run on a new runner can mint one from the printed JSON.
//! - `VERBATIM_BENCH_MAX_P50_MS`: hard budget assert (800 on Apple Silicon).

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use verbatim_engines::{
    AudioBuffer, EngineOptions, LanguageTag, ModelHandle, PIPELINE_SAMPLE_RATE_HZ,
    TranscribeOptions, TranscriptionEngine, WhisperCppEngine,
};

const DEFAULT_ITERATIONS: usize = 20;
const REGRESSION_LIMIT: f64 = 1.20;

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(message) => {
            eprintln!("latency bench: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let Some(model_path) = std::env::var_os("VERBATIM_WHISPER_MODEL").map(PathBuf::from) else {
        let message = "VERBATIM_WHISPER_MODEL not set; skipping latency bench";
        if std::env::var_os("VERBATIM_BENCH_REQUIRE").is_some() {
            return Err(message.replace("skipping", "cannot run"));
        }
        eprintln!("latency bench: {message}");
        return Ok(ExitCode::SUCCESS);
    };

    let fixture = fixture_path("jfk.wav");
    let audio = load_wav(&fixture)?;
    let duration = audio.duration();

    let mut engine = WhisperCppEngine::new();
    engine
        .load(&ModelHandle { path: model_path }, &EngineOptions::default())
        .map_err(|err| format!("model load failed: {err}"))?;

    // Warm-up: absorbs one-time backend initialisation (Metal shader compile,
    // caches) that a resident engine pays once, not per dictation.
    let text = transcribe_once(&engine, &audio)?.1;
    if text.trim().is_empty() {
        return Err("fixture transcribed to empty text; timing would be meaningless".to_owned());
    }

    let iterations = std::env::var("VERBATIM_BENCH_ITERATIONS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(DEFAULT_ITERATIONS)
        .max(1);

    let mut samples_ms = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        samples_ms.push(transcribe_once(&engine, &audio)?.0);
    }
    samples_ms.sort_by(f64::total_cmp);

    let p50 = percentile(&samples_ms, 50.0);
    let p95 = percentile(&samples_ms, 95.0);

    println!(
        "latency bench: {iterations} iterations over {:.1}s fixture: p50 {p50:.1} ms, p95 {p95:.1} ms",
        duration.as_secs_f64()
    );
    println!("{{\"p50_ms\": {p50:.1}, \"p95_ms\": {p95:.1}, \"iterations\": {iterations}}}");

    // Empty values count as unset so CI can pass the env var through a matrix
    // field that only some legs populate.
    if let Ok(budget) = std::env::var("VERBATIM_BENCH_MAX_P50_MS")
        && !budget.is_empty()
    {
        let budget: f64 = budget
            .parse()
            .map_err(|_| format!("VERBATIM_BENCH_MAX_P50_MS is not a number: {budget}"))?;
        if p50 > budget {
            return Err(format!("p50 {p50:.1} ms exceeds the {budget} ms budget"));
        }
        println!("latency bench: p50 within the {budget} ms budget");
    }

    if let Ok(name) = std::env::var("VERBATIM_BENCH_BASELINE")
        && !name.is_empty()
    {
        check_baseline(&name, p50)?;
    }

    Ok(ExitCode::SUCCESS)
}

/// One timed transcription; returns (wall ms, transcript text).
fn transcribe_once(
    engine: &WhisperCppEngine,
    audio: &AudioBuffer,
) -> Result<(f64, String), String> {
    // Pin the language: autodetect adds a variable-cost pass the dictation
    // pipeline does not pay, and the fixture is English.
    let options = TranscribeOptions {
        language: Some(LanguageTag::from("en")),
    };
    let started = Instant::now();
    let transcript = engine
        .transcribe(audio, &options)
        .map_err(|err| format!("transcription failed: {err}"))?;
    Ok((started.elapsed().as_secs_f64() * 1000.0, transcript.text()))
}

/// Nearest-rank percentile over an ascending-sorted, non-empty sample set.
fn percentile(sorted_ms: &[f64], pct: f64) -> f64 {
    if sorted_ms.is_empty() {
        return 0.0;
    }
    let rank = ((pct / 100.0) * sorted_ms.len() as f64).ceil() as usize;
    sorted_ms[rank.clamp(1, sorted_ms.len()) - 1]
}

/// Compare against the committed per-runner baseline; >20% p50 regression
/// fails the bench (ENGINEERING.md 6).
fn check_baseline(name: &str, p50: f64) -> Result<(), String> {
    let path = fixture_path("..")
        .join("baselines")
        .join(format!("{name}.json"));
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(_) => {
            eprintln!(
                "latency bench: no baseline at {}; commit the printed JSON there to arm the regression gate",
                path.display()
            );
            return Ok(());
        }
    };
    let baseline_p50 = parse_p50(&content)
        .ok_or_else(|| format!("baseline {} has no numeric p50_ms field", path.display()))?;
    let limit = baseline_p50 * REGRESSION_LIMIT;
    if p50 > limit {
        return Err(format!(
            "p50 {p50:.1} ms regressed >20% against baseline {baseline_p50:.1} ms (limit {limit:.1} ms)"
        ));
    }
    println!("latency bench: within 20% of the {baseline_p50:.1} ms baseline");
    Ok(())
}

/// Minimal extraction of `"p50_ms": <number>` from the baseline JSON; the
/// bench takes no serde dependency for one field.
fn parse_p50(json: &str) -> Option<f64> {
    let start = json.find("\"p50_ms\"")? + "\"p50_ms\"".len();
    let rest = json[start..].trim_start().strip_prefix(':')?.trim_start();
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// Resolve a path under the workspace-level `benches/fixtures/` directory.
fn fixture_path(file: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("benches/fixtures")
        .join(file)
}

/// Decode the 16 kHz mono fixture WAV into the pipeline's f32 buffer.
fn load_wav(path: &Path) -> Result<AudioBuffer, String> {
    let reader = hound::WavReader::open(path)
        .map_err(|err| format!("cannot open fixture {}: {err}", path.display()))?;
    let spec = reader.spec();
    if spec.sample_rate != PIPELINE_SAMPLE_RATE_HZ || spec.channels != 1 {
        return Err(format!(
            "fixture must be {PIPELINE_SAMPLE_RATE_HZ} Hz mono; got {} Hz, {} channels",
            spec.sample_rate, spec.channels
        ));
    }
    let samples = match spec.sample_format {
        hound::SampleFormat::Int => {
            let shift = u32::from(spec.bits_per_sample.clamp(1, 16)) - 1;
            let scale = (1i32 << shift) as f32;
            reader
                .into_samples::<i16>()
                .map(|sample| sample.map(|value| f32::from(value) / scale))
                .collect::<Result<Vec<f32>, _>>()
        }
        hound::SampleFormat::Float => reader.into_samples::<f32>().collect(),
    }
    .map_err(|err| format!("cannot decode fixture {}: {err}", path.display()))?;
    Ok(AudioBuffer {
        samples,
        sample_rate_hz: PIPELINE_SAMPLE_RATE_HZ,
    })
}
