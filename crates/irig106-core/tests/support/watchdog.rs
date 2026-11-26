//! Test-only wall-clock boundary around operations and thread completion.
use std::{
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
pub(crate) fn watchdog(timeout: Duration, operation: impl FnOnce() + Send + 'static) {
    let deadline = Instant::now() + timeout;
    let (finished, completion) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        let result = catch_unwind(AssertUnwindSafe(operation));
        let _ = finished.send(());
        result
    });
    let remaining = deadline.saturating_duration_since(Instant::now());
    completion
        .recv_timeout(remaining)
        .expect("whole-reader operation exceeded watchdog");
    while !worker.is_finished() {
        assert!(
            Instant::now() < deadline,
            "whole-reader join exceeded watchdog"
        );
        thread::yield_now();
    }
    if let Err(panic) = worker.join().expect("watchdog worker failed") {
        resume_unwind(panic);
    }
}
