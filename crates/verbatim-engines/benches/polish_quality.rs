//! Polish-quality + latency bench for M3 acceptance criteria 1/3/4.
//!
//! Runs a fixed transcript set through a resident `LlamaPolishEngine` under the
//! versioned `default` prompt asset and grades three things:
//!
//! - **Similarity guard** (criterion 1, meaning-preserving): every polished
//!   output must stay within the length-scaled edit-distance guard of its raw.
//!   A hallucinated answer or paraphrase blows the budget and fails the bench.
//! - **Golden expectation** (prompt changes ship deltas): each output is compared
//!   to a committed golden line. A prompt or engine change that moves outputs
//!   fails until the golden is regenerated in the same change - so a prompt edit
//!   cannot land without its reviewed benchmark delta. The first run mints the
//!   golden from the printed block.
//! - **Latency** (criteria 3/4): p50 wall time per polish, gated at >20%
//!   regression against a per-runner baseline, mirroring the whisper harness.
//!
//! It also prints the measured ms/output-token and the deadline that calibrates
//! to (calibration::deadline_from_ms_per_token) - the per-machine value the
//! onboarding calibrator will persist.
//!
//! Environment (mirrors the latency bench):
//! - `VERBATIM_POLISH_MODEL`: GGUF path. Unset: skips (exit 0) unless
//!   `VERBATIM_BENCH_REQUIRE=1`.
//! - `VERBATIM_BENCH_ITERATIONS`: latency sample passes over the set (default 5).
//! - `VERBATIM_BENCH_BASELINE`: per-runner p50 baseline name under
//!   `benches/baselines/polish-<name>.json`; >20% p50 regression fails.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use verbatim_engines::{
    EngineOptions, LlamaPolishEngine, ModelHandle, PolishEngine, PolishOutcome, PolishProfile,
    prompts,
};

/// Fixed transcript set: raw dictation the polish engine must clean without
/// altering meaning. Kept small and inline - it is graded input, not a corpus.
const TRANSCRIPTS: &[&str] = &[
    "um so hey can you uh can you send over the the latest draft of the architecture doc when you get a chance i wanna like review the section on on text injection before our meeting tomorrow morning at ten",
    "hey um quick question uh what time does the the standup start tomorrow and and should i prepare anything",
    "um yeah ok sounds good",
    "so i think we should uh we should ship the the feature on friday if if the tests pass",
    "so um i noticed that the the login page is is kinda broken on mobile like when you tap the the email field the keyboard covers the the submit button so you cant actually see what youre typing",
    "hey can we uh can we move our one on one to like thursday afternoon i have a a conflict come up in the morning that i cant really get out of",
    "ok so the the way the cache works is uh it stores the the response for like five minutes and then after that it it just refetches from the the api so you might see stale data for a bit",
    "sounds good ill uh ill get that done by end of day",
    "um so for the release we still need to uh we need to finish the docs update the changelog and and run the the full test suite before we tag it",
    "yeah i i totally agree with with what you said earlier about about keeping the scope small for for the first version",
];

