//! Model manager domain (M2 Phase E-2, UX.md 7): list catalog models with
//! their installed state and on-disk size, report total disk usage, delete an
//! installed model, and set the default transcription/polish model (persisted
//! to the user config from E-1). Download drives the shared `ModelDownloader`
//! seam and streams byte progress on the core bus (same contract onboarding
//! uses), so tests run over the deterministic fake.
//!
//! Model files live under the *data* dir (ENGINEERING.md 5.2), one file per
//! catalog id. Setting a default writes `config.toml`; deleting the current
//! default clears it so config never points at a missing file.
//!
//! Not here yet: the real hash-verified *network* downloader and E8 byte-range
//! resume. Those need pinned release URLs + sha256 digests (the catalog TODO)
//! and the first sanctioned HTTP dependency; they land in a follow-up once the
//! release artifacts exist. Everything below works over the existing seam.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;

use verbatim_core::event::{Event, EventBus};
use verbatim_engines::model::{self, DownloadError, ModelDownloader, ModelKind, ModelSpec};

use crate::config;
use crate::settings::{self, Config};

/// A catalog model annotated with its local state, for the Model manager UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedModel {
    pub id: String,
    pub name: String,
    pub kind: ModelKindDto,
    /// Catalog (download) size in bytes.
    pub size_bytes: u64,
    /// Whether a file for this model exists on disk.
    pub installed: bool,
    /// Actual on-disk size when installed (may differ from the catalog size for
    /// a partial/interrupted file).
    pub on_disk_bytes: Option<u64>,
    /// Whether this model is the configured default for its kind.
    pub is_default: bool,
    /// SPDX license id of the weights (PRD 136 attribution surface).
    pub license: String,
    /// Required credit line, rendered in the model manager + About.
    pub attribution: String,
    /// Whether this is the recommended transcription model for the detected
    /// hardware - the model manager's "Recommended" badge (Phase C item 4).
    pub recommended: bool,
}

/// Serde mirror of `ModelKind` (engines stays serde-free).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelKindDto {
    Transcription,
    Polish,
}

impl From<ModelKind> for ModelKindDto {
    fn from(kind: ModelKind) -> Self {
        match kind {
            ModelKind::Transcription => ModelKindDto::Transcription,
            ModelKind::Polish => ModelKindDto::Polish,
        }
    }
}

/// The model manager service. Holds explicit dirs so tests inject temp
/// locations without touching process-global env vars.
#[derive(Clone)]
pub struct ModelManager {
    downloader: Arc<dyn ModelDownloader>,
    events: Arc<EventBus>,
    models_dir: PathBuf,
    config_dir: PathBuf,
}

impl ModelManager {
    /// Wire the manager to the real per-user data/config dirs.
    pub fn new(downloader: Arc<dyn ModelDownloader>, events: Arc<EventBus>) -> Self {
        Self {
            downloader,
            events,
            models_dir: models_dir(),
            config_dir: settings::config_dir(),
        }
    }

    /// The catalog with each model's local state resolved against disk + config.
    pub fn list(&self) -> Vec<ManagedModel> {
        let config = settings::load_from(&self.config_dir);
        model::CATALOG
            .iter()
            .map(|spec| self.managed(spec, &config))
            .collect()
    }

    /// Total bytes used by installed model files.
    pub fn disk_usage(&self) -> u64 {
        model::CATALOG
            .iter()
            .filter_map(|spec| on_disk_size(&self.model_path(spec.id)))
            .sum()
    }

