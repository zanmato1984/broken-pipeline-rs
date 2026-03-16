use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use arrow_schema::ArrowError;
use broken_pipeline::{SharedAwaiter, SharedResumer};

use crate::detail::{CallbackResumer, ConditionalAwaiter};
use crate::traits::{Result, TaskContext, TaskGroup, TaskStatus};

pub struct TaskGroupHandle {
    pub(crate) join_handle: Option<JoinHandle<Result<TaskStatus>>>,
    pub(crate) statuses: Arc<Mutex<Vec<TaskStatus>>>,
}

impl TaskGroupHandle {
    pub fn statuses(&self) -> Vec<TaskStatus> {
        self.statuses
            .lock()
            .expect("task group status mutex poisoned")
            .clone()
    }
}

#[derive(Clone, Default)]
pub struct NaiveParallelScheduler;

pub(crate) fn wait_handle(mut handle: TaskGroupHandle) -> Result<TaskStatus> {
    handle
        .join_handle
        .take()
        .expect("task group handle already awaited")
        .join()
        .expect("task group thread panicked")
}

impl NaiveParallelScheduler {
    pub fn make_task_context(&self, context: Option<Arc<dyn Any + Send + Sync>>) -> TaskContext {
        TaskContext::new(
            context,
            Arc::new(|| Ok(Arc::new(CallbackResumer::default()) as SharedResumer)),
            Arc::new(|resumers| {
                Ok(ConditionalAwaiter::new(1, resumers)? as Arc<dyn broken_pipeline::Awaiter>)
            }),
        )
    }

    pub fn schedule_task_group(&self, group: TaskGroup, task_ctx: TaskContext) -> TaskGroupHandle {
        let statuses = Arc::new(Mutex::new(Vec::new()));
        let statuses_clone = Arc::clone(&statuses);
        let join_handle = thread::spawn(move || {
            run_task_group(
                group,
                task_ctx,
                Arc::new(wait_conditional_ready),
                statuses_clone,
            )
        });
        TaskGroupHandle {
            join_handle: Some(join_handle),
            statuses,
        }
    }

    pub fn wait_task_group(&self, handle: TaskGroupHandle) -> Result<TaskStatus> {
        wait_handle(handle)
    }
}

pub(crate) type AwaiterReadyFn =
    Arc<dyn Fn(&SharedAwaiter) -> Result<bool> + Send + Sync + 'static>;

pub(crate) fn run_task_group(
    group: TaskGroup,
    task_ctx: TaskContext,
    awaiter_ready: AwaiterReadyFn,
    statuses: Arc<Mutex<Vec<TaskStatus>>>,
) -> Result<TaskStatus> {
    let task = group.task().clone();
    let mut blocked = vec![None; group.num_tasks()];
    let mut complete = vec![false; group.num_tasks()];
    let cancelled = Arc::new(AtomicBool::new(false));
    let mut saw_cancelled = false;

    loop {
        if complete.iter().all(|done| *done) {
            break;
        }

        let mut progressed = false;
        for task_id in 0..group.num_tasks() {
            if complete[task_id] {
                continue;
            }

            if cancelled.load(Ordering::SeqCst) {
                complete[task_id] = true;
                saw_cancelled = true;
                progressed = true;
                continue;
            }

            if let Some(awaiter) = blocked[task_id].as_ref() {
                if !(awaiter_ready)(awaiter)? {
                    continue;
                }
                blocked[task_id] = None;
            }

            match task.run(&task_ctx, task_id) {
                Ok(status) => {
                    statuses
                        .lock()
                        .expect("task group status mutex poisoned")
                        .push(status.clone());
                    progressed = true;
                    match status {
                        TaskStatus::Continue | TaskStatus::Yield => {}
                        TaskStatus::Blocked(awaiter) => {
                            blocked[task_id] = Some(awaiter);
                        }
                        TaskStatus::Finished => {
                            complete[task_id] = true;
                        }
                        TaskStatus::Cancelled => {
                            complete[task_id] = true;
                            saw_cancelled = true;
                        }
                    }
                }
                Err(error) => {
                    cancelled.store(true, Ordering::SeqCst);
                    return Err(error);
                }
            }
        }

        if !progressed {
            thread::sleep(Duration::from_millis(1));
        } else {
            thread::yield_now();
        }
    }

    if saw_cancelled {
        return Ok(TaskStatus::Cancelled);
    }

    if let Some(continuation) = group.continuation().cloned() {
        run_continuation(continuation, task_ctx, awaiter_ready, statuses)
    } else {
        Ok(TaskStatus::Finished)
    }
}

fn run_continuation(
    continuation: broken_pipeline::Continuation<crate::traits::Traits>,
    task_ctx: TaskContext,
    awaiter_ready: AwaiterReadyFn,
    statuses: Arc<Mutex<Vec<TaskStatus>>>,
) -> Result<TaskStatus> {
    let mut blocked = None;

    loop {
        if let Some(awaiter) = blocked.as_ref() {
            if !(awaiter_ready)(awaiter)? {
                thread::sleep(Duration::from_millis(1));
                continue;
            }
            blocked = None;
        }

        match continuation.run(&task_ctx) {
            Ok(status) => {
                statuses
                    .lock()
                    .expect("task group status mutex poisoned")
                    .push(status.clone());
                match status {
                    TaskStatus::Continue | TaskStatus::Yield => {
                        thread::yield_now();
                    }
                    TaskStatus::Blocked(awaiter) => {
                        blocked = Some(awaiter);
                    }
                    TaskStatus::Finished => return Ok(TaskStatus::Finished),
                    TaskStatus::Cancelled => return Ok(TaskStatus::Cancelled),
                }
            }
            Err(error) => return Err(error),
        }
    }
}

pub(crate) fn wait_conditional_ready(awaiter: &SharedAwaiter) -> Result<bool> {
    awaiter
        .as_any()
        .downcast_ref::<ConditionalAwaiter>()
        .map(|awaiter| awaiter.is_ready())
        .ok_or_else(|| unexpected_awaiter("ConditionalAwaiter"))
}

pub(crate) fn wait_future_ready(awaiter: &SharedAwaiter) -> Result<bool> {
    awaiter
        .as_any()
        .downcast_ref::<crate::detail::FutureAwaiter>()
        .map(|awaiter| awaiter.is_ready())
        .ok_or_else(|| unexpected_awaiter("FutureAwaiter"))
}

pub(crate) fn wait_coro_ready(awaiter: &SharedAwaiter) -> Result<bool> {
    awaiter
        .as_any()
        .downcast_ref::<crate::detail::CoroAwaiter>()
        .map(|awaiter| awaiter.is_ready())
        .ok_or_else(|| unexpected_awaiter("CoroAwaiter"))
}

pub(crate) fn wait_single_thread_ready(awaiter: &SharedAwaiter) -> Result<bool> {
    awaiter
        .as_any()
        .downcast_ref::<crate::detail::SingleThreadAwaiter>()
        .map(|awaiter| awaiter.is_ready())
        .ok_or_else(|| unexpected_awaiter("SingleThreadAwaiter"))
}

fn unexpected_awaiter(expected: &str) -> ArrowError {
    ArrowError::ComputeError(format!("unexpected awaiter type, expected {expected}"))
}
