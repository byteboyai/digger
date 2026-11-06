//! A bounded, serial executor for this workbench's disk operations. Enqueueing
//! never waits on a disk lock; dropping the UI does not join a blocked filesystem.
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};

type Job = Box<dyn FnOnce() + Send>;

pub struct IoExecutor {
    tx: SyncSender<Job>,
}

impl IoExecutor {
    pub fn new() -> Self {
        let (tx, rx) = sync_channel::<Job>(8);
        std::thread::Builder::new()
            .name("writing-io".into())
            .spawn(move || {
                while let Ok(job) = rx.recv() {
                    job();
                }
            })
            .expect("start writing IO worker");
        Self { tx }
    }

    pub fn submit<T: Send + 'static>(
        &self,
        run: impl FnOnce() -> T + Send + 'static,
    ) -> Result<tokio::sync::oneshot::Receiver<Result<T, String>>, String> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.tx
            .try_send(Box::new(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run))
                    .map_err(|_| "文稿 IO 任务异常退出，原始文件仍保留".to_owned());
                let _ = tx.send(result);
            }))
            .map_err(|error| match error {
                TrySendError::Full(_) => "文稿 IO 队列已满，请稍后重试".to_owned(),
                TrySendError::Disconnected(_) => "文稿 IO 工作线程已停止".to_owned(),
            })?;
        Ok(rx)
    }

    pub fn dispatch(&self, run: impl FnOnce() + Send + 'static) -> Result<(), String> {
        self.tx
            .try_send(Box::new(move || {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run));
            }))
            .map_err(|_| "文稿 IO 队列不可用或已满".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blocked_io_has_a_bounded_nonblocking_queue_and_recovers_after_panic() {
        let executor = IoExecutor::new();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let first = executor
            .submit(move || {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            })
            .unwrap();
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        let mut queued = vec![];
        for value in 0..8 {
            queued.push(executor.submit(move || value).unwrap());
        }
        assert!(
            executor.submit(|| ()).is_err(),
            "ninth queued task must fail immediately"
        );
        release_tx.send(()).unwrap();
        first.blocking_recv().unwrap().unwrap();
        for (value, receiver) in queued.into_iter().enumerate() {
            assert_eq!(receiver.blocking_recv().unwrap().unwrap(), value);
        }
        assert!(
            executor
                .submit(|| panic!("fixture"))
                .unwrap()
                .blocking_recv()
                .unwrap()
                .is_err()
        );
        assert_eq!(
            executor
                .submit(|| 42)
                .unwrap()
                .blocking_recv()
                .unwrap()
                .unwrap(),
            42
        );
    }
}
