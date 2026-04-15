use color_eyre::Result;
use scriptrs::{
    LongFormTranscriptionPipeline, ModelBundle as TranscriptionModelBundle,
    ModelManager as ScriptModelManager,
};
use speakrs::{
    ExecutionMode, ModelBundle as DiarizationModelBundle, ModelManager as SpeakerModelManager,
    OwnedDiarizationPipeline,
};
use std::path::Path;
use std::time::Instant;
use tracing::debug;

const DIARIZATION_EXECUTION_MODE: ExecutionMode = ExecutionMode::CoreMl;

pub fn ensure_transcription_models(scriptrs_cache_dir: &Path) -> Result<TranscriptionModelBundle> {
    let transcription_started = Instant::now();
    let transcription =
        ScriptModelManager::with_cache_dir(scriptrs_cache_dir.to_path_buf())?.ensure_long_form()?;
    debug!(
        "Ensured transcription models in {:.2}s",
        transcription_started.elapsed().as_secs_f64()
    );
    Ok(transcription)
}

pub fn ensure_diarization_models(speakrs_cache_dir: &Path) -> Result<DiarizationModelBundle> {
    let diarization_started = Instant::now();
    let diarization_models_dir =
        SpeakerModelManager::with_cache_dir(speakrs_cache_dir.to_path_buf())?
            .ensure(DIARIZATION_EXECUTION_MODE)?;
    debug!(
        "Ensured diarization models in {:.2}s",
        diarization_started.elapsed().as_secs_f64()
    );
    Ok(DiarizationModelBundle::from_dir(diarization_models_dir))
}

pub fn build_transcription_pipeline(
    bundle: TranscriptionModelBundle,
) -> Result<LongFormTranscriptionPipeline> {
    let transcription_started = Instant::now();
    let transcription = LongFormTranscriptionPipeline::from_bundle(bundle)?;
    debug!(
        "Built transcription pipeline in {:.2}s",
        transcription_started.elapsed().as_secs_f64()
    );
    Ok(transcription)
}

pub fn build_diarization_pipeline(
    bundle: DiarizationModelBundle,
) -> Result<OwnedDiarizationPipeline> {
    let diarization_started = Instant::now();
    let diarization = OwnedDiarizationPipeline::from_bundle(bundle, DIARIZATION_EXECUTION_MODE)?;
    debug!(
        "Built diarization pipeline in {:.2}s",
        diarization_started.elapsed().as_secs_f64()
    );
    Ok(diarization)
}
