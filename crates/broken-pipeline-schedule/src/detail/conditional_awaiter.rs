use std::any::Any;
use std::sync::{Arc, Condvar, Mutex};

use arrow_schema::ArrowError;
use broken_pipeline::{Awaiter, SharedResumer};

use super::CallbackResumer;

pub struct ConditionalAwaiter {
    num_readies: usize,
    resumers: Vec<SharedResumer>,
    state: Mutex<usize>,
    ready: Condvar,
}

impl ConditionalAwaiter {
    pub fn new(num_readies: usize, resumers: Vec<SharedResumer>) -> Result<Arc<Self>, ArrowError> {
        if resumers.is_empty() {
            return Err(ArrowError::ComputeError(
                "ConditionalAwaiter: empty resumers".to_string(),
            ));
        }
        if num_readies == 0 {
            return Err(ArrowError::ComputeError(
                "ConditionalAwaiter: num_readies must be > 0".to_string(),
            ));
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
                .downcast_ref::<CallbackResumer>()
                .ok_or_else(|| {
                    ArrowError::ComputeError(
                        "ConditionalAwaiter: unexpected resumer type".to_string(),
                    )
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
            .expect("conditional awaiter mutex poisoned");
        while *ready_count < self.num_readies {
            ready_count = self
                .ready
                .wait(ready_count)
                .expect("conditional awaiter mutex poisoned");
        }
    }

    pub fn is_ready(&self) -> bool {
        *self
            .state
            .lock()
            .expect("conditional awaiter mutex poisoned")
            >= self.num_readies
    }

    pub fn resumers(&self) -> &[SharedResumer] {
        &self.resumers
    }

    fn notify_ready(&self) {
        let mut ready_count = self
            .state
            .lock()
            .expect("conditional awaiter mutex poisoned");
        *ready_count += 1;
        if *ready_count >= self.num_readies {
            self.ready.notify_all();
        }
    }
}

impl Awaiter for ConditionalAwaiter {
    fn as_any(&self) -> &dyn Any {
        self
    }
}
