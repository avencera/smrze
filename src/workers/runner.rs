use color_eyre::{
    Result,
    eyre::{Context, eyre},
};
use std::sync::{
    Arc,
    mpsc::{self, Receiver, SyncSender},
};
use std::thread::{self, JoinHandle};

pub(super) enum WorkerOutcome<T> {
    Completed(T),
    CancelledBeforeInput,
}

pub(super) struct Worker<T> {
    name: &'static str,
    request_tx: SyncSender<Arc<[f32]>>,
    join_handle: JoinHandle<Result<WorkerOutcome<T>>>,
}

impl<T: Send + 'static> Worker<T> {
    pub(super) fn spawn<F>(name: &'static str, run: F) -> Self
    where
        F: FnOnce(Receiver<Arc<[f32]>>) -> Result<WorkerOutcome<T>> + Send + 'static,
    {
        let (request_tx, request_rx) = mpsc::sync_channel(1);
        let join_handle = thread::spawn(move || run(request_rx));
        Self {
            name,
            request_tx,
            join_handle,
        }
    }

    pub(super) fn run(self, audio: Arc<[f32]>) -> Result<T> {
        let Worker {
            name,
            request_tx,
            join_handle,
        } = self;
        let send_result = request_tx.send(audio);
        let worker_result = join_worker(name, join_handle)?;

        if send_result.is_err() {
            return match worker_result {
                WorkerOutcome::Completed(_) => Err(eyre!("{name} worker exited unexpectedly")),
                WorkerOutcome::CancelledBeforeInput => {
                    Err(eyre!("{name} worker stopped before receiving audio"))
                }
            };
        }

        match worker_result {
            WorkerOutcome::Completed(value) => Ok(value),
            WorkerOutcome::CancelledBeforeInput => {
                Err(eyre!("{name} worker stopped without producing a result"))
            }
        }
    }

    pub(super) fn cancel(self) -> Result<()> {
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
    join_handle: JoinHandle<Result<WorkerOutcome<T>>>,
) -> Result<WorkerOutcome<T>> {
    join_handle
        .join()
        .map_err(|_| eyre!("{name} worker thread panicked"))?
        .with_context(|| format!("{name} worker failed"))
}

#[cfg(test)]
mod tests {
    use super::{Worker, WorkerOutcome, join_worker};
    use color_eyre::Result;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn run_returns_worker_result() -> Result<()> {
        let worker = Worker::spawn("test", |request_rx| {
            let audio = request_rx.recv().expect("audio should be sent");
            Ok(WorkerOutcome::Completed(audio.len()))
        });

        let result = worker.run(Arc::<[f32]>::from(vec![0.0_f32, 1.0, 2.0]))?;
        assert_eq!(result, 3);
        Ok(())
    }

    #[test]
    fn cancel_stops_worker_without_error() -> Result<()> {
        let worker = Worker::<usize>::spawn("test", |request_rx| {
            let _ = request_rx.recv();
            Ok(WorkerOutcome::CancelledBeforeInput)
        });

        worker.cancel()?;
        Ok(())
    }

    #[test]
    fn join_worker_returns_cancelled_before_input() -> Result<()> {
        let join_handle = thread::spawn(|| Ok(WorkerOutcome::<usize>::CancelledBeforeInput));
        let outcome = join_worker("test", join_handle)?;

        assert!(matches!(outcome, WorkerOutcome::CancelledBeforeInput));
        Ok(())
    }
}
