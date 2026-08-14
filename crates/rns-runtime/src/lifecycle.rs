//! Ctrl-C / SIGTERM flip a shared flag that all runtime tasks observe,
//! allowing them to detach interfaces and flush state cleanly before exit.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::time::Duration;

use tokio::sync::mpsc;

#[derive(Clone)]
pub struct ShutdownSignal {
    flag: Arc<AtomicBool>,
    exit_code: Arc<AtomicU8>,
    notify: Arc<tokio::sync::Notify>,
}

impl ShutdownSignal {
    pub fn new() -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
            exit_code: Arc::new(AtomicU8::new(0)),
            notify: Arc::new(tokio::sync::Notify::new()),
        }
    }

    pub fn trigger(&self) {
        if !self.flag.swap(true, Ordering::SeqCst) {
            tracing::debug!("shutdown triggered, notifying waiters");
        }
        self.notify.notify_waiters();
    }

    pub fn request_exit(&self, code: u8) -> bool {
        self.exit_code
            .compare_exchange(0, code, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    pub fn exit_code(&self) -> u8 {
        self.exit_code.load(Ordering::SeqCst)
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

/// Upper bound on how long the runtime's `TransportActor` teardown will wait
/// for outstanding [`DrainGuard`]s before proceeding anyway. Every current
/// owner bounds its own drain to 5s (`drain_grace`) plus a 300ms
/// `INTERFACE_FLUSH_GRACE` best-effort wait; this sits comfortably above
/// that combined worst case so it only ever fires as a last-resort safety
/// net, not as the normal path.
pub const TRANSPORT_SHUTDOWN_DRAIN_TIMEOUT: Duration = Duration::from_secs(7);

/// The runtime's `TransportActor` teardown (`reticulum.rs`'s dedicated
/// shutdown task) and every shutdown-aware `LinkManager` owner (rncp, rnsh,
/// remote management, the blackhole publisher, `LinkListener`, ...) all
/// react to the *same* [`ShutdownSignal`], but the actor teardown has far
/// less work to do before it can act on it -- it just forwards one message.
/// A `LinkManager`'s own drain needs several more `.await` hops (check
/// in-flight transfers, build and send an explicit `LinkClose`, an RPC
/// barrier, a flush grace) before its outbound traffic is even handed to the
/// transport actor. Left uncoordinated, the actor teardown routinely wins
/// that race and drops the interface sockets before `LinkClose` reaches the
/// wire -- confirmed live over real TCP with real process-level SIGTERM
/// (see the shutdown-race live-testing report). `DrainCoordinator` closes
/// that gap: every owner holds a [`DrainGuard`] for exactly as long as its
/// drain-aware run loop is alive, and the actor teardown task waits for the
/// outstanding count to hit zero -- bounded by `max_wait`, so a genuinely
/// unreachable peer (whose drain runs out its own grace but never gets
/// acked) still can't hang shutdown forever.
#[derive(Clone)]
pub struct DrainCoordinator {
    count: Arc<AtomicUsize>,
    notify: Arc<tokio::sync::Notify>,
}

impl DrainCoordinator {
    pub fn new() -> Self {
        Self {
            count: Arc::new(AtomicUsize::new(0)),
            notify: Arc::new(tokio::sync::Notify::new()),
        }
    }

    /// Register one in-flight drain. Hold the returned guard for exactly as
    /// long as the owner's `run_until_shutdown` / `run_with_commands_until_shutdown`
    /// task is alive -- drop it (letting the task's natural end run the
    /// `Drop` impl) once that call returns, meaning this owner's own bounded
    /// drain has already finished (successfully or by timing out on its own
    /// grace) and its close/deregister traffic has been handed to the
    /// transport actor.
    pub fn register(&self) -> DrainGuard {
        self.count.fetch_add(1, Ordering::SeqCst);
        DrainGuard {
            coordinator: self.clone(),
        }
    }

    #[cfg(test)]
    fn outstanding(&self) -> usize {
        self.count.load(Ordering::SeqCst)
    }

    /// Wait until every registered guard has been released, or `max_wait`
    /// elapses, whichever comes first. Always returns -- never hangs
    /// indefinitely, even if a guard is leaked or an owner's own drain logic
    /// has a bug that keeps it from ever finishing.
    pub async fn wait_for_drain(&self, max_wait: Duration) {
        let deadline = tokio::time::Instant::now() + max_wait;
        loop {
            // Register before observing the count, same lost-wakeup-safe
            // pattern as `ShutdownSignal::wait`: if the last guard drops
            // between the count check and the await below, this `Notified`
            // is already a registered waiter and still fires.
            let notified = self.notify.notified();
            tokio::pin!(notified);
            if self.count.load(Ordering::SeqCst) == 0 {
                return;
            }
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                tracing::warn!(
                    outstanding = self.count.load(Ordering::SeqCst),
                    "drain coordinator wait expired with owners still draining"
                );
                return;
            }
            let _ = tokio::time::timeout(remaining, notified).await;
        }
    }
}

impl Default for DrainCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

pub struct DrainGuard {
    coordinator: DrainCoordinator,
}

impl Drop for DrainGuard {
    fn drop(&mut self) {
        self.coordinator.count.fetch_sub(1, Ordering::SeqCst);
        self.coordinator.notify.notify_waiters();
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
    fn exit_request_is_shared_and_only_accepted_once() {
        let signal = ShutdownSignal::new();
        let clone = signal.clone();
        assert!(signal.request_exit(100));
        assert!(!clone.request_exit(101));
        assert_eq!(clone.exit_code(), 100);
        assert!(!signal.is_triggered());
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

    #[tokio::test]
    async fn drain_coordinator_waits_for_every_outstanding_guard() {
        let coordinator = DrainCoordinator::new();
        let guard_a = coordinator.register();
        let guard_b = coordinator.register();
        assert_eq!(coordinator.outstanding(), 2);

        let waiter = coordinator.clone();
        let waited = tokio::spawn(async move {
            let started = std::time::Instant::now();
            waiter.wait_for_drain(Duration::from_secs(5)).await;
            started.elapsed()
        });

        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(
            !waited.is_finished(),
            "must still be waiting while guards are outstanding"
        );

        drop(guard_a);
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(
            !waited.is_finished(),
            "must still be waiting on the second guard"
        );

        drop(guard_b);
        let elapsed = tokio::time::timeout(Duration::from_secs(1), waited)
            .await
            .expect("wait_for_drain must return promptly once the last guard drops")
            .expect("task panicked");
        assert!(
            elapsed < Duration::from_millis(500),
            "should return as soon as the last guard drops, not wait out the full timeout: {elapsed:?}"
        );
        assert_eq!(coordinator.outstanding(), 0);
    }

    #[tokio::test]
    async fn drain_coordinator_bounds_the_wait_when_a_guard_is_never_released() {
        let coordinator = DrainCoordinator::new();
        let guard = coordinator.register();

        let started = std::time::Instant::now();
        coordinator.wait_for_drain(Duration::from_millis(100)).await;
        let elapsed = started.elapsed();

        assert!(
            elapsed >= Duration::from_millis(100),
            "must wait out the full bound: {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "must not hang indefinitely past the bound: {elapsed:?}"
        );
        // The leaked guard still reports outstanding work; only dropping it
        // (never happening here, by construction) would clear the count.
        drop(guard);
    }

    #[tokio::test]
    async fn drain_coordinator_returns_immediately_with_nothing_registered() {
        let coordinator = DrainCoordinator::new();
        let started = std::time::Instant::now();
        coordinator.wait_for_drain(Duration::from_secs(5)).await;
        assert!(
            started.elapsed() < Duration::from_millis(50),
            "an idle coordinator must not add any shutdown latency"
        );
    }
}
