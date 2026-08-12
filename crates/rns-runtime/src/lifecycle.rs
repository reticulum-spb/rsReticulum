//! Ctrl-C / SIGTERM flip a shared flag that all runtime tasks observe,
//! allowing them to detach interfaces and flush state cleanly before exit.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::mpsc;

#[derive(Clone)]
pub struct ShutdownSignal {
    flag: Arc<AtomicBool>,
    notify: Arc<tokio::sync::Notify>,
}

impl ShutdownSignal {
    pub fn new() -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(tokio::sync::Notify::new()),
        }
    }

    pub fn trigger(&self) {
        if !self.flag.swap(true, Ordering::SeqCst) {
            tracing::debug!("shutdown triggered, notifying waiters");
        }
        self.notify.notify_waiters();
    }

    pub fn is_triggered(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    pub async fn wait(&self) {
        loop {
            // Register before observing the flag. If trigger() runs between
            // these two operations, this Notified is already a waiter; if it
            // ran earlier, the flag makes us return without awaiting it.
            let notified = self.notify.notified();
            tokio::pin!(notified);
            if self.is_triggered() {
                return;
            }
            notified.await;
            if self.is_triggered() {
                return;
            }
        }
    }
}

impl Default for ShutdownSignal {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ExitHandler {
    actions: Vec<Box<dyn FnOnce() + Send>>,
}

impl ExitHandler {
    pub fn new() -> Self {
        Self {
            actions: Vec::new(),
        }
    }

    pub fn register(&mut self, action: impl FnOnce() + Send + 'static) {
        self.actions.push(Box::new(action));
    }

    pub fn execute(self) {
        let n = self.actions.len();
        tracing::debug!(actions = n, "running exit handlers");
        for action in self.actions {
            action();
        }
    }
}

impl Default for ExitHandler {
    fn default() -> Self {
        Self::new()
    }
}

/// Install SIGINT and SIGTERM handlers that trip `shutdown`. Returned
/// receiver yields once on signal for await-based callers.
///
/// SIGTERM matters as much as SIGINT: it is what systemd, Docker, and
/// `kill` send, so a daemon that only handles SIGINT dies by default
/// disposition on every orderly stop — no drain, no exit handlers.
///
/// On unix the OS-level registration happens synchronously in this call — a
/// handler registered inside a spawned task only takes effect once that task
/// is first polled, leaving a window where the signal kills the process via
/// the default disposition.
pub fn install_signal_handlers(shutdown: ShutdownSignal) -> mpsc::Receiver<()> {
    let (tx, rx) = mpsc::channel(1);

    #[cfg(unix)]
    {
        use tokio::signal::unix::SignalKind;

        let mut sigint_registered = false;
        for (name, kind) in [
            ("SIGINT", SignalKind::interrupt()),
            ("SIGTERM", SignalKind::terminate()),
        ] {
            match tokio::signal::unix::signal(kind) {
                Ok(mut sig) => {
                    // One task per signal: a select! over both would drop the
                    // loser's registration when the first one fires.
                    let shutdown = shutdown.clone();
                    let tx = tx.clone();
                    tokio::spawn(async move {
                        sig.recv().await;
                        signal_shutdown(shutdown, tx).await;
                    });
                    if name == "SIGINT" {
                        sigint_registered = true;
                    }
                }
                Err(e) => tracing::warn!(signal = name, error = %e, "signal registration failed"),
            }
        }
        if !sigint_registered {
            spawn_ctrl_c_handler(shutdown, tx);
        }
    }

    #[cfg(not(unix))]
    spawn_ctrl_c_handler(shutdown, tx);

    rx
}

fn spawn_ctrl_c_handler(shutdown: ShutdownSignal, tx: mpsc::Sender<()>) {
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        signal_shutdown(shutdown, tx).await;
    });
}

async fn signal_shutdown(shutdown: ShutdownSignal, tx: mpsc::Sender<()>) {
    tracing::info!("received shutdown signal");
    shutdown.trigger();
    let _ = tx.send(()).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sends a real SIGTERM to this process. Before SIGTERM was registered
    /// this did not fail the assertion -- it killed the test binary outright
    /// via the default disposition, which is exactly the daemon behaviour
    /// being fixed.
    #[cfg(unix)]
    #[tokio::test]
    async fn sigterm_triggers_shutdown_instead_of_killing_the_process() {
        let shutdown = ShutdownSignal::new();
        let mut rx = install_signal_handlers(shutdown.clone());

        assert!(
            std::process::Command::new("kill")
                .args(["-TERM", &std::process::id().to_string()])
                .status()
                .expect("kill")
                .success()
        );

        tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("SIGTERM did not reach the handler")
            .expect("signal channel closed");
        assert!(shutdown.is_triggered());
    }

    #[test]
    fn test_shutdown_signal() {
        let signal = ShutdownSignal::new();
        assert!(!signal.is_triggered());
        signal.trigger();
        assert!(signal.is_triggered());
    }

    #[test]
    fn test_shutdown_signal_clone() {
        let signal = ShutdownSignal::new();
        let clone = signal.clone();
        signal.trigger();
        assert!(clone.is_triggered());
    }

    #[test]
    fn test_exit_handler() {
        use std::sync::atomic::{AtomicU32, Ordering};
        let counter = Arc::new(AtomicU32::new(0));

        let mut handler = ExitHandler::new();
        let c1 = counter.clone();
        handler.register(move || {
            c1.fetch_add(1, Ordering::SeqCst);
        });
        let c2 = counter.clone();
        handler.register(move || {
            c2.fetch_add(10, Ordering::SeqCst);
        });

        handler.execute();
        assert_eq!(counter.load(Ordering::SeqCst), 11);
    }

    #[tokio::test]
    async fn test_shutdown_wait() {
        let signal = ShutdownSignal::new();
        let signal_clone = signal.clone();

        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            signal_clone.trigger();
        });

        signal.wait().await;
        assert!(signal.is_triggered());
    }

    #[tokio::test]
    async fn shutdown_wait_returns_when_triggered_before_registration() {
        let signal = ShutdownSignal::new();
        signal.trigger();
        tokio::time::timeout(std::time::Duration::from_millis(100), signal.wait())
            .await
            .expect("pre-triggered shutdown must be retained");
    }
}
