use std::any::Any;
use std::marker::PhantomData;
use std::sync::Arc;

use broken_pipeline::{BpResult, SharedResumer, TaskContext, TaskGroup, TaskStatus};

use crate::detail::{SingleThreadAwaiter, SingleThreadResumer};
use crate::naive_parallel_scheduler::{
    run_task_group, wait_handle, wait_single_thread_ready, TaskGroupHandle,
};
use crate::traits::ScheduleTypes;

#[derive(Clone)]
pub struct SequentialCoroScheduler<T: ScheduleTypes>
where
    T::Error: Send + 'static,
{
    _marker: PhantomData<fn() -> T>,
}

impl<T: ScheduleTypes> Default for SequentialCoroScheduler<T>
where
    T::Error: Send + 'static,
{
    fn default() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

impl<T: ScheduleTypes> SequentialCoroScheduler<T>
where
    T::Error: Send + 'static,
{
    pub fn make_task_context(&self, context: Option<Arc<dyn Any + Send + Sync>>) -> TaskContext<T> {
        TaskContext::new(
            context,
            Arc::new(|| Ok(Arc::new(SingleThreadResumer::default()) as SharedResumer)),
            Arc::new(|resumers| {
                SingleThreadAwaiter::new(1, resumers)
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
                Arc::new(wait_single_thread_ready::<T>),
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
