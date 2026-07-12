//! First-run onboarding service (M2 Phase D, UX.md 6).
//!
//! The webview renders the six-screen flow; the load-bearing logic - triggering
//! permission requests and re-checking, recommending a model for the detected
//! hardware, running the download while streaming progress on the bus, and
//! persisting completion - lives here in Rust behind trait seams (no business
//! logic in TypeScript, ARCHITECTURE.md 1). The Tauri command layer in `gui`
//! is a thin wrapper over these methods; every one is unit-testable headless
//! over the platform/engine fakes.
//!
//! Deep-link re-entry (UX.md 6): a permission that later fails routes E1/E9
//! back to the exact step, never the start - `step_for_error` is that map.

use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use verbatim_core::error::ErrorId;
use verbatim_core::event::{Event, EventBus};
use verbatim_engines::model::{
    self, DownloadError, HardwareProfile, ModelDownloader, ModelKind, ModelSpec,
};
use verbatim_platform::{Capability, PermissionProbe, PermissionRequest, PermissionState};

/// The onboarding screens, in order (UX.md 6). All are skippable-but-discouraged
/// except the permission steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OnboardingStep {
    Welcome,
    Microphone,
    Typing,
    ModelDownload,
    TryIt,
    Polish,
}

/// The screen a later permission failure deep-links back to (UX.md 6 re-entry):
/// E1 (mic) -> Microphone, E9 (Linux typing) -> Typing. Other errors are not
/// onboarding re-entries.
pub fn step_for_error(id: ErrorId) -> Option<OnboardingStep> {
    match id {
        ErrorId::E1 => Some(OnboardingStep::Microphone),
        ErrorId::E9 => Some(OnboardingStep::Typing),
        _ => None,
    }
}

/// Parse the webview's capability string into the platform enum. Unknown
/// strings return `None` so a command rejects them rather than guessing.
pub fn parse_capability(name: &str) -> Option<Capability> {
    match name {
        "microphone" => Some(Capability::Microphone),
        "textInjection" => Some(Capability::TextInjection),
        "inputMonitoring" => Some(Capability::InputMonitoring),
        _ => None,
    }
}

/// Tauri window label for the onboarding webview.
pub const WINDOW_LABEL: &str = "onboarding";

/// Build and show the onboarding window - a normal focusable window (unlike the
/// non-activating overlay), loading the onboarding webview surface (UX.md 6).
pub fn create_window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    WebviewWindowBuilder::new(app, WINDOW_LABEL, WebviewUrl::App("onboarding.html".into()))
        .title("Welcome to Verbatim")
        .inner_size(680.0, 720.0)
        .resizable(false)
        .center()
        .build()
}

/// Detect the hardware feeding the model recommendation (UX.md 6 step 4).
// TODO(m2): replace the conservative default with real RAM/GPU probing; a
// safe mid-tier default keeps onboarding functional until then.
pub fn detect_hardware() -> HardwareProfile {
    HardwareProfile {
        total_ram_gib: 8,
        has_gpu: false,
    }
}

/// Serde view of a catalog model for the webview (size shown per UX.md 6 step 4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub kind: ModelKindDto,
    pub size_bytes: u64,
}

/// Serde mirror of `ModelKind` (engines stays serde-free).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelKindDto {
    Transcription,
    Polish,
}

impl From<&ModelSpec> for ModelInfo {
    fn from(spec: &ModelSpec) -> Self {
        Self {
            id: spec.id.to_owned(),
            name: spec.name.to_owned(),
            kind: match spec.kind {
                ModelKind::Transcription => ModelKindDto::Transcription,
                ModelKind::Polish => ModelKindDto::Polish,
            },
            size_bytes: spec.size_bytes,
        }
    }
}

/// The dependencies onboarding drives - all trait seams so the flow runs over
/// the deterministic fakes in tests and CI.
#[derive(Clone)]
pub struct Onboarding {
    probe: Arc<dyn PermissionProbe>,
    requester: Arc<dyn PermissionRequest>,
    downloader: Arc<dyn ModelDownloader>,
    hardware: HardwareProfile,
    events: Arc<EventBus>,
}

impl Onboarding {
    pub fn new(
        probe: Arc<dyn PermissionProbe>,
        requester: Arc<dyn PermissionRequest>,
        downloader: Arc<dyn ModelDownloader>,
        hardware: HardwareProfile,
        events: Arc<EventBus>,
    ) -> Self {
        Self {
            probe,
            requester,
            downloader,
            hardware,
            events,
        }
    }

    /// Current permission state without prompting (initial render, re-check).
    pub fn permission(&self, capability: Capability) -> PermissionState {
        self.probe.probe(capability)
    }

    /// Trigger the OS permission request on user click, then re-check and return
    /// the resulting state. A capability the platform cannot request (Windows
    /// typing) is reported `NotNeeded` rather than surfaced as a failure.
    pub fn request_permission(&self, capability: Capability) -> PermissionState {
        match self.requester.request(capability) {
            Ok(()) => self.probe.probe(capability),
            Err(verbatim_platform::PermissionRequestError::Unsupported(_)) => {
                PermissionState::NotNeeded
            }
            Err(err) => {
                tracing::warn!(?err, ?capability, "requesting permission failed");
                self.probe.probe(capability)
            }
        }
    }

    /// Open the OS settings pane for a capability (deep link for the re-check
    /// loop and E1/E9 re-entry). Best-effort: a failure to launch settings is
    /// not fatal to onboarding.
    pub fn open_settings(&self, capability: Capability) {
        if let Err(err) = self.requester.open_settings(capability) {
            tracing::warn!(?err, ?capability, "opening permission settings failed");
        }
    }

