//! Model catalog + download seam (M2 Phase D onboarding, UX.md 6 step 4).
//!
//! The catalog is static, network-free metadata: id, human name, byte size, and
//! the pinned sha256 the downloader must verify. `ModelDownloader` is the seam
//! the app drives to fetch a model; the real hash-verified *network* downloader
//! is the single sanctioned place for network code (security posture) and lands
//! with the Phase E model manager. Onboarding drives the deterministic fake
//! (`fake::FakeModelDownloader`) so the "< 5 min to first dictation" flow is
//! testable without touching the network.

use thiserror::Error;

use crate::ModelHandle;

/// Which pipeline stage a model serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelKind {
    Transcription,
    Polish,
}

/// Static, network-free metadata for a downloadable model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelSpec {
    /// Stable id, also the key carried on `Event::DownloadProgress`.
    pub id: &'static str,
    /// Human-facing name shown in onboarding and the model manager.
    pub name: &'static str,
    pub kind: ModelKind,
    pub size_bytes: u64,
    /// HTTPS source for the immutable model artifact.
    pub url: &'static str,
    /// Lowercase-hex sha256 the real downloader verifies before a model is used.
    pub sha256: &'static str,
    /// Installed RAM (GiB) at or above which this model is a smooth realtime
    /// choice - the input to the onboarding hardware recommendation.
    pub min_ram_gib: u32,
    /// SPDX license id of the model weights, rendered in the model manager +
    /// About surface (PRD 136).
    pub license: &'static str,
    /// The credit line the license requires. Parakeet's CC-BY-4.0 obliges
    /// attribution to NVIDIA; every catalog entry carries one so the surfaces
    /// never render a blank.
    pub attribution: &'static str,
}

/// The built-in catalog (ARCHITECTURE.md 4.2/4.3). Ordered small -> large
/// within each kind so recommendation can scan by fit.
pub const CATALOG: &[ModelSpec] = &[
    ModelSpec {
        id: "whisper-base.en",
        name: "Base (English)",
        kind: ModelKind::Transcription,
        size_bytes: 147_964_211,
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-base.en.bin",
        sha256: "a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002",
        min_ram_gib: 4,
        license: "MIT",
        attribution: "OpenAI Whisper, via whisper.cpp (ggml)",
    },
    ModelSpec {
        id: "whisper-small.en",
        name: "Small (English)",
        kind: ModelKind::Transcription,
        size_bytes: 487_614_201,
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-small.en.bin",
        sha256: "c6138d6d58ecc8322097e0f987c32f1be8bb0a18532a3f88f734d1bbf9c41e5d",
        min_ram_gib: 8,
        license: "MIT",
        attribution: "OpenAI Whisper, via whisper.cpp (ggml)",
    },
    ModelSpec {
        id: "whisper-medium.en",
        name: "Medium (English)",
        kind: ModelKind::Transcription,
        size_bytes: 1_533_774_781,
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-medium.en.bin",
        sha256: "cc37e93478338ec7700281a7ac30a10128929eb8f427dda2e865faa8f6da4356",
        min_ram_gib: 16,
        license: "MIT",
        attribution: "OpenAI Whisper, via whisper.cpp (ggml)",
    },
    // Parakeet (Phase C): the second transcription engine's model. CC-BY-4.0
    // obliges the NVIDIA credit rendered in the model manager + About (PRD 136).
    ModelSpec {
        id: "parakeet-tdt-0.6b",
        name: "Parakeet (English)",
        kind: ModelKind::Transcription,
        size_bytes: 660_000_000,
        url: "",
        sha256: "",
        min_ram_gib: 4,
        license: "CC-BY-4.0",
        attribution: "NVIDIA Parakeet TDT 0.6B, ONNX export via sherpa-onnx (k2-fsa)",
    },
    ModelSpec {
        id: "polish-qwen2.5-0.5b",
        name: "Polish (Qwen2.5 0.5B)",
        kind: ModelKind::Polish,
        size_bytes: 352_000_000,
        url: "",
        sha256: "",
        min_ram_gib: 8,
        license: "Apache-2.0",
        attribution: "Qwen2.5 0.5B by Alibaba Cloud, via llama.cpp (GGUF)",
    },
];

/// Look a model up by its catalog id.
pub fn spec(id: &str) -> Option<&'static ModelSpec> {
    CATALOG.iter().find(|spec| spec.id == id)
}

/// Detected hardware feeding the onboarding recommendation (UX.md 6 step 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HardwareProfile {
    pub total_ram_gib: u32,
    pub has_gpu: bool,
}

/// The recommended transcription model for `hardware`: the largest model whose
/// `min_ram_gib` fits, falling back to the smallest in the catalog. Deterministic
/// and network-free so onboarding can preselect without probing anything remote.
pub fn recommend_transcription(hardware: HardwareProfile) -> &'static ModelSpec {
    let asr = CATALOG
        .iter()
        .filter(|spec| spec.kind == ModelKind::Transcription);
    // CATALOG has at least one transcription model; the fallback is the first.
    let smallest = asr.clone().next().unwrap_or(&CATALOG[0]);
    // Largest that fits by RAM, tie broken toward the *earlier* catalog entry.
    // The tie-break keeps the pick stable when two models share a RAM tier (e.g.
    // whisper-base and parakeet at 4 GiB): a new same-tier model must not
    // silently become the auto-recommendation before its engine is wired.
    asr.filter(|spec| spec.min_ram_gib <= hardware.total_ram_gib)
        .fold(None, |best: Option<&ModelSpec>, spec| match best {
            Some(b) if b.min_ram_gib >= spec.min_ram_gib => Some(b),
            _ => Some(spec),
        })
        .unwrap_or(smallest)
}

