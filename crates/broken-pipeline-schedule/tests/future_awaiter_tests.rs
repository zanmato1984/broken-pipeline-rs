use std::sync::Arc;
use std::thread;
use std::time::Duration;

use broken_pipeline_core::SharedResumer;
use broken_pipeline_schedule::detail::{CallbackResumer, FutureAwaiter};

#[test]
fn future_awaiter_wait_first() {
    let resumer: SharedResumer = Arc::new(CallbackResumer::default());
    let awaiter = FutureAwaiter::new(1, vec![Arc::clone(&resumer)]).unwrap();
    let resume_handle = thread::spawn(move || {
        thread::sleep(Duration::from_millis(20));
        resumer.resume();
    });

    awaiter.wait();
    assert!(awaiter.is_ready());
    resume_handle.join().unwrap();
}

#[test]
fn future_awaiter_resume_first() {
    let resumer: SharedResumer = Arc::new(CallbackResumer::default());
    resumer.resume();
    let awaiter = FutureAwaiter::new(1, vec![resumer]).unwrap();
    awaiter.wait();
    assert!(awaiter.is_ready());
}
