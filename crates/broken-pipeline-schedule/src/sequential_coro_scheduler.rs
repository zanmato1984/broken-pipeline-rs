use std::any::Any;
use std::sync::Arc;

use broken_pipeline_core::SharedResumer;

use crate::detail::{SingleThreadAwaiter, SingleThreadResumer};
use crate::naive_parallel_scheduler::{
    run_task_group, wait_handle, wait_single_thread_ready, TaskGroupHandle,
};
use crate::traits::{TaskContext, TaskGroup};

#[derive(Clone, Default)]
pub struct SequentialCoroScheduler;

impl SequentialCoroScheduler {
    pub fn make_task_context(&self, context: Option<Arc<dyn Any + Send + Sync>>) -> TaskContext {
        TaskContext::new(
            context,
            Arc::new(|| Ok(Arc::new(SingleThreadResumer::default()) as SharedResumer)),
            Arc::new(|resumers| {
                Ok(SingleThreadAwaiter::new(1, resumers)?
                    as Arc<dyn broken_pipeline_core::Awaiter>)
            }),
        )
    }

    pub fn schedule_task_group(&self, group: TaskGroup, task_ctx: TaskContext) -> TaskGroupHandle {
        let statuses = Arc::new(std::sync::Mutex::new(Vec::new()));
        let statuses_clone = Arc::clone(&statuses);
        let join_handle = std::thread::spawn(move || {
            run_task_group(
                group,
                task_ctx,
                Arc::new(wait_single_thread_ready),
                statuses_clone,
            )
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
