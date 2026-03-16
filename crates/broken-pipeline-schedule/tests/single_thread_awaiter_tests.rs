use std::sync::Arc;

use broken_pipeline::SharedResumer;
use broken_pipeline_schedule::detail::{SingleThreadAwaiter, SingleThreadResumer};

#[test]
fn single_thread_awaiter_waits_for_all_resumers() {
    let resumers = (0..3)
        .map(|_| Arc::new(SingleThreadResumer::default()) as SharedResumer)
        .collect::<Vec<_>>();
    let awaiter = SingleThreadAwaiter::new(3, resumers.clone()).unwrap();

    for resumer in &resumers {
        resumer.resume();
    }

    awaiter.wait();
    assert!(awaiter.is_ready());
}
