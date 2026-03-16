use std::any::Any;
use std::sync::Arc;

use broken_pipeline::SharedResumer;

use crate::detail::{CoroAwaiter, CoroResumer};
use crate::naive_parallel_scheduler::{
    run_task_group, wait_coro_ready, wait_handle, TaskGroupHandle,
};
use crate::traits::{TaskContext, TaskGroup};

#[derive(Clone, Default)]
pub struct ParallelCoroScheduler;

impl ParallelCoroScheduler {
    pub fn make_task_context(&self, context: Option<Arc<dyn Any + Send + Sync>>) -> TaskContext {
        TaskContext::new(
            context,
            Arc::new(|| Ok(Arc::new(CoroResumer::default()) as SharedResumer)),
            Arc::new(|resumers| {
                Ok(CoroAwaiter::new(1, resumers)? as Arc<dyn broken_pipeline::Awaiter>)
            }),
        )
    }

    pub fn schedule_task_group(&self, group: TaskGroup, task_ctx: TaskContext) -> TaskGroupHandle {
        let statuses = Arc::new(std::sync::Mutex::new(Vec::new()));
        let statuses_clone = Arc::clone(&statuses);
        let join_handle = std::thread::spawn(move || {
            run_task_group(group, task_ctx, Arc::new(wait_coro_ready), statuses_clone)
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
