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
use tracing::{debug, info};

pub struct PrefetchedModels {
    pub transcription: TranscriptionModelBundle,
    pub diarization: DiarizationModelBundle,
}

pub fn ensure_models(
    scriptrs_cache_dir: &Path,
    speakrs_cache_dir: &Path,
) -> Result<PrefetchedModels> {
    let ensure_started = Instant::now();

    debug!("ensuring transcription models");
    let transcription_started = Instant::now();
    let transcription =
        ScriptModelManager::with_cache_dir(scriptrs_cache_dir.to_path_buf())?.ensure_long_form()?;
    info!(
        "Ensured transcription models in {:.2}s",
        transcription_started.elapsed().as_secs_f64()
    );

    debug!("ensuring diarization models");
    let diarization_started = Instant::now();
    let diarization_models_dir =
        SpeakerModelManager::with_cache_dir(speakrs_cache_dir.to_path_buf())?
            .ensure(ExecutionMode::CoreMl)?;
    info!(
        "Ensured diarization models in {:.2}s",
        diarization_started.elapsed().as_secs_f64()
    );
    info!(
        "Ensured all model assets in {:.2}s",
        ensure_started.elapsed().as_secs_f64()
    );

    Ok(PrefetchedModels {
        transcription,
        diarization: DiarizationModelBundle::from_dir(diarization_models_dir),
    })
}

pub fn build_transcription_pipeline(
    bundle: TranscriptionModelBundle,
) -> Result<LongFormTranscriptionPipeline> {
    debug!("building transcription pipeline");
    let transcription_started = Instant::now();
    let transcription = LongFormTranscriptionPipeline::from_bundle(bundle)?;
    info!(
        "Built transcription pipeline in {:.2}s",
        transcription_started.elapsed().as_secs_f64()
    );
    Ok(transcription)
}

pub fn build_diarization_pipeline(
    bundle: DiarizationModelBundle,
) -> Result<OwnedDiarizationPipeline> {
    debug!("building diarization pipeline");
    let diarization_started = Instant::now();
    let diarization = OwnedDiarizationPipeline::from_bundle(bundle, ExecutionMode::CoreMl)?;
    info!(
        "Built diarization pipeline in {:.2}s",
        diarization_started.elapsed().as_secs_f64()
    );
    Ok(diarization)
}
