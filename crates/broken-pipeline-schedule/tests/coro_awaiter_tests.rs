use std::sync::Arc;

use broken_pipeline_core::SharedResumer;
use broken_pipeline_schedule::detail::{CoroAwaiter, CoroResumer};

#[test]
fn coro_resumer_and_awaiter_support_all_ready_mode() {
    let resumers = (0..4)
        .map(|_| Arc::new(CoroResumer::default()) as SharedResumer)
        .collect::<Vec<_>>();
    let awaiter = CoroAwaiter::new(4, resumers.clone()).unwrap();

    for resumer in &resumers {
        resumer.resume();
    }

    awaiter.wait();
    assert!(awaiter.is_ready());
}
