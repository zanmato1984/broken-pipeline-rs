use std::any::Any;
use std::sync::{Arc, Condvar, Mutex};

use broken_pipeline::{Awaiter, SharedResumer};

use super::SingleThreadResumer;
use crate::traits::ScheduleError;

pub struct SingleThreadAwaiter {
    num_readies: usize,
    resumers: Vec<SharedResumer>,
    state: Mutex<usize>,
    ready: Condvar,
}

impl SingleThreadAwaiter {
    pub fn new(
        num_readies: usize,
        resumers: Vec<SharedResumer>,
    ) -> Result<Arc<Self>, ScheduleError> {
        if resumers.is_empty() {
            return Err(ScheduleError::EmptyResumers {
                awaiter: "SingleThreadAwaiter",
            });
        }
        if num_readies == 0 {
            return Err(ScheduleError::InvalidReadyCount {
                awaiter: "SingleThreadAwaiter",
                num_readies,
            });
        }

        let awaiter = Arc::new(Self {
            num_readies,
            resumers,
            state: Mutex::new(0),
            ready: Condvar::new(),
        });

        for resumer in &awaiter.resumers {
            let callback_resumer = resumer
                .as_any()
                .downcast_ref::<SingleThreadResumer>()
                .ok_or(ScheduleError::UnexpectedResumerType {
                    awaiter: "SingleThreadAwaiter",
                    expected: "SingleThreadResumer",
                })?;
            let awaiter_clone = Arc::clone(&awaiter);
            callback_resumer.add_callback(move || awaiter_clone.notify_ready());
        }

        Ok(awaiter)
    }

    pub fn wait(&self) {
        let mut ready_count = self
            .state
            .lock()
            .expect("single-thread awaiter mutex poisoned");
        while *ready_count < self.num_readies {
            ready_count = self
                .ready
                .wait(ready_count)
                .expect("single-thread awaiter mutex poisoned");
        }
    }

    pub fn is_ready(&self) -> bool {
        *self
            .state
            .lock()
            .expect("single-thread awaiter mutex poisoned")
            >= self.num_readies
    }

    pub fn resumers(&self) -> &[SharedResumer] {
        &self.resumers
    }

    fn notify_ready(&self) {
        let mut ready_count = self
            .state
            .lock()
            .expect("single-thread awaiter mutex poisoned");
        *ready_count += 1;
        if *ready_count >= self.num_readies {
            self.ready.notify_all();
        }
    }
}

impl Awaiter for SingleThreadAwaiter {
    fn as_any(&self) -> &dyn Any {
        self
    }
}
