use crate::{EngineId, PolishEngine, TranscriptionEngine};

type TranscriptionFactory = Box<dyn Fn() -> Box<dyn TranscriptionEngine> + Send + Sync>;
type PolishFactory = Box<dyn Fn() -> Box<dyn PolishEngine> + Send + Sync>;

/// Registry of available engine implementations.
///
/// Adding an engine touches only the engine layer plus one registry entry
/// (ARCHITECTURE.md 1). Real engines register here behind their feature flags
/// during M1 wire-up.
#[derive(Default)]
pub struct EngineRegistry {
    transcription: Vec<(EngineId, TranscriptionFactory)>,
    polish: Vec<(EngineId, PolishFactory)>,
}

impl EngineRegistry {
    pub fn register_transcription(
        &mut self,
        id: EngineId,
        factory: impl Fn() -> Box<dyn TranscriptionEngine> + Send + Sync + 'static,
    ) {
        self.transcription.push((id, Box::new(factory)));
    }

    pub fn register_polish(
        &mut self,
        id: EngineId,
        factory: impl Fn() -> Box<dyn PolishEngine> + Send + Sync + 'static,
    ) {
        self.polish.push((id, Box::new(factory)));
    }

    pub fn transcription_ids(&self) -> Vec<EngineId> {
        self.transcription.iter().map(|(id, _)| *id).collect()
    }

    pub fn polish_ids(&self) -> Vec<EngineId> {
        self.polish.iter().map(|(id, _)| *id).collect()
    }

    pub fn create_transcription(&self, id: EngineId) -> Option<Box<dyn TranscriptionEngine>> {
        self.transcription
            .iter()
            .find(|(candidate, _)| *candidate == id)
            .map(|(_, factory)| factory())
    }

    pub fn create_polish(&self, id: EngineId) -> Option<Box<dyn PolishEngine>> {
        self.polish
            .iter()
            .find(|(candidate, _)| *candidate == id)
            .map(|(_, factory)| factory())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeTranscriptionEngine;

    #[test]
    fn registry_creates_registered_engines() {
        let mut registry = EngineRegistry::default();
        registry.register_transcription(EngineId::Fake, || {
            Box::new(FakeTranscriptionEngine::speaking("hi"))
        });

        assert_eq!(registry.transcription_ids(), vec![EngineId::Fake]);
        assert!(registry.create_transcription(EngineId::Fake).is_some());
        assert!(
            registry
                .create_transcription(EngineId::WhisperCpp)
                .is_none()
        );
        assert!(registry.create_polish(EngineId::LlamaCpp).is_none());
    }
}
