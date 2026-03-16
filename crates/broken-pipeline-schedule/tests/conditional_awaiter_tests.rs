use std::sync::Arc;
use std::thread;
use std::time::Duration;

use broken_pipeline::{Resumer, SharedResumer};
use broken_pipeline_schedule::detail::{CallbackResumer, ConditionalAwaiter};

#[test]
fn callback_resumer_runs_callbacks_immediately_after_resume() {
    let resumer = CallbackResumer::default();
    let state = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let state_clone = Arc::clone(&state);
    resumer.add_callback(move || {
        state_clone.store(true, std::sync::atomic::Ordering::SeqCst);
    });

    assert!(!resumer.is_resumed());
    resumer.resume();
    assert!(resumer.is_resumed());
    assert!(state.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn conditional_awaiter_waits_until_a_resumer_fires() {
    let resumer: SharedResumer = Arc::new(CallbackResumer::default());
    let awaiter = ConditionalAwaiter::new(1, vec![Arc::clone(&resumer)]).unwrap();
    let resume_handle = thread::spawn(move || {
        thread::sleep(Duration::from_millis(25));
        resumer.resume();
    });

    awaiter.wait();
    assert!(awaiter.is_ready());
    resume_handle.join().unwrap();
}

#[test]
fn conditional_awaiter_any_mode_unblocks_after_one_resumer() {
    let resumers = (0..8)
        .map(|_| Arc::new(CallbackResumer::default()) as SharedResumer)
        .collect::<Vec<_>>();
    let awaiter = ConditionalAwaiter::new(1, resumers.clone()).unwrap();

    resumers[3].resume();

    awaiter.wait();
    assert!(awaiter.is_ready());
    assert!(resumers[3].is_resumed());
    assert!(!resumers[0].is_resumed());
}
