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
    /// Lowercase-hex sha256 the real downloader verifies before a model is used.
    // TODO(phase-e): pin the real release digests before enabling the network
    // downloader; empty means "not yet pinned" and the fake never reads it.
    pub sha256: &'static str,
    /// Installed RAM (GiB) at or above which this model is a smooth realtime
    /// choice - the input to the onboarding hardware recommendation.
    pub min_ram_gib: u32,
}

/// The built-in catalog (ARCHITECTURE.md 4.2/4.3). Ordered small -> large
/// within each kind so recommendation can scan by fit.
pub const CATALOG: &[ModelSpec] = &[
    ModelSpec {
        id: "whisper-base.en",
        name: "Base (English)",
        kind: ModelKind::Transcription,
        size_bytes: 148_000_000,
        sha256: "",
        min_ram_gib: 4,
    },
    ModelSpec {
        id: "whisper-small.en",
        name: "Small (English)",
        kind: ModelKind::Transcription,
        size_bytes: 488_000_000,
        sha256: "",
        min_ram_gib: 8,
    },
    ModelSpec {
        id: "whisper-medium.en",
        name: "Medium (English)",
        kind: ModelKind::Transcription,
        size_bytes: 1_530_000_000,
        sha256: "",
        min_ram_gib: 16,
    },
    ModelSpec {
        id: "polish-qwen2.5-0.5b",
        name: "Polish (Qwen2.5 0.5B)",
        kind: ModelKind::Polish,
        size_bytes: 352_000_000,
        sha256: "",
        min_ram_gib: 8,
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
    asr.filter(|spec| spec.min_ram_gib <= hardware.total_ram_gib)
        .max_by_key(|spec| spec.min_ram_gib)
        .unwrap_or(smallest)
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
