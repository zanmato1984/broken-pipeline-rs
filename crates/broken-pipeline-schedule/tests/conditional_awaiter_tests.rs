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

#[test]
fn conditional_lifetime_abandoned() {
    let first: SharedResumer = Arc::new(CallbackResumer::default());
    let second: SharedResumer = Arc::new(CallbackResumer::default());
    let awaiter =
        ConditionalAwaiter::new(2, vec![Arc::clone(&first), Arc::clone(&second)]).unwrap();
    let weak = Arc::downgrade(&awaiter);
    assert!(!awaiter.is_ready());
    assert_eq!(awaiter.resumers().len(), 2);
    assert_eq!(Arc::strong_count(&first), 2);
    assert_eq!(Arc::strong_count(&second), 2);

    drop(awaiter);

    assert_eq!(
        weak.strong_count(),
        0,
        "abandoned awaiter retained by callbacks"
    );
    assert!(weak.upgrade().is_none());
    assert_eq!(Arc::strong_count(&first), 1);
    assert_eq!(Arc::strong_count(&second), 1);
    assert!(!first.is_resumed());
    assert!(!second.is_resumed());
    first.resume();
    first.resume();
    assert!(first.is_resumed());
    assert!(!second.is_resumed());
    assert!(
        weak.upgrade().is_none(),
        "late resume resurrected the awaiter"
    );
    second.resume();
    second.resume();
    assert!(weak.upgrade().is_none());
}

#[test]
fn conditional_lifetime_all_handles_dropped_without_resume() {
    let resumer: SharedResumer = Arc::new(CallbackResumer::default());
    let weak_resumer = Arc::downgrade(&resumer);
    let awaiter = ConditionalAwaiter::new(1, vec![Arc::clone(&resumer)]).unwrap();
    let weak_awaiter = Arc::downgrade(&awaiter);
    drop(resumer);
    assert!(!awaiter.resumers()[0].is_resumed());

    drop(awaiter);

    // These are strong-object observations, not physical allocation-free proof.
    assert_eq!(weak_awaiter.strong_count(), 0);
    assert_eq!(weak_resumer.strong_count(), 0);
    assert!(weak_awaiter.upgrade().is_none());
    assert!(weak_resumer.upgrade().is_none());
}

#[test]
fn conditional_lifetime_partial_constructor() {
    use broken_pipeline_schedule::detail::CoroResumer;
    use broken_pipeline_schedule::traits::ScheduleError;

    let first: SharedResumer = Arc::new(CallbackResumer::default());
    let wrong: SharedResumer = Arc::new(CoroResumer::default());
    // The exact valid concrete type is first: registration succeeds before the
    // second element triggers the existing downcast error.
    let result = ConditionalAwaiter::new(2, vec![Arc::clone(&first), Arc::clone(&wrong)]);
    assert!(matches!(
        result,
        Err(ScheduleError::UnexpectedResumerType {
            awaiter: "ConditionalAwaiter",
            expected: "CallbackResumer",
        })
    ));
    assert_eq!(
        Arc::strong_count(&first),
        1,
        "partial construction retained valid input"
    );
    assert_eq!(
        Arc::strong_count(&wrong),
        1,
        "partial construction retained wrong input"
    );
    assert!(!first.is_resumed());
    assert!(!wrong.is_resumed());
    // A registered callback may outlive the failed constructor.
    first.resume();
    first.resume();
    assert!(first.is_resumed());
    assert!(!wrong.is_resumed());
    assert_eq!(Arc::strong_count(&first), 1);
    assert_eq!(Arc::strong_count(&wrong), 1);
}