    /// Delete an installed model file. Clearing the current default for its kind
    /// keeps config from pointing at a missing file. Deleting an absent file is
    /// a no-op (idempotent), not an error.
    pub fn delete(&self, model_id: &str) -> Result<(), ModelError> {
        let spec = model::spec(model_id).ok_or(ModelError::Unknown)?;
        let path = self.model_path(model_id);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|err| ModelError::Io(err.to_string()))?;
        }
        // Drop the default if it was this model, so nothing references it.
        let mut config = settings::load_from(&self.config_dir);
        if clear_default_if(&mut config, spec.kind, model_id) {
            settings::save_to(&self.config_dir, &config)
                .map_err(|err| ModelError::Io(err.to_string()))?;
        }
        Ok(())
    }

    /// Set a model as the default for its kind (transcription or polish),
    /// persisted to config. The model must be installed - defaulting to a
    /// missing file would break the pipeline.
    pub fn set_default(&self, model_id: &str) -> Result<(), ModelError> {
        let spec = model::spec(model_id).ok_or(ModelError::Unknown)?;
        if !self.model_path(model_id).exists() {
            return Err(ModelError::NotInstalled);
        }
        let mut config = settings::load_from(&self.config_dir);
        match spec.kind {
            ModelKind::Transcription => config.transcription_model = Some(model_id.to_owned()),
            ModelKind::Polish => config.polish_model = Some(model_id.to_owned()),
        }
        settings::save_to(&self.config_dir, &config).map_err(|err| ModelError::Io(err.to_string()))
    }

    /// Download `model_id`, streaming `DownloadProgress` on the bus, returning
    /// the resolved on-disk path. An interrupted download surfaces as an error
    /// for the UI to offer a resumable retry (E8).
    pub fn download(&self, model_id: &str) -> Result<String, DownloadError> {
        let spec = model::spec(model_id)
            .ok_or_else(|| DownloadError::UnknownModel(model_id.to_owned()))?;
        let events = Arc::clone(&self.events);
        let id = model_id.to_owned();
        let progress = move |received: u64, total: u64| {
            events.publish(Event::DownloadProgress {
                model_id: id.clone(),
                received_bytes: received,
                total_bytes: Some(total),
            });
        };
        let handle = self.downloader.download(spec, &progress)?;
        Ok(handle.path.to_string_lossy().into_owned())
    }

    fn model_path(&self, model_id: &str) -> PathBuf {
        model_file(&self.models_dir, model_id)
    }

    fn managed(&self, spec: &ModelSpec, config: &Config) -> ManagedModel {
        let hardware = crate::onboarding::detect_hardware();
        let on_disk_bytes = on_disk_size(&self.model_path(spec.id));
        let default_id = match spec.kind {
            ModelKind::Transcription => config.transcription_model.as_deref(),
            ModelKind::Polish => config.polish_model.as_deref(),
        };
        ManagedModel {
            id: spec.id.to_owned(),
            name: spec.name.to_owned(),
            kind: spec.kind.into(),
            size_bytes: spec.size_bytes,
            installed: on_disk_bytes.is_some(),
            on_disk_bytes,
            is_default: default_id == Some(spec.id),
            license: spec.license.to_owned(),
            attribution: spec.attribution.to_owned(),
            recommended: model::fitness(spec, hardware) == model::Fitness::Recommended,
        }
    }
}

/// Why a model-manager operation failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ModelError {
    #[error("unknown model")]
    Unknown,
    #[error("model is not installed")]
    NotInstalled,
    #[error("filesystem error: {0}")]
    Io(String),
}

/// The model store: `<data dir>/models`, overridable via `$VERBATIM_DATA_DIR`.
pub fn models_dir() -> PathBuf {
    config::data_dir().join("models")
}

/// The on-disk file for a catalog model id, present or not.
fn model_file(models_dir: &Path, model_id: &str) -> PathBuf {
    models_dir.join(format!("{model_id}.bin"))
}

/// Resolve a configured catalog id to its installed model file, or `None` when
/// the file is not on disk. This is how engine construction (daemon/GUI boot)
/// finds the model the settings/onboarding flow chose.
pub fn installed_model_path(model_id: &str) -> Option<PathBuf> {
    let path = model_file(&models_dir(), model_id);
    path.exists().then_some(path)
}

/// File size in bytes, or `None` if it does not exist / cannot be read.
fn on_disk_size(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok().map(|m| m.len())
}