/// How a catalog model fits the detected hardware - the model manager's per-row
/// label (plan Phase C item 4). Fit is RAM-bound: `has_gpu` affects inference
/// *speed* (backend fallback at load, calibration ms/token), not whether a model
/// fits, so it does not move a model between tiers here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fitness {
    /// The single best pick for this hardware (the onboarding recommendation).
    Recommended,
    /// Runs comfortably, just not the top pick.
    Fits,
    /// Above the machine's RAM tier - runnable but may miss the latency budget.
    Heavy,
}

/// Classify `spec` against `hardware` for the model-manager label. Polish models
/// are never "Recommended" (that word is the transcription pick); they are Fits
/// or Heavy by RAM alone.
pub fn fitness(spec: &ModelSpec, hardware: HardwareProfile) -> Fitness {
    if spec.kind == ModelKind::Transcription && spec.id == recommend_transcription(hardware).id {
        Fitness::Recommended
    } else if spec.min_ram_gib <= hardware.total_ram_gib {
        Fitness::Fits
    } else {
        Fitness::Heavy
    }
}

#[derive(Debug, Error)]
pub enum DownloadError {
    #[error("unknown model {0}")]
    UnknownModel(String),
    #[error("download transport failed: {0}")]
    Transport(String),
    #[error("hash mismatch for {0}")]
    HashMismatch(String),
}

/// Progress callback: `(received_bytes, total_bytes)`, called as bytes arrive.
pub type ProgressSink = dyn Fn(u64, u64) + Send + Sync;

/// The seam the app drives to fetch a model. The real implementation (Phase E)
/// is the only sanctioned network code and hash-verifies against `spec.sha256`
/// before returning; onboarding uses the fake. Blocking - callers run it off
/// the UI thread. Resumable retry (E8) is the caller's concern.
pub trait ModelDownloader: Send + Sync {
    fn download(
        &self,
        spec: &ModelSpec,
        progress: &ProgressSink,
    ) -> Result<ModelHandle, DownloadError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommendation_picks_the_largest_that_fits() {
        assert_eq!(
            recommend_transcription(HardwareProfile {
                total_ram_gib: 8,
                has_gpu: false,
            })
            .id,
            "whisper-small.en"
        );
        assert_eq!(
            recommend_transcription(HardwareProfile {
                total_ram_gib: 64,
                has_gpu: true,
            })
            .id,
            "whisper-medium.en"
        );
    }

    #[test]
    fn every_catalog_model_carries_license_and_attribution() {
        // The model manager + About surface must never render a blank credit
        // (PRD 136 - Parakeet's CC-BY-4.0 in particular obliges attribution).
        for spec in CATALOG {
            assert!(!spec.license.is_empty(), "{} missing license", spec.id);
            assert!(
                !spec.attribution.is_empty(),
                "{} missing attribution",
                spec.id
            );
        }
        let parakeet = spec("parakeet-tdt-0.6b").expect("Parakeet is in the catalog");
        assert_eq!(parakeet.license, "CC-BY-4.0");
        assert!(parakeet.attribution.contains("NVIDIA"));
    }

    #[test]
    fn fitness_labels_track_the_recommendation() {
        let hw = HardwareProfile {
            total_ram_gib: 8,
            has_gpu: false,
        };
        let rec = recommend_transcription(hw);
        assert_eq!(fitness(rec, hw), Fitness::Recommended);
        // A model above the RAM tier is Heavy; one that fits but is not the pick
        // is Fits.
        assert_eq!(
            fitness(spec("whisper-medium.en").unwrap(), hw),
            Fitness::Heavy
        );
        assert_eq!(fitness(spec("whisper-base.en").unwrap(), hw), Fitness::Fits);
    }

    #[test]
    fn same_tier_tie_stays_on_the_earlier_model() {
        // Parakeet shares the 4 GiB tier with whisper-base; the recommendation
        // must not flip to it purely on the tie (its engine lands later).
        assert_eq!(
            recommend_transcription(HardwareProfile {
                total_ram_gib: 4,
                has_gpu: false,
            })
            .id,
            "whisper-base.en"
        );
    }

    #[test]
    fn recommendation_falls_back_to_smallest_on_tiny_hardware() {
        assert_eq!(
            recommend_transcription(HardwareProfile {
                total_ram_gib: 2,
                has_gpu: false,
            })
            .id,
            "whisper-base.en"
        );
    }

    #[test]
    fn catalog_ids_are_unique_and_resolvable() {
        for spec in CATALOG {
            assert_eq!(super::spec(spec.id), Some(spec));
        }
        assert_eq!(super::spec("nonexistent"), None);
    }
}