#[test]
fn conditional_lifetime_readiness_and_duplicate_resume() {
    let first: SharedResumer = Arc::new(CallbackResumer::default());
    let second: SharedResumer = Arc::new(CallbackResumer::default());
    first.resume();
    let awaiter =
        ConditionalAwaiter::new(2, vec![Arc::clone(&first), Arc::clone(&second)]).unwrap();
    assert_eq!(Arc::strong_count(&awaiter), 1);
    assert!(!awaiter.is_ready());
    first.resume();
    assert!(
        !awaiter.is_ready(),
        "duplicate resume counted as another input"
    );
    second.resume();
    assert!(awaiter.is_ready());
    awaiter.wait();
    second.resume();
    assert!(awaiter.is_ready());
    let ready = ConditionalAwaiter::new(1, vec![first]).unwrap();
    assert!(
        ready.is_ready(),
        "resume before construction must notify immediately"
    );
    ready.wait();
}

#[test]
fn conditional_lifetime_validation_and_registration_semantics() {
    use broken_pipeline_schedule::traits::ScheduleError;

    assert!(matches!(
        ConditionalAwaiter::new(1, vec![]),
        Err(ScheduleError::EmptyResumers {
            awaiter: "ConditionalAwaiter"
        })
    ));
    let resumer: SharedResumer = Arc::new(CallbackResumer::default());
    assert!(matches!(
        ConditionalAwaiter::new(0, vec![Arc::clone(&resumer)]),
        Err(ScheduleError::InvalidReadyCount {
            awaiter: "ConditionalAwaiter",
            num_readies: 0
        })
    ));
    let unreachable = ConditionalAwaiter::new(2, vec![Arc::clone(&resumer)]).unwrap();
    let repeated =
        ConditionalAwaiter::new(2, vec![Arc::clone(&resumer), Arc::clone(&resumer)]).unwrap();
    resumer.resume();
    assert!(
        !unreachable.is_ready(),
        "positive oversized ready counts remain permitted"
    );
    assert!(
        repeated.is_ready(),
        "each registered input entry still counts"
    );
    repeated.wait();
}

#[test]
fn conditional_lifetime_drop_resume_race() {
    use std::sync::Barrier;

    // Every round owns a real awaiter; both racing operations start at the
    // barrier. Joining both threads also releases any successful temporary
    // upgrade before checking destruction.
    for _ in 0..32 {
        let resumer: SharedResumer = Arc::new(CallbackResumer::default());
        let awaiter = ConditionalAwaiter::new(1, vec![Arc::clone(&resumer)]).unwrap();
        let weak = Arc::downgrade(&awaiter);
        let start = Arc::new(Barrier::new(3));
        let drop_start = Arc::clone(&start);
        let dropper = std::thread::spawn(move || {
            drop_start.wait();
            drop(awaiter);
        });
        let resume_start = Arc::clone(&start);
        let racing_resumer = Arc::clone(&resumer);
        let resuming = std::thread::spawn(move || {
            resume_start.wait();
            racing_resumer.resume();
            racing_resumer.resume();
        });
        start.wait();
        dropper.join().unwrap();
        resuming.join().unwrap();
        assert!(weak.upgrade().is_none());
        assert_eq!(Arc::strong_count(&resumer), 1);
        resumer.resume();
        assert!(weak.upgrade().is_none());
    }
}

#[test]
fn conditional_lifetime_concurrent_ready_count() {
    use std::sync::Barrier;

    let resumers = (0..4)
        .map(|_| Arc::new(CallbackResumer::default()) as SharedResumer)
        .collect::<Vec<_>>();
    let awaiter = ConditionalAwaiter::new(4, resumers.clone()).unwrap();
    let start = Arc::new(Barrier::new(9));
    let mut handles = Vec::new();
    for resumer in &resumers {
        for _ in 0..2 {
            let resumer = Arc::clone(resumer);
            let start = Arc::clone(&start);
            handles.push(std::thread::spawn(move || {
                start.wait();
                resumer.resume();
            }));
        }
    }
    assert!(!awaiter.is_ready());
    start.wait();
    for handle in handles {
        handle.join().unwrap();
    }
    assert!(awaiter.is_ready());
    awaiter.wait();
    assert_eq!(Arc::strong_count(&awaiter), 1);
}
