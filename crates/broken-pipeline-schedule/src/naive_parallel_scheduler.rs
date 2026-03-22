use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use broken_pipeline::{
    BpResult, Continuation, SharedAwaiter, SharedResumer, TaskContext, TaskGroup, TaskStatus,
};

use crate::detail::{CallbackResumer, ConditionalAwaiter};
use crate::traits::{ScheduleError, ScheduleTypes};

pub struct TaskGroupHandle<T: ScheduleTypes>
where
    T::Error: Send + 'static,
{
    pub(crate) join_handle: Option<JoinHandle<BpResult<TaskStatus, T>>>,
    pub(crate) statuses: Arc<Mutex<Vec<TaskStatus>>>,
}

impl<T: ScheduleTypes> TaskGroupHandle<T>
where
    T::Error: Send + 'static,
{
    pub fn statuses(&self) -> Vec<TaskStatus> {
        self.statuses
            .lock()
            .expect("task group status mutex poisoned")
            .clone()
    }
}

#[derive(Clone)]
pub struct NaiveParallelScheduler<T: ScheduleTypes>
where
    T::Error: Send + 'static,
{
    _marker: PhantomData<fn() -> T>,
}

impl<T: ScheduleTypes> Default for NaiveParallelScheduler<T>
where
    T::Error: Send + 'static,
{
    fn default() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

pub(crate) fn wait_handle<T: ScheduleTypes>(
    mut handle: TaskGroupHandle<T>,
) -> BpResult<TaskStatus, T>
where
    T::Error: Send + 'static,
{
    handle
        .join_handle
        .take()
        .expect("task group handle already awaited")
        .join()
        .unwrap_or_else(|_| {
            Err(T::from_schedule_error(
                ScheduleError::TaskGroupThreadPanicked,
            ))
        })
}

impl<T: ScheduleTypes> NaiveParallelScheduler<T>
where
    T::Error: Send + 'static,
{
    pub fn make_task_context(&self, context: T::Context) -> TaskContext<T> {
        TaskContext::new(
            context,
            Arc::new(|| Ok(Arc::new(CallbackResumer::default()) as SharedResumer)),
            Arc::new(|resumers| {
                ConditionalAwaiter::new(1, resumers)
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
        let statuses = Arc::new(Mutex::new(Vec::new()));
        let statuses_clone = Arc::clone(&statuses);
        let join_handle = thread::spawn(move || {
            run_task_group(
                group,
                task_ctx,
                Arc::new(wait_conditional_ready::<T>),
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

pub(crate) type AwaiterReadyFnFor<T> =
    Arc<dyn Fn(&SharedAwaiter) -> BpResult<bool, T> + Send + Sync + 'static>;

pub(crate) fn run_task_group<T: ScheduleTypes>(
    group: TaskGroup<T>,
    task_ctx: TaskContext<T>,
    awaiter_ready: AwaiterReadyFnFor<T>,
    statuses: Arc<Mutex<Vec<TaskStatus>>>,
) -> BpResult<TaskStatus, T>
where
    T::Error: Send + 'static,
{
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

fn run_continuation<T: ScheduleTypes>(
    continuation: Continuation<T>,
    task_ctx: TaskContext<T>,
    awaiter_ready: AwaiterReadyFnFor<T>,
    statuses: Arc<Mutex<Vec<TaskStatus>>>,
) -> BpResult<TaskStatus, T>
where
    T::Error: Send + 'static,
{
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

pub(crate) fn wait_conditional_ready<T: ScheduleTypes>(awaiter: &SharedAwaiter) -> BpResult<bool, T>
where
    T::Error: Send + 'static,
{
    awaiter
        .as_any()
        .downcast_ref::<ConditionalAwaiter>()
        .map(|awaiter| awaiter.is_ready())
        .ok_or_else(|| unexpected_awaiter::<T>("ConditionalAwaiter"))
}

pub(crate) fn wait_future_ready<T: ScheduleTypes>(awaiter: &SharedAwaiter) -> BpResult<bool, T>
where
    T::Error: Send + 'static,
{
    awaiter
        .as_any()
        .downcast_ref::<crate::detail::FutureAwaiter>()
        .map(|awaiter| awaiter.is_ready())
        .ok_or_else(|| unexpected_awaiter::<T>("FutureAwaiter"))
}

pub(crate) fn wait_coro_ready<T: ScheduleTypes>(awaiter: &SharedAwaiter) -> BpResult<bool, T>
where
    T::Error: Send + 'static,
{
    awaiter
        .as_any()
        .downcast_ref::<crate::detail::CoroAwaiter>()
        .map(|awaiter| awaiter.is_ready())
        .ok_or_else(|| unexpected_awaiter::<T>("CoroAwaiter"))
}

pub(crate) fn wait_single_thread_ready<T: ScheduleTypes>(
    awaiter: &SharedAwaiter,
) -> BpResult<bool, T>
where
    T::Error: Send + 'static,
{
    awaiter
        .as_any()
        .downcast_ref::<crate::detail::SingleThreadAwaiter>()
        .map(|awaiter| awaiter.is_ready())
        .ok_or_else(|| unexpected_awaiter::<T>("SingleThreadAwaiter"))
}

fn unexpected_awaiter<T: ScheduleTypes>(expected: &'static str) -> T::Error
where
    T::Error: Send + 'static,
{
    T::from_schedule_error(ScheduleError::UnexpectedAwaiterType { expected })
}
