//! Deterministic personal-dictionary post-pass (ARCHITECTURE.md 4.3, UX.md 5.3).
//!
//! The dictionary reaches the LLM through the prompt (engine-side, `PolishProfile`),
//! but critical terms must never depend on the model alone: a user who defined
//! "PCM" wants that casing whether polish ran, fell back to raw, or is off
//! entirely. So the same terms are re-applied here as a deterministic post-pass
//! over the text about to be injected - a whole-word, case-insensitive rewrite to
//! the term's canonical casing.
//!
//! Matching is ASCII-case-insensitive (dictionary terms are technical tokens like
//! `gRPC`/`PCM`) and whole-word (bounded by non-alphanumeric chars), so "pcm" ->
//! "PCM" but "pcmcia" is left alone.

/// Rewrite every whole-word, case-insensitive occurrence of each dictionary term
/// in `text` to the term's canonical casing. Terms are applied in order; an empty
/// term is skipped.
pub fn apply_dictionary(text: &str, terms: &[String]) -> String {
    let mut out = text.to_owned();
    for term in terms {
        if !term.is_empty() {
            out = replace_whole_word_ci(&out, term);
        }
    }
    out
}

/// Replace whole-word, ASCII-case-insensitive matches of `term` in `haystack`
/// with `term` verbatim (canonical casing).
///
/// `to_ascii_lowercase` preserves byte length (A-Z map to a-z, same width; all
/// other bytes untouched), so offsets in the lowercased copy align with
/// `haystack` and Unicode content survives.
fn replace_whole_word_ci(haystack: &str, term: &str) -> String {
    let term_lower = term.to_ascii_lowercase();
    let hay_lower = haystack.to_ascii_lowercase();
    let mut out = String::with_capacity(haystack.len());
    let mut cursor = 0;

    while let Some(rel) = hay_lower[cursor..].find(&term_lower) {
        let start = cursor + rel;
        let end = start + term_lower.len();
        let before_ok = haystack[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric());
        let after_ok = haystack[end..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric());

        if before_ok && after_ok {
            out.push_str(&haystack[cursor..start]);
            out.push_str(term);
            cursor = end;
        } else {
            // Not a whole word: keep one char and re-scan past it so overlapping
            // candidates still get a chance and the loop always advances.
            let next = start + haystack[start..].chars().next().map_or(1, char::len_utf8);
            out.push_str(&haystack[cursor..next]);
            cursor = next;
        }
    }
    out.push_str(&haystack[cursor..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(t: &[&str]) -> Vec<String> {
        t.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn rewrites_casing_of_whole_word_match() {
        assert_eq!(
            apply_dictionary("i sent the pcm buffer", &terms(&["PCM"])),
            "i sent the PCM buffer"
        );
    }

    #[test]
    fn leaves_substrings_inside_larger_words_alone() {
        // "pcm" inside "pcmcia" is not a whole word.
        assert_eq!(
            apply_dictionary("a pcmcia slot", &terms(&["PCM"])),
            "a pcmcia slot"
        );
    }

    #[test]
    fn matches_regardless_of_input_casing() {
        assert_eq!(
            apply_dictionary("GRPC and GrPc and grpc", &terms(&["gRPC"])),
            "gRPC and gRPC and gRPC"
        );
    }

    #[test]
    fn already_correct_casing_is_unchanged() {
        assert_eq!(apply_dictionary("PCM ready", &terms(&["PCM"])), "PCM ready");
    }

    #[test]
    fn applies_the_same_map_to_raw_and_polished_forms() {
        // Post-pass must not depend on whether polish ran; same fn, same result.
        let dict = terms(&["PCM", "gRPC"]);
        assert_eq!(
            apply_dictionary("um the pcm over grpc", &dict),
            "um the PCM over gRPC"
        );
        assert_eq!(
            apply_dictionary("The PCM streams over gRPC.", &dict),
            "The PCM streams over gRPC."
        );
    }

    #[test]
    fn empty_terms_and_empty_text_are_noops() {
        assert_eq!(apply_dictionary("pcm", &terms(&[""])), "pcm");
        assert_eq!(apply_dictionary("", &terms(&["PCM"])), "");
        assert_eq!(apply_dictionary("pcm", &[]), "pcm");
    }

    #[test]
    fn word_boundaries_respect_punctuation_not_just_spaces() {
        assert_eq!(
            apply_dictionary("(pcm) and pcm.", &terms(&["PCM"])),
            "(PCM) and PCM."
        );
    }

    #[test]
    fn unicode_content_is_preserved_around_matches() {
        assert_eq!(
            apply_dictionary("café pcm café", &terms(&["PCM"])),
            "café PCM café"
        );
    }

    #[test]
    fn multi_word_term_matches_as_a_phrase() {
        assert_eq!(
            apply_dictionary("run github actions now", &terms(&["GitHub Actions"])),
            "run GitHub Actions now"
        );
    }
}
