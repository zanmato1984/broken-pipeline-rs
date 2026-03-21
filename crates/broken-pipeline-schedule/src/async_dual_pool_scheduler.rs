use std::any::Any;
use std::marker::PhantomData;
use std::sync::Arc;

use broken_pipeline::{BpResult, SharedResumer, TaskContext, TaskGroup, TaskStatus};

use crate::detail::{CallbackResumer, FutureAwaiter};
use crate::naive_parallel_scheduler::{
    run_task_group, wait_future_ready, wait_handle, TaskGroupHandle,
};
use crate::traits::{ScheduleTypes, Traits};

#[derive(Clone, Debug)]
pub struct AsyncDualPoolScheduler<T: ScheduleTypes = Traits>
where
    T::Error: Send + 'static,
{
    cpu_threads: usize,
    io_threads: usize,
    _marker: PhantomData<fn() -> T>,
}

impl<T: ScheduleTypes> Default for AsyncDualPoolScheduler<T>
where
    T::Error: Send + 'static,
{
    fn default() -> Self {
        Self::new(4, 2)
    }
}

impl<T: ScheduleTypes> AsyncDualPoolScheduler<T>
where
    T::Error: Send + 'static,
{
    pub fn new(cpu_threads: usize, io_threads: usize) -> Self {
        Self {
            cpu_threads,
            io_threads,
            _marker: PhantomData,
        }
    }

    pub fn cpu_threads(&self) -> usize {
        self.cpu_threads
    }

    pub fn io_threads(&self) -> usize {
        self.io_threads
    }

    pub fn make_task_context(&self, context: Option<Arc<dyn Any + Send + Sync>>) -> TaskContext<T> {
        let _ = (self.cpu_threads, self.io_threads);
        TaskContext::new(
            context,
            Arc::new(|| Ok(Arc::new(CallbackResumer::default()) as SharedResumer)),
            Arc::new(|resumers| {
                FutureAwaiter::new(1, resumers)
                    .map(|awaiter| awaiter as Arc<dyn broken_pipeline::Awaiter>)
                    .map_err(T::from_schedule_error)
            }),
        )
    }

    pub fn schedule_task_group(
        &self,
        group: TaskGroup<T>,
        task_ctx: TaskContext<T>,
    ) -> TaskGroupHandle<T> {
        let statuses = Arc::new(std::sync::Mutex::new(Vec::new()));
        let statuses_clone = Arc::clone(&statuses);
        let join_handle = std::thread::spawn(move || {
            run_task_group(
                group,
                task_ctx,
                Arc::new(wait_future_ready::<T>),
                statuses_clone,
            )
        });
        TaskGroupHandle {
            join_handle: Some(join_handle),
            statuses,
        }
    }

    pub fn wait_task_group(&self, handle: TaskGroupHandle<T>) -> BpResult<TaskStatus, T> {
        wait_handle(handle)
    }
}
