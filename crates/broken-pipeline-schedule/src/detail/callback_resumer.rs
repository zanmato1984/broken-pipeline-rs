use std::any::Any;
use std::sync::Mutex;

use broken_pipeline::Resumer;

type Callback = Box<dyn Fn() + Send + Sync + 'static>;

#[derive(Default)]
pub struct CallbackResumer {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    resumed: bool,
    callbacks: Vec<Callback>,
}

impl CallbackResumer {
    pub fn add_callback<F>(&self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        let callback: Callback = Box::new(callback);
        let maybe_callback = {
            let mut state = self.state.lock().expect("callback resumer mutex poisoned");
            if state.resumed {
                Some(callback)
            } else {
                state.callbacks.push(callback);
                None
            }
        };

        if let Some(callback) = maybe_callback {
            callback();
        }
    }
}

impl Resumer for CallbackResumer {
    fn resume(&self) {
        let callbacks = {
            let mut state = self.state.lock().expect("callback resumer mutex poisoned");
            if state.resumed {
                return;
            }
            state.resumed = true;
            std::mem::take(&mut state.callbacks)
        };

        for callback in callbacks {
            callback();
        }
    }

    fn is_resumed(&self) -> bool {
        self.state
            .lock()
            .expect("callback resumer mutex poisoned")
            .resumed
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