/// A polish generation is never allowed to self-reject in the bench: we measure
/// true wall time, so the deadline is far above any real budget. Sized for the
/// slowest supported runner - virtualized macOS CI has no Metal and runs with
/// `GGML_NO_I8MM=1`, so a single CPU polish there is orders slower than the
/// ~115 ms reference-hardware p50. Latency regressions are gated separately via
/// the per-runner p50/p95 baseline; this deadline only guards against false
/// self-rejection. ponytail: bump if an even slower runner trips it.
const BENCH_DEADLINE: Duration = Duration::from_secs(300);
const DEFAULT_ITERATIONS: usize = 5;
const REGRESSION_LIMIT: f64 = 1.20;

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(message) => {
            eprintln!("polish bench: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let Some(model_path) = std::env::var_os("VERBATIM_POLISH_MODEL").map(PathBuf::from) else {
        let message = "VERBATIM_POLISH_MODEL not set; skipping polish bench";
        if std::env::var_os("VERBATIM_BENCH_REQUIRE").is_some() {
            return Err(message.replace("skipping", "cannot run"));
        }
        eprintln!("polish bench: {message}");
        return Ok(ExitCode::SUCCESS);
    };

    let content = prompts::load("default").ok_or("default prompt asset failed to load")?;
    let profile = PolishProfile {
        id: "default".to_owned(),
        system_prompt: content.system_prompt,
        few_shot: content.few_shot,
        dictionary: Vec::new(),
    };

    let mut engine = LlamaPolishEngine::new();
    engine
        .load(&ModelHandle { path: model_path }, &EngineOptions::default())
        .map_err(|err| format!("model load failed: {err}"))?;

    // Warm-up absorbs one-time backend init (Metal shader compile) that a
    // resident engine pays once, not per dictation.
    polish_once(&engine, &profile, TRANSCRIPTS[0])?;

    // First graded pass: guard + golden over the deterministic (temp 0) outputs.
    let mut outputs = Vec::with_capacity(TRANSCRIPTS.len());
    for raw in TRANSCRIPTS {
        let (_, polished) = polish_once(&engine, &profile, raw)?;
        if !within_guard(raw, &polished) {
            return Err(format!(
                "similarity guard failed - polish drifted from raw:\n  raw:      {raw}\n  polished: {polished}"
            ));
        }
        outputs.push(polished);
    }
    check_golden(&outputs)?;
    println!(
        "polish bench: {} transcripts within the similarity guard",
        outputs.len()
    );

    // Latency passes: pooled per-polish wall times across the set.
    let iterations = std::env::var("VERBATIM_BENCH_ITERATIONS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(DEFAULT_ITERATIONS)
        .max(1);

    let mut samples_ms = Vec::with_capacity(iterations * TRANSCRIPTS.len());
    let mut total_ms = 0.0;
    let mut total_tokens = 0.0;
    for _ in 0..iterations {
        for raw in TRANSCRIPTS {
            let (ms, polished) = polish_once(&engine, &profile, raw)?;
            samples_ms.push(ms);
            total_ms += ms;
            total_tokens += estimate_tokens(&polished);
        }
    }
    samples_ms.sort_by(f64::total_cmp);

    let p50 = percentile(&samples_ms, 50.0);
    let p95 = percentile(&samples_ms, 95.0);
    let ms_per_token = if total_tokens > 0.0 {
        total_ms / total_tokens
    } else {
        0.0
    };
    let deadline = verbatim_engines::calibration::deadline_from_ms_per_token(ms_per_token);

    println!(
        "polish bench: {iterations}x{} polishes: p50 {p50:.1} ms, p95 {p95:.1} ms, {ms_per_token:.1} ms/token -> deadline {} ms",
        TRANSCRIPTS.len(),
        deadline.as_millis()
    );
    println!(
        "{{\"p50_ms\": {p50:.1}, \"p95_ms\": {p95:.1}, \"ms_per_token\": {ms_per_token:.2}, \"deadline_ms\": {}}}",
        deadline.as_millis()
    );

    // A saturated deadline is the ceiling, not a calibration: this machine is
    // slower than ~47 ms/token, so every polish is *designed* to degrade to raw
    // and the miss rate is 100% by arithmetic, whatever the code does. Grading
    // it would test the runner, not the change. Reference hardware measures
    // ~10 ms/token (640 ms deadline) and is graded normally.
    if verbatim_engines::calibration::is_saturated(deadline) {
        println!(
            "polish bench: {ms_per_token:.1} ms/token saturates the {} ms deadline ceiling; \
             skipping the deadline-miss criterion (needs <47 ms/token hardware)",
            deadline.as_millis()
        );
    } else {
        check_deadline_miss_rate(&engine, &profile, deadline, iterations)?;
    }

    if let Ok(name) = std::env::var("VERBATIM_BENCH_BASELINE")
        && !name.is_empty()
    {
        check_baseline(&name, p50)?;
    }

    Ok(ExitCode::SUCCESS)
}

/// Criterion 2 (ROADMAP M3): deadline-miss rate < 5% for 10 s-scale utterances.
/// Re-run the set under the *calibrated* deadline - not the 60 s bench deadline -
/// and count self-rejections. Calibration scales the budget off this machine's own
/// ms/token against a typical output length (calibration::TYPICAL_OUTPUT_TOKENS,
/// ~2x a real 10 s utterance), so a correctly-tuned deadline degrades almost no
/// real utterance to raw. Measuring it here proves the calibration holds on the
/// machine the bench runs on, and fails CI if a prompt/engine change ever pushes
/// generation past the calibrated budget.
fn check_deadline_miss_rate(
    engine: &LlamaPolishEngine,
    profile: &PolishProfile,
    deadline: Duration,
    iterations: usize,
) -> Result<(), String> {
    const MISS_LIMIT: f64 = 0.05;
    let mut misses = 0usize;
    let mut trials = 0usize;
    for _ in 0..iterations {
        for raw in TRANSCRIPTS {
            trials += 1;
            let outcome = engine
                .polish(raw, profile, deadline)
                .map_err(|err| format!("polish failed under the calibrated deadline: {err}"))?;
            if matches!(outcome, PolishOutcome::Rejected { .. }) {
                misses += 1;
            }
        }
    }
    let miss_rate = misses as f64 / trials.max(1) as f64;
    println!(
        "polish bench: deadline-miss rate {:.1}% ({misses}/{trials}) at the calibrated {} ms deadline",
        miss_rate * 100.0,
        deadline.as_millis()
    );
    if miss_rate > MISS_LIMIT {
        return Err(format!(
            "deadline-miss rate {:.1}% exceeds the 5% criterion at the calibrated {} ms deadline",
            miss_rate * 100.0,
            deadline.as_millis()
        ));
    }
    Ok(())
}

/// One timed polish; returns (wall ms, polished text). A rejection or error at the
/// bench deadline is a real failure - the model should always produce text here.
fn polish_once(
    engine: &LlamaPolishEngine,
    profile: &PolishProfile,
    raw: &str,
) -> Result<(f64, String), String> {
    let started = Instant::now();
    let outcome = engine
        .polish(raw, profile, BENCH_DEADLINE)
        .map_err(|err| format!("polish failed: {err}"))?;
    let ms = started.elapsed().as_secs_f64() * 1000.0;
    match outcome {
        PolishOutcome::Polished { text } => Ok((ms, text)),
        PolishOutcome::Rejected { reason } => Err(format!(
            "polish rejected ({reason:?}) under a {BENCH_DEADLINE:?} deadline"
        )),
    }
}

/// Rough token count for the ms/token calibration estimate. The engine does not
/// surface generated-token counts, so approximate: whitespace words plus a
/// subword factor. Only feeds the per-machine deadline heuristic, not a gate.
fn estimate_tokens(text: &str) -> f64 {
    (text.split_whitespace().count() as f64 * 1.3).max(1.0)
}

/// Compare outputs to the committed golden set. Absent golden: print the block so
/// the run can mint it, and pass (first run on a new prompt/model). Present:
/// any mismatch fails - a prompt or engine change must ship a regenerated golden.
fn check_golden(outputs: &[String]) -> Result<(), String> {
    let path = bench_path("baselines/polish-golden.txt");
    let Ok(content) = std::fs::read_to_string(&path) else {
        eprintln!(
            "polish bench: no golden at {}; commit these lines there to arm the expectation gate:",
            path.display()
        );
        for line in outputs {
            println!("{line}");
        }
        return Ok(());
    };
    let golden: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
    if golden.len() != outputs.len() {
        return Err(format!(
            "golden has {} lines but the set has {} transcripts; regenerate {}",
            golden.len(),
            outputs.len(),
            path.display()
        ));
    }
    for (got, want) in outputs.iter().zip(golden) {
        if got != want {
            return Err(format!(
                "output changed from golden - prompt/engine edits must ship a regenerated golden:\n  golden: {want}\n  got:    {got}"
            ));
        }
    }
    println!("polish bench: outputs match the committed golden");
    Ok(())
}

/// Compare p50 against the committed per-runner baseline; >20% regression fails
/// (ENGINEERING.md 6). Missing baseline warns and passes so a new runner mints one.
fn check_baseline(name: &str, p50: f64) -> Result<(), String> {
    let path = bench_path(&format!("baselines/polish-{name}.json"));
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(_) => {
            eprintln!(
                "polish bench: no baseline at {}; commit the printed JSON there to arm the regression gate",
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
    println!("polish bench: within 20% of the {baseline_p50:.1} ms baseline");
    Ok(())
}

/// Length-scaled edit-distance guard, mirroring `verbatim_core::polish_guard`
/// (the source of truth). Copied because a bench in the engine layer cannot
/// depend on core; the boundary tests live there.
fn within_guard(raw: &str, polished: &str) -> bool {
    const SLACK: usize = 8;
    const MAX_RATIO: f64 = 0.5;
    let raw_chars: Vec<char> = raw.chars().collect();
    let pol_chars: Vec<char> = polished.chars().collect();
    let longer = raw_chars.len().max(pol_chars.len());
    let budget = SLACK + (MAX_RATIO * longer as f64) as usize;
    levenshtein(&raw_chars, &pol_chars) <= budget
}

fn levenshtein(a: &[char], b: &[char]) -> usize {
    let (a, b) = if a.len() < b.len() { (b, a) } else { (a, b) };
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

/// Nearest-rank percentile over an ascending-sorted, non-empty sample set.
fn percentile(sorted_ms: &[f64], pct: f64) -> f64 {
    if sorted_ms.is_empty() {
        return 0.0;
    }
    let rank = ((pct / 100.0) * sorted_ms.len() as f64).ceil() as usize;
    sorted_ms[rank.clamp(1, sorted_ms.len()) - 1]
}

/// Minimal `"p50_ms": <number>` extraction; the bench takes no serde dep.
fn parse_p50(json: &str) -> Option<f64> {
    let start = json.find("\"p50_ms\"")? + "\"p50_ms\"".len();
    let rest = json[start..].trim_start().strip_prefix(':')?.trim_start();
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// Resolve a path under the workspace-level `benches/` directory.
fn bench_path(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("benches")
        .join(rel)
}
