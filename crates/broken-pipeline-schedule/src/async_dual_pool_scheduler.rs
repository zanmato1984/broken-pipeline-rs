use std::any::Any;
use std::sync::Arc;

use broken_pipeline_core::SharedResumer;

use crate::detail::{CallbackResumer, FutureAwaiter};
use crate::naive_parallel_scheduler::{
    run_task_group, wait_future_ready, wait_handle, TaskGroupHandle,
};
use crate::traits::{TaskContext, TaskGroup};

#[derive(Clone, Debug)]
pub struct AsyncDualPoolScheduler {
    cpu_threads: usize,
    io_threads: usize,
}

impl Default for AsyncDualPoolScheduler {
    fn default() -> Self {
        Self::new(4, 2)
    }
}

impl AsyncDualPoolScheduler {
    pub fn new(cpu_threads: usize, io_threads: usize) -> Self {
        Self {
            cpu_threads,
            io_threads,
        }
    }

    pub fn cpu_threads(&self) -> usize {
        self.cpu_threads
    }

    pub fn io_threads(&self) -> usize {
        self.io_threads
    }

    pub fn make_task_context(&self, context: Option<Arc<dyn Any + Send + Sync>>) -> TaskContext {
        let _ = (self.cpu_threads, self.io_threads);
        TaskContext::new(
            context,
            Arc::new(|| Ok(Arc::new(CallbackResumer::default()) as SharedResumer)),
            Arc::new(|resumers| {
                Ok(FutureAwaiter::new(1, resumers)? as Arc<dyn broken_pipeline_core::Awaiter>)
            }),
        )
    }

    pub fn schedule_task_group(&self, group: TaskGroup, task_ctx: TaskContext) -> TaskGroupHandle {
        let statuses = Arc::new(std::sync::Mutex::new(Vec::new()));
        let statuses_clone = Arc::clone(&statuses);
        let join_handle = std::thread::spawn(move || {
            run_task_group(group, task_ctx, Arc::new(wait_future_ready), statuses_clone)
        });
        TaskGroupHandle {
            join_handle: Some(join_handle),
            statuses,
        }
    }

    pub fn wait_task_group(
        &self,
        handle: TaskGroupHandle,
    ) -> crate::traits::Result<crate::traits::TaskStatus> {
        wait_handle(handle)
    }
}
