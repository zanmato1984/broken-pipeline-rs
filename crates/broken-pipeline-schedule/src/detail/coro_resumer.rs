use std::any::Any;

use broken_pipeline::Resumer;

use super::CallbackResumer;

#[derive(Default)]
pub struct CoroResumer {
    inner: CallbackResumer,
}

impl CoroResumer {
    pub fn add_callback<F>(&self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.inner.add_callback(callback);
    }
}

impl Resumer for CoroResumer {
    fn resume(&self) {
        self.inner.resume();
    }

    fn is_resumed(&self) -> bool {
        self.inner.is_resumed()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
