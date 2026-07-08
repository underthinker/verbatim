//! Versioned polish prompt assets (ENGINEERING.md; M3 Phase E).
//!
//! Prompt content - the system template and the load-bearing few-shot examples
//! (spike 4) - lives in `assets/prompts/<profile>@<version>.txt`, not in code, so
//! a prompt change is a reviewable asset diff that the polish-quality benchmark
//! gates. Assets are compiled in with `include_str!`: they ship with the binary
//! and need no runtime path resolution, keeping the core/engine layers path-free.
//!
//! File format: everything before the first `Raw:` line is the system prompt;
//! after it, consecutive `Raw:` / `Polished:` line pairs are few-shot examples
//! (one utterance per line - dictation is single-utterance). The framing matches
//! `llama::build_prompt` so few-shot and live turns look identical to the model.

use crate::types::FewShotExample;

const RAW_TAG: &str = "Raw:";
const POLISHED_TAG: &str = "Polished:";

/// One compiled-in prompt asset. `version` mirrors the `@<version>` in the source
/// filename so [`load`] can pick the newest revision of a profile.
struct Asset {
    id: &'static str,
    version: u32,
    text: &'static str,
}

/// Every shipped prompt asset. Adding a profile or bumping a version is one line
/// here plus the file. `load` resolves the highest version per id.
const ASSETS: &[Asset] = &[Asset {
    id: "default",
    version: 1,
    text: include_str!("../../../assets/prompts/default@1.txt"),
}];

/// The parsed contents of a prompt asset: the system template plus few-shot
/// examples. The caller adds the per-dictation dictionary and raw transcript.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromptContent {
    pub system_prompt: String,
    pub few_shot: Vec<FewShotExample>,
}

/// Load the newest prompt asset for `profile_id`, or `None` if no asset ships for
/// it (a user-defined profile id with no template - the caller polishes with an
/// empty system prompt rather than failing).
pub fn load(profile_id: &str) -> Option<PromptContent> {
    ASSETS
        .iter()
        .filter(|asset| asset.id == profile_id)
        .max_by_key(|asset| asset.version)
        .map(|asset| parse(asset.text))
}

/// Split an asset into system prompt (everything before the first `Raw:`) and the
/// `Raw:`/`Polished:` few-shot pairs. A `Raw:` without a following `Polished:` is
/// skipped rather than treated as a malformed error - the asset is trusted input.
fn parse(text: &str) -> PromptContent {
    let mut lines = text.lines().peekable();

    let mut system_lines = Vec::new();
    while let Some(line) = lines.peek() {
        if line.trim_start().starts_with(RAW_TAG) {
            break;
        }
        system_lines.push(*line);
        lines.next();
    }

    let mut few_shot = Vec::new();
    while let Some(line) = lines.next() {
        let Some(raw) = line.trim_start().strip_prefix(RAW_TAG) else {
            continue;
        };
        let Some(polished) = lines
            .peek()
            .and_then(|next| next.trim_start().strip_prefix(POLISHED_TAG))
        else {
            continue;
        };
        few_shot.push(FewShotExample {
            raw: raw.trim().to_owned(),
            polished: polished.trim().to_owned(),
        });
        lines.next();
    }

    PromptContent {
        system_prompt: system_lines.join("\n").trim().to_owned(),
        few_shot,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_loads_with_prompt_and_few_shot() {
        let content = load("default").expect("default asset ships");
        assert!(
            content.system_prompt.contains("meaning"),
            "system prompt should state the meaning-preserving rule"
        );
        assert!(
            !content.few_shot.is_empty(),
            "few-shot examples are load-bearing (spike 4)"
        );
        // Few-shot round-trips both sides cleanly (tags and whitespace stripped).
        let first = &content.few_shot[0];
        assert!(first.raw.starts_with("um so hey"));
        assert!(first.polished.starts_with("Hey, can you"));
    }

    #[test]
    fn unknown_profile_is_none() {
        assert_eq!(load("no-such-profile"), None);
    }

    #[test]
    fn parse_splits_system_from_examples() {
        let content = parse("Be terse.\nStay factual.\n\nRaw: um hi\nPolished: Hi.\n");
        assert_eq!(content.system_prompt, "Be terse.\nStay factual.");
        assert_eq!(
            content.few_shot,
            vec![FewShotExample {
                raw: "um hi".to_owned(),
                polished: "Hi.".to_owned(),
            }]
        );
    }

    #[test]
    fn parse_skips_a_raw_without_polished() {
        let content = parse("Sys.\n\nRaw: dangling\nRaw: um hi\nPolished: Hi.\n");
        assert_eq!(
            content.few_shot,
            vec![FewShotExample {
                raw: "um hi".to_owned(),
                polished: "Hi.".to_owned(),
            }]
        );
    }
}