/// Clear the default for `kind` if it currently names `model_id`. Returns
/// whether anything changed (so the caller only rewrites config when needed).
fn clear_default_if(config: &mut Config, kind: ModelKind, model_id: &str) -> bool {
    let slot = match kind {
        ModelKind::Transcription => &mut config.transcription_model,
        ModelKind::Polish => &mut config.polish_model,
    };
    if slot.as_deref() == Some(model_id) {
        *slot = None;
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    use verbatim_engines::fake::FakeModelDownloader;

    /// A manager pointed at fresh temp dirs, isolated from the real user data.
    fn manager(tag: &str) -> (ModelManager, PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!("verbatim-mm-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let models_dir = base.join("models");
        let config_dir = base.join("config");
        std::fs::create_dir_all(&models_dir).expect("mkdir models");
        std::fs::create_dir_all(&config_dir).expect("mkdir config");
        let mgr = ModelManager {
            downloader: Arc::new(FakeModelDownloader::default()),
            events: Arc::new(EventBus::default()),
            models_dir: models_dir.clone(),
            config_dir: config_dir.clone(),
        };
        (mgr, models_dir, config_dir)
    }

    fn install(models_dir: &Path, id: &str, bytes: &[u8]) {
        std::fs::write(models_dir.join(format!("{id}.bin")), bytes).expect("write model");
    }

    #[test]
    fn list_reflects_installed_state_and_size() {
        let (mgr, models_dir, _cfg) = manager("list");
        install(&models_dir, "whisper-base.en", &[0u8; 10]);
        let list = mgr.list();
        let base = list
            .iter()
            .find(|m| m.id == "whisper-base.en")
            .expect("base present");
        assert!(base.installed);
        assert_eq!(base.on_disk_bytes, Some(10));
        let small = list
            .iter()
            .find(|m| m.id == "whisper-small.en")
            .expect("small present");
        assert!(!small.installed);
        assert_eq!(small.on_disk_bytes, None);
    }

    #[test]
    fn disk_usage_sums_installed_files() {
        let (mgr, models_dir, _cfg) = manager("disk");
        install(&models_dir, "whisper-base.en", &[0u8; 100]);
        install(&models_dir, "polish-qwen2.5-0.5b", &[0u8; 50]);
        assert_eq!(mgr.disk_usage(), 150);
    }

    #[test]
    fn set_default_requires_install_then_persists() {
        let (mgr, models_dir, config_dir) = manager("default");
        assert_eq!(
            mgr.set_default("whisper-small.en").unwrap_err(),
            ModelError::NotInstalled
        );
        install(&models_dir, "whisper-small.en", &[0u8; 4]);
        mgr.set_default("whisper-small.en").expect("set default");
        let config = settings::load_from(&config_dir);
        assert_eq!(
            config.transcription_model.as_deref(),
            Some("whisper-small.en")
        );
    }

    #[test]
    fn deleting_the_default_clears_it() {
        let (mgr, models_dir, config_dir) = manager("del-default");
        install(&models_dir, "whisper-small.en", &[0u8; 4]);
        mgr.set_default("whisper-small.en").expect("set default");
        mgr.delete("whisper-small.en").expect("delete");
        assert!(!models_dir.join("whisper-small.en.bin").exists());
        let config = settings::load_from(&config_dir);
        assert_eq!(config.transcription_model, None);
    }

    #[test]
    fn deleting_a_non_default_leaves_config_untouched() {
        let (mgr, models_dir, config_dir) = manager("del-other");
        install(&models_dir, "whisper-base.en", &[0u8; 4]);
        install(&models_dir, "whisper-small.en", &[0u8; 4]);
        mgr.set_default("whisper-small.en").expect("set default");
        mgr.delete("whisper-base.en").expect("delete base");
        let config = settings::load_from(&config_dir);
        assert_eq!(
            config.transcription_model.as_deref(),
            Some("whisper-small.en")
        );
    }

    #[test]
    fn unknown_model_is_rejected() {
        let (mgr, _m, _c) = manager("unknown");
        assert_eq!(mgr.delete("nope").unwrap_err(), ModelError::Unknown);
        assert_eq!(mgr.set_default("nope").unwrap_err(), ModelError::Unknown);
    }

    #[test]
    fn download_streams_progress_on_the_bus() {
        let (mgr, _m, _c) = manager("download");
        let mut rx = mgr.events.subscribe();
        mgr.download("whisper-base.en").expect("download");
        // At least one DownloadProgress for this model reached the bus.
        let mut saw = false;
        while let Ok(event) = rx.try_recv() {
            if let Event::DownloadProgress { model_id, .. } = event
                && model_id == "whisper-base.en"
            {
                saw = true;
            }
        }
        assert!(saw, "expected DownloadProgress on the bus");
    }
}
