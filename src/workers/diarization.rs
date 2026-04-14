use color_eyre::{Result, eyre::eyre};
use speakrs::{BatchInput, DiarizationResult};
use std::path::PathBuf;
use std::time::Instant;
use tracing::debug;

use crate::console;
use crate::models::{build_diarization_pipeline, ensure_diarization_models};

use super::runner::{Worker, WorkerOutcome};

pub(crate) struct DiarizationWorker(Worker<DiarizationResult>);

impl DiarizationWorker {
    pub(crate) fn spawn(cache_dir: PathBuf) -> Self {
        Self(Worker::spawn("diarization", move |request_rx| {
            let stage_started = Instant::now();
            let bundle = ensure_diarization_models(&cache_dir)?;
            let mut pipeline = build_diarization_pipeline(bundle)?;
            debug!(
                "Built diarization stage in {:.2}s",
                stage_started.elapsed().as_secs_f64()
            );

            let audio = match request_rx.recv() {
                Ok(audio) => audio,
                Err(_) => return Ok(WorkerOutcome::CancelledBeforeInput),
            };

            console::info("Running diarization");
            let diarization_started = Instant::now();
            let diarization_results = pipeline.run_batch(&[BatchInput {
                audio: audio.as_ref(),
                file_id: "input",
            }])?;
            let diarization = diarization_results
                .into_iter()
                .next()
                .ok_or_else(|| eyre!("diarization returned no results"))?;
            debug!(
                "Finished diarization in {:.2}s",
                diarization_started.elapsed().as_secs_f64()
            );

            Ok(WorkerOutcome::Completed(diarization))
        }))
    }

    pub(crate) fn run(self, audio: std::sync::Arc<[f32]>) -> Result<DiarizationResult> {
        self.0.run(audio)
    }
}