    /// The recommended transcription model for the detected hardware (UX.md 6
    /// step 4: one model preselected, size shown).
    pub fn recommended_model(&self) -> ModelInfo {
        ModelInfo::from(model::recommend_transcription(self.hardware))
    }

    /// Every catalog model, for the "choose a different model" affordance.
    pub fn catalog(&self) -> Vec<ModelInfo> {
        model::CATALOG.iter().map(ModelInfo::from).collect()
    }

    /// Download `model_id`, streaming byte progress onto the core bus so the
    /// onboarding progress bar (and any other surface) updates 1:1. Returns the
    /// resolved on-disk path once fully written and hash-verified by the
    /// downloader. E8 (interrupted) surfaces as `DownloadError` for the caller
    /// to offer a resumable retry.
    pub fn download_model(&self, model_id: &str) -> Result<String, DownloadError> {
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
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    use verbatim_engines::fake::FakeModelDownloader;
    use verbatim_platform::fake::{FakePermissionProbe, FakePermissionRequester};

    fn hardware(ram: u32) -> HardwareProfile {
        HardwareProfile {
            total_ram_gib: ram,
            has_gpu: false,
        }
    }

    fn onboarding_with(
        probe: Arc<FakePermissionProbe>,
        requester: Arc<dyn PermissionRequest>,
        downloader: Arc<dyn ModelDownloader>,
        ram: u32,
    ) -> Onboarding {
        Onboarding::new(
            probe,
            requester,
            downloader,
            hardware(ram),
            Arc::new(EventBus::default()),
        )
    }

    /// The criterion-1 scripted walkthrough: an undetermined mic is prompted,
    /// re-checks Granted, a model is recommended and downloaded to completion -
    /// the whole install -> first-dictation path with no OS UI.
    #[test]
    fn scripted_walkthrough_reaches_a_downloaded_model() {
        let probe = Arc::new(FakePermissionProbe::default());
        probe.set(Capability::Microphone, PermissionState::Undetermined);
        let requester = Arc::new(FakePermissionRequester::new(Arc::clone(&probe)));
        let onboarding = onboarding_with(
            Arc::clone(&probe),
            Arc::clone(&requester) as Arc<dyn PermissionRequest>,
            Arc::new(FakeModelDownloader::default()),
            8,
        );

        assert_eq!(
            onboarding.permission(Capability::Microphone),
            PermissionState::Undetermined
        );
        assert_eq!(
            onboarding.request_permission(Capability::Microphone),
            PermissionState::Granted,
            "granting the prompt must flip the re-checked state"
        );

        let recommended = onboarding.recommended_model();
        assert_eq!(recommended.id, "whisper-small.en");

        let path = onboarding
            .download_model(&recommended.id)
            .expect("download");
        assert!(path.ends_with("whisper-small.en.bin"));
    }

    /// Download streams progress onto the bus 1:1 (UX.md 6 step 4 progress bar).
    #[test]
    fn download_publishes_monotonic_progress_on_the_bus() {
        let probe = Arc::new(FakePermissionProbe::default());
        let requester = Arc::new(FakePermissionRequester::new(Arc::clone(&probe)));
        let events = Arc::new(EventBus::default());
        let mut rx = events.subscribe();
        let onboarding = Onboarding::new(
            probe,
            requester,
            Arc::new(FakeModelDownloader::default()),
            hardware(8),
            Arc::clone(&events),
        );

        onboarding
            .download_model("whisper-base.en")
            .expect("download");

        let mut last = 0;
        let mut saw_final = false;
        while let Ok(event) = rx.try_recv() {
            if let Event::DownloadProgress {
                received_bytes,
                total_bytes,
                ..
            } = event
            {
                assert!(received_bytes >= last, "progress must be monotonic");
                last = received_bytes;
                if total_bytes == Some(received_bytes) {
                    saw_final = true;
                }
            }
        }
        assert!(saw_final, "the final tick must reach 100%");
    }

    /// An interrupted download surfaces E8 for a resumable retry, not a panic.
    #[test]
    fn interrupted_download_surfaces_an_error() {
        let probe = Arc::new(FakePermissionProbe::default());
        let requester = Arc::new(FakePermissionRequester::new(Arc::clone(&probe)));
        let onboarding = onboarding_with(
            probe,
            requester,
            Arc::new(FakeModelDownloader::failing_after(1)),
            8,
        );
        assert!(onboarding.download_model("whisper-base.en").is_err());
    }

    /// Windows has no typing permission: a request for an unsupported capability
    /// reports `NotNeeded`, so onboarding advances instead of dead-ending.
    #[test]
    fn unsupported_permission_reports_not_needed() {
        let probe = Arc::new(FakePermissionProbe::default());
        let requester = Arc::new(
            FakePermissionRequester::new(Arc::clone(&probe))
                .unsupported(vec![Capability::TextInjection]),
        );
        let onboarding = onboarding_with(
            probe,
            requester,
            Arc::new(FakeModelDownloader::default()),
            8,
        );
        assert_eq!(
            onboarding.request_permission(Capability::TextInjection),
            PermissionState::NotNeeded
        );
    }

    #[test]
    fn error_re_entry_maps_to_the_exact_step() {
        assert_eq!(
            step_for_error(ErrorId::E1),
            Some(OnboardingStep::Microphone)
        );
        assert_eq!(step_for_error(ErrorId::E9), Some(OnboardingStep::Typing));
        assert_eq!(step_for_error(ErrorId::E4), None);
    }
}
