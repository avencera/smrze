use color_eyre::Result;
use scriptrs::TranscriptionResult;
use std::path::PathBuf;
use std::time::Instant;
use tracing::debug;

use crate::console;
use crate::models::{build_transcription_pipeline, ensure_transcription_models};

use super::runner::{Worker, WorkerOutcome};

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

    pub(crate) fn run(self, audio: std::sync::Arc<[f32]>) -> Result<TranscriptionResult> {
        self.0.run(audio)
    }

    pub(crate) fn cancel(self) -> Result<()> {
        self.0.cancel()
    }
}
