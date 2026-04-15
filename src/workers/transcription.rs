use color_eyre::Result;
use scriptrs::TranscriptionResult;
use std::path::PathBuf;
use std::time::Instant;
use tracing::debug;

use crate::console;
use crate::models::{build_transcription_pipeline, ensure_transcription_models};

use super::runner::{RunningWorker, Worker, WorkerOutcome};

pub(crate) struct TranscriptionWorker(Worker<TranscriptionResult>);

impl TranscriptionWorker {
    pub(crate) fn spawn(cache_dir: PathBuf) -> Self {
        Self(Worker::spawn("transcription", move |request_rx| {
            let stage_started = Instant::now();
            let bundle = ensure_transcription_models(&cache_dir)?;
            let pipeline = build_transcription_pipeline(bundle)?;
            debug!(
                "Built transcription stage in {:.2}s",
                stage_started.elapsed().as_secs_f64()
            );

            let audio = match request_rx.recv() {
                Ok(audio) => audio,
                Err(_) => return Ok(WorkerOutcome::CancelledBeforeInput),
            };

            console::info("Running transcription");
            let transcription_started = Instant::now();
            let transcription = pipeline.run(audio.as_ref())?;
            debug!(
                "Finished transcription in {:.2}s",
                transcription_started.elapsed().as_secs_f64()
            );

            Ok(WorkerOutcome::Completed(transcription))
        }))
    }

    pub(crate) fn start(
        self,
        audio: std::sync::Arc<[f32]>,
    ) -> Result<RunningWorker<TranscriptionResult>> {
        self.0.start(audio)
    }
}
