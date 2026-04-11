use color_eyre::{
    Result,
    eyre::{Context, eyre},
};
use scriptrs::TranscriptionResult;
use speakrs::{BatchInput, DiarizationResult};
use std::path::PathBuf;
use std::sync::{
    Arc,
    mpsc::{self, Receiver, SyncSender},
};
use std::thread::{self, JoinHandle};
use std::time::Instant;
use tracing::debug;

use crate::console;
use crate::models::{
    build_diarization_pipeline, build_transcription_pipeline, ensure_diarization_models,
    ensure_transcription_models,
};

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
                Err(_) => return Ok(None),
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

            Ok(Some(diarization))
        }))
    }

    pub(crate) fn run(self, audio: Arc<[f32]>) -> Result<DiarizationResult> {
        self.0.run(audio)
    }
}

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
                Err(_) => return Ok(None),
            };

            console::info("Running transcription");
            let transcription_started = Instant::now();
            let transcription = pipeline.run(audio.as_ref())?;
            debug!(
                "Finished transcription in {:.2}s",
                transcription_started.elapsed().as_secs_f64()
            );

            Ok(Some(transcription))
        }))
    }

    pub(crate) fn run(self, audio: Arc<[f32]>) -> Result<TranscriptionResult> {
        self.0.run(audio)
    }

    pub(crate) fn cancel(self) -> Result<()> {
        self.0.cancel()
    }
}

struct Worker<T> {
    name: &'static str,
    request_tx: SyncSender<Arc<[f32]>>,
    join_handle: JoinHandle<Result<Option<T>>>,
}

impl<T: Send + 'static> Worker<T> {
    fn spawn<F>(name: &'static str, run: F) -> Self
    where
        F: FnOnce(Receiver<Arc<[f32]>>) -> Result<Option<T>> + Send + 'static,
    {
        let (request_tx, request_rx) = mpsc::sync_channel(1);
        let join_handle = thread::spawn(move || run(request_rx));
        Self {
            name,
            request_tx,
            join_handle,
        }
    }

    fn run(self, audio: Arc<[f32]>) -> Result<T> {
        let Worker {
            name,
            request_tx,
            join_handle,
        } = self;
        let send_result = request_tx.send(audio);
        let worker_result = join_worker(name, join_handle)?;

        if send_result.is_err() {
            return match worker_result {
                Some(_) => Err(eyre!("{name} worker exited unexpectedly")),
                None => Err(eyre!("{name} worker stopped before receiving audio")),
            };
        }

        worker_result.ok_or_else(|| eyre!("{name} worker stopped without producing a result"))
    }

    fn cancel(self) -> Result<()> {
        let Worker {
            name,
            request_tx,
            join_handle,
        } = self;
        drop(request_tx);
        join_worker(name, join_handle).map(|_| ())
    }
}

fn join_worker<T>(
    name: &'static str,
    join_handle: JoinHandle<Result<Option<T>>>,
) -> Result<Option<T>> {
    join_handle
        .join()
        .map_err(|_| eyre!("{name} worker thread panicked"))?
        .with_context(|| format!("{name} worker failed"))
}

#[cfg(test)]
mod tests {
    use super::Worker;
    use color_eyre::Result;
    use std::sync::Arc;

    #[test]
    fn run_returns_worker_result() -> Result<()> {
        let worker = Worker::spawn("test", |request_rx| {
            let audio = request_rx.recv().expect("audio should be sent");
            Ok(Some(audio.len()))
        });

        let result = worker.run(Arc::<[f32]>::from(vec![0.0_f32, 1.0, 2.0]))?;
        assert_eq!(result, 3);
        Ok(())
    }

    #[test]
    fn cancel_stops_worker_without_error() -> Result<()> {
        let worker = Worker::<usize>::spawn("test", |request_rx| {
            let _ = request_rx.recv();
            Ok(None)
        });

        worker.cancel()?;
        Ok(())
    }
}
