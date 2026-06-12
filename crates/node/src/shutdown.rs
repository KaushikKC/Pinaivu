//! Process-wide shutdown coordination.
//!
//! A single `watch<bool>` channel is the one source of truth: `main` flips it
//! to `true` on Ctrl-C (or any future trigger), and every long-lived task — the
//! HTTP API server, the health server, the daemon event loop, the deadline
//! watcher — awaits [`wait`] and tears down cleanly. This replaces each task
//! owning its own `ctrl_c()` (which gave abrupt, uncoordinated exits).

use tokio::sync::watch;

/// Sender half — held by `main`; `send(true)` initiates shutdown.
pub type ShutdownTx = watch::Sender<bool>;
/// Receiver half — cloned to every task that needs to stop on shutdown.
pub type ShutdownRx = watch::Receiver<bool>;

/// Create a fresh shutdown channel (initially "not shutting down").
pub fn channel() -> (ShutdownTx, ShutdownRx) {
    watch::channel(false)
}

/// Resolve once shutdown has been signalled. Safe to call after the signal has
/// already fired (returns immediately). A closed channel also resolves it, so a
/// dropped sender never strands a task.
pub async fn wait(mut rx: ShutdownRx) {
    // If already true, return at once; otherwise wait for the flip.
    if *rx.borrow_and_update() {
        return;
    }
    let _ = rx.wait_for(|v| *v).await;
}
