use super::*;
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot};

struct Work {
    queued: std::time::Instant,
    request: Option<Request>,
    reply: oneshot::Sender<Result<Reply>>,
}

/// A request rejected before enqueueing is returned intact for retry. Accepted
/// mutations are never silently dropped or diverted to an in-memory fallback.
#[derive(Debug)]
pub struct Rejected {
    pub error: StorageError,
    pub request: Option<Request>,
}

/// Owns an in-flight slot until the reply is consumed or dropped, so a caller
/// cannot leave unlimited completed pages buffered inside pending requests.
pub struct Pending {
    reply: oneshot::Receiver<Result<Reply>>,
    _permit: OwnedSemaphorePermit,
}

impl Pending {
    pub async fn wait(self) -> Result<Reply> {
        self.reply.await.map_err(|_| StorageError::Closed)?
    }
}

#[derive(Clone)]
pub struct StorageHandle {
    tx: mpsc::Sender<Work>,
    slots: Arc<Semaphore>,
}

impl StorageHandle {
    #[cfg(all(test, feature = "sqlite"))]
    pub(crate) fn available_slots(&self) -> usize {
        self.slots.available_permits()
    }

    /// Initializes the backend on a dedicated blocking thread. The normal
    /// bound is 8 in-flight operations; callers can choose 1..=32. SQLite's
    /// connection, transactions, busy waits and checkpoints never run on Tokio.
    pub async fn start<F, B>(capacity: usize, open: F) -> Result<Self>
    where
        F: FnOnce() -> Result<B> + Send + 'static,
        B: TransportStorage + 'static,
    {
        if !(1..=32).contains(&capacity) {
            return Err(StorageError::Invalid("worker capacity must be 1..=32"));
        }
        let (tx, mut rx) = mpsc::channel::<Work>(capacity);
        let (ready_tx, ready_rx) = oneshot::channel();
        std::thread::Builder::new()
            .name("rns-storage".into())
            .spawn(move || {
                let mut backend = match open() {
                    Ok(b) => b,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                if ready_tx.send(Ok(())).is_err() {
                    return;
                }
                let mut shutdown_replies = Vec::new();
                let mut metrics = metrics::WorkerMetrics::default();
                while let Some(work) = rx.blocking_recv() {
                    if let Some(request) = work.request {
                        let operation = request.operation();
                        let queue = work.queued.elapsed();
                        let queue_ms = queue.as_millis() as u64;
                        let started = std::time::Instant::now();
                        tracing::debug!(operation, queue_ms, "transport storage operation started");
                        let result = backend.execute(request);
                        let execution = started.elapsed();
                        let execution_ms = execution.as_millis() as u64;
                        metrics.record(operation, queue, execution, result.is_err());
                        if queue_ms >= 100 || execution_ms >= 100 {
                            tracing::debug!(
                                operation,
                                queue_ms,
                                execution_ms,
                                failed = result.is_err(),
                                "transport storage operation delayed"
                            );
                        } else {
                            tracing::debug!(
                                operation,
                                queue_ms,
                                execution_ms,
                                "transport storage operation completed"
                            );
                        }
                        let _ = work.reply.send(result);
                    } else {
                        // Reject future sends, but finish every accepted operation.
                        rx.close();
                        shutdown_replies.push(work.reply);
                    }
                }
                let checkpoint = backend.execute(Request::Checkpoint);
                drop(backend); // Release DB and owner lock before acknowledging shutdown.
                match checkpoint {
                    Ok(Reply::Checkpoint { remaining_frames }) => {
                        for reply in shutdown_replies {
                            let _ = reply.send(Ok(Reply::Checkpoint { remaining_frames }));
                        }
                    }
                    Ok(_) => {
                        for reply in shutdown_replies {
                            let _ =
                                reply.send(Err(StorageError::Invalid("invalid checkpoint reply")));
                        }
                    }
                    Err(error) => {
                        tracing::error!(%error, "transport storage shutdown checkpoint failed");
                        // The first shutdown caller receives the original SQL/IO
                        // error; additional callers learn that shutdown failed.
                        let mut error = Some(error);
                        for reply in shutdown_replies {
                            let _ = reply.send(Err(error.take().unwrap_or(StorageError::Closed)));
                        }
                    }
                }
            })?;
        ready_rx.await.map_err(|_| StorageError::Closed)??;
        Ok(Self {
            tx,
            slots: Arc::new(Semaphore::new(capacity)),
        })
    }

    pub fn try_submit(&self, request: Request) -> std::result::Result<Pending, Box<Rejected>> {
        if let Err(error) = request.validate() {
            return Err(Box::new(Rejected {
                error,
                request: Some(request),
            }));
        }
        self.enqueue(Some(request))
    }

    /// Ordered shutdown. Busy means nothing was submitted: consume a pending
    /// result and retry. The acknowledgment comes after drain, checkpoint and
    /// releasing ownership. Dropping all handles also drains the queue.
    pub fn try_shutdown(&self) -> std::result::Result<Pending, Box<Rejected>> {
        self.enqueue(None)
    }

    fn enqueue(&self, request: Option<Request>) -> std::result::Result<Pending, Box<Rejected>> {
        if self.tx.is_closed() {
            return Err(Box::new(Rejected {
                error: StorageError::Closed,
                request,
            }));
        }
        let permit = match self.slots.clone().try_acquire_owned() {
            Ok(p) => p,
            Err(_) => {
                return Err(Box::new(Rejected {
                    error: StorageError::Busy,
                    request,
                }));
            }
        };
        let (tx, rx) = oneshot::channel();
        match self.tx.try_send(Work {
            queued: std::time::Instant::now(),
            request,
            reply: tx,
        }) {
            Ok(()) => Ok(Pending {
                reply: rx,
                _permit: permit,
            }),
            Err(e) => {
                let error = if matches!(&e, mpsc::error::TrySendError::Full(_)) {
                    StorageError::Busy
                } else {
                    StorageError::Closed
                };
                Err(Box::new(Rejected {
                    error,
                    request: e.into_inner().request,
                }))
            }
        }
    }
}
