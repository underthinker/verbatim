//! Caller-side similarity guard against polish rewording drift (spike 4,
//! ARCHITECTURE.md 4.3). The guard lives here, not in the engine: the engine
//! only generates and self-rejects on deadline; deciding whether the polish
//! strayed too far from the raw transcript is a pipeline concern.
//!
//! The metric is a length-scaled edit distance. Cleaning disfluent speech is
//! inherently edit-heavy - removing "um so", false starts, and "the the"
//! repeats can rewrite a large fraction of a short utterance - so the budget
//! is generous and grows with length. Polish whose edit distance from the raw
//! exceeds the budget is treated as rewording drift and dropped in favour of
//! raw (injected as `PolishRejection::SimilarityGuard`).

/// Absolute edit slack allowed regardless of length. Short utterances are
/// proportionally rewritten by legitimate cleanup ("i" -> "I.", "um yeah" ->
/// "Yes"), so a flat floor keeps the guard from firing on them.
const SLACK: usize = 8;

/// Fraction of the longer string's length added to the slack. Filler-heavy
/// dictation legitimately loses ~a third of its characters; the cap sits above
/// that so honest cleanup passes while a full paraphrase or hallucinated answer
/// (which substitutes rather than deletes) blows the budget.
//
// ponytail: constants calibrated against the spike-4 fixtures below, not a
// benchmark set. Phase E re-tunes MAX_RATIO/SLACK against the polish-quality
// benchmark; the fixture tests here are the interim calibration floor.
const MAX_RATIO: f64 = 0.5;

/// Whether `polished` is close enough to `raw` to inject. `false` means the
/// polish drifted too far and the pipeline should fall back to raw.
pub fn within_guard(raw: &str, polished: &str) -> bool {
    let raw_chars: Vec<char> = raw.chars().collect();
    let pol_chars: Vec<char> = polished.chars().collect();
    let longer = raw_chars.len().max(pol_chars.len());
    let budget = SLACK + (MAX_RATIO * longer as f64) as usize;
    levenshtein(&raw_chars, &pol_chars) <= budget
}

/// Levenshtein edit distance over char slices (two-row DP, O(n*m) time,
/// O(min) space). Inputs are single utterances, so allocation is trivial.
fn levenshtein(a: &[char], b: &[char]) -> usize {
    // Keep the inner (column) dimension the shorter of the two.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levenshtein_basics() {
        assert_eq!(levenshtein(&chars(""), &chars("")), 0);
        assert_eq!(levenshtein(&chars("kitten"), &chars("sitting")), 3);
        assert_eq!(levenshtein(&chars("abc"), &chars("")), 3);
        assert_eq!(levenshtein(&chars("PCM"), &chars("pcm")), 3);
    }

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    // Legitimate polish from spike 4 must pass: honest cleanup of disfluent
    // speech is edit-heavy but never rewording drift.
    #[test]
    fn accepts_legit_spike_polish() {
        let raw = "um so hey can you uh can you send over the the latest draft of the architecture doc when you get a chance i wanna like review the section on on text injection before our meeting tomorrow morning at ten";
        let polished = "Hey, can you send over the latest draft of the architecture doc when you get a chance? I want to review the section on text injection before our meeting tomorrow morning at ten.";
        assert!(within_guard(raw, polished));

        let raw2 = "hey um quick question uh what time does the the standup start tomorrow and and should i prepare anything";
        let polished2 = "Hey, quick question. What time does the standup start tomorrow, and should I prepare anything?";
        assert!(within_guard(raw2, polished2));
    }

    // Short utterances: proportionally large but honest edits still pass.
    #[test]
    fn accepts_short_cleanup() {
        assert!(within_guard("um yeah ok", "Yeah, okay."));
        assert!(within_guard("i", "I."));
    }

    // Rewording drift: a hallucinated answer or full paraphrase that
    // substitutes meaning rather than deleting filler must be rejected.
    #[test]
    fn rejects_rewording_drift() {
        // The engine answered the question instead of cleaning it.
        let raw = "what time does the standup start tomorrow";
        let drifted = "The standup starts at nine in the morning, and you should bring your notes.";
        assert!(!within_guard(raw, drifted));

        // Wholesale paraphrase of a technical instruction.
        let raw2 = "send over the latest draft of the architecture doc";
        let drifted2 =
            "I have forwarded the newest version of the system design paper to your inbox already.";
        assert!(!within_guard(raw2, drifted2));
    }

    // Boundary: pin the just-under/just-over transition so the curve can't
    // silently move. Raw of length 20 -> budget = 8 + 0.5*len(longer).
    #[test]
    fn boundary_is_pinned() {
        let raw = "aaaaaaaaaaaaaaaaaaaa"; // 20 chars
        // Same length: budget = 8 + 10 = 18. 18 substitutions accept.
        let just_under: String = "b".repeat(18) + "aa";
        assert_eq!(levenshtein(&chars(raw), &chars(&just_under)), 18);
        assert!(within_guard(raw, &just_under));
        // 19 substitutions exceed the budget of 18.
        let just_over: String = "b".repeat(19) + "a";
        assert_eq!(levenshtein(&chars(raw), &chars(&just_over)), 19);
        assert!(!within_guard(raw, &just_over));
    }
}
