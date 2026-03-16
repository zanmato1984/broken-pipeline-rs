use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use arrow_schema::ArrowError;
use broken_pipeline::{Continuation, SharedResumer, Task, TaskHint, TaskHintType, TaskStatus};
use broken_pipeline_schedule::{
    AsyncDualPoolScheduler, NaiveParallelScheduler, ParallelCoroScheduler, SequentialCoroScheduler,
    TaskContext, TaskGroup,
};

trait SchedulerLike: Default + Send + Sync + 'static {
    fn make_ctx(&self) -> TaskContext;
    fn schedule(
        &self,
        group: TaskGroup,
        ctx: TaskContext,
    ) -> broken_pipeline_schedule::TaskGroupHandle;
    fn wait(
        &self,
        handle: broken_pipeline_schedule::TaskGroupHandle,
    ) -> broken_pipeline_schedule::Result<TaskStatus>;
}

impl SchedulerLike for NaiveParallelScheduler {
    fn make_ctx(&self) -> TaskContext {
        self.make_task_context(None)
    }
    fn schedule(
        &self,
        group: TaskGroup,
        ctx: TaskContext,
    ) -> broken_pipeline_schedule::TaskGroupHandle {
        self.schedule_task_group(group, ctx)
    }
    fn wait(
        &self,
        handle: broken_pipeline_schedule::TaskGroupHandle,
    ) -> broken_pipeline_schedule::Result<TaskStatus> {
        self.wait_task_group(handle)
    }
}

impl SchedulerLike for AsyncDualPoolScheduler {
    fn make_ctx(&self) -> TaskContext {
        self.make_task_context(None)
    }
    fn schedule(
        &self,
        group: TaskGroup,
        ctx: TaskContext,
    ) -> broken_pipeline_schedule::TaskGroupHandle {
        self.schedule_task_group(group, ctx)
    }
    fn wait(
        &self,
        handle: broken_pipeline_schedule::TaskGroupHandle,
    ) -> broken_pipeline_schedule::Result<TaskStatus> {
        self.wait_task_group(handle)
    }
}

impl SchedulerLike for ParallelCoroScheduler {
    fn make_ctx(&self) -> TaskContext {
        self.make_task_context(None)
    }
    fn schedule(
        &self,
        group: TaskGroup,
        ctx: TaskContext,
    ) -> broken_pipeline_schedule::TaskGroupHandle {
        self.schedule_task_group(group, ctx)
    }
    fn wait(
        &self,
        handle: broken_pipeline_schedule::TaskGroupHandle,
    ) -> broken_pipeline_schedule::Result<TaskStatus> {
        self.wait_task_group(handle)
    }
}

impl SchedulerLike for SequentialCoroScheduler {
    fn make_ctx(&self) -> TaskContext {
        self.make_task_context(None)
    }
    fn schedule(
        &self,
        group: TaskGroup,
        ctx: TaskContext,
    ) -> broken_pipeline_schedule::TaskGroupHandle {
        self.schedule_task_group(group, ctx)
    }
    fn wait(
        &self,
        handle: broken_pipeline_schedule::TaskGroupHandle,
    ) -> broken_pipeline_schedule::Result<TaskStatus> {
        self.wait_task_group(handle)
    }
}

fn run_task_group<S: SchedulerLike>(
    task: Task<broken_pipeline_schedule::Traits>,
    num_tasks: usize,
    continuation: Option<Continuation<broken_pipeline_schedule::Traits>>,
) -> broken_pipeline_schedule::Result<TaskStatus> {
    let scheduler = S::default();
    let group = if let Some(continuation) = continuation {
        TaskGroup::with_continuation("ScheduleTest", task, num_tasks, continuation)
    } else {
        TaskGroup::new("ScheduleTest", task, num_tasks)
    };
    let ctx = scheduler.make_ctx();
    let handle = scheduler.schedule(group, ctx);
    scheduler.wait(handle)
}

fn run_scheduler_suite<S: SchedulerLike>() {
    let task = Task::new("Task", |_, _| Ok(TaskStatus::Finished));
    let result = run_task_group::<S>(task, 4, None).unwrap();
    assert!(result.is_finished());

    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = Arc::clone(&counter);
    let task = Task::new("Task", move |_, _| {
        counter_clone.fetch_add(1, Ordering::SeqCst);
        Ok(TaskStatus::Finished)
    });
    let cont_counter = Arc::clone(&counter);
    let continuation = Continuation::new("Cont", move |_| {
        if cont_counter.load(Ordering::SeqCst) == 8 {
            Ok(TaskStatus::Finished)
        } else {
            Err(ArrowError::ComputeError(
                "continuation observed wrong count".into(),
            ))
        }
    });
    let result = run_task_group::<S>(task, 8, Some(continuation)).unwrap();
    assert!(result.is_finished());

    let yielded = Arc::new(Mutex::new(vec![false; 8]));
    let yielded_clone = Arc::clone(&yielded);
    let task = Task::new("YieldTask", move |_, task_id| {
        let mut yielded = yielded_clone.lock().unwrap();
        if !yielded[task_id] {
            yielded[task_id] = true;
            Ok(TaskStatus::Yield)
        } else {
            Ok(TaskStatus::Finished)
        }
    });
    let result = run_task_group::<S>(task, 8, None).unwrap();
    assert!(result.is_finished());

    let resumers = Arc::new(Mutex::new(vec![None::<SharedResumer>; 8]));
    let num_resumers = Arc::new(AtomicUsize::new(0));
    let completed = Arc::new(AtomicUsize::new(0));
    let blocked_resumers = Arc::clone(&resumers);
    let blocked_num_resumers = Arc::clone(&num_resumers);
    let blocked_completed = Arc::clone(&completed);
    let blocked_task = Task::new("BlockedTask", move |ctx, task_id| {
        let mut resumers = blocked_resumers.lock().unwrap();
        if resumers[task_id].is_none() {
            let resumer = ctx.make_resumer()?;
            let awaiter = ctx.make_awaiter(vec![Arc::clone(&resumer)])?;
            resumers[task_id] = Some(resumer);
            blocked_num_resumers.fetch_add(1, Ordering::SeqCst);
            return Ok(TaskStatus::Blocked(awaiter));
        }
        blocked_completed.fetch_add(1, Ordering::SeqCst);
        Ok(TaskStatus::Finished)
    });

    let resumer_resumers = Arc::clone(&resumers);
    let resumer_num_resumers = Arc::clone(&num_resumers);
    let resumer_task = Task::with_hint(
        "ResumerTask",
        move |_, _| {
            if resumer_num_resumers.load(Ordering::SeqCst) != 8 {
                thread::sleep(Duration::from_millis(5));
                return Ok(TaskStatus::Continue);
            }
            thread::sleep(Duration::from_millis(15));
            for resumer in resumer_resumers.lock().unwrap().iter().flatten() {
                resumer.resume();
            }
            Ok(TaskStatus::Finished)
        },
        TaskHint {
            kind: TaskHintType::Io,
        },
    );

    let blocked_future = thread::spawn(move || run_task_group::<S>(blocked_task, 8, None));
    let resumer_future = thread::spawn(move || run_task_group::<S>(resumer_task, 1, None));

    assert!(blocked_future.join().unwrap().unwrap().is_finished());
    assert!(resumer_future.join().unwrap().unwrap().is_finished());
    assert_eq!(completed.load(Ordering::SeqCst), 8);

    let gate = Arc::new(AtomicBool::new(false));
    let gate_clone = Arc::clone(&gate);
    let errors = Arc::new(AtomicUsize::new(0));
    let errors_clone = Arc::clone(&errors);
    let task = Task::new("ErrorTask", move |_, _| {
        if !gate_clone.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(5));
            return Ok(TaskStatus::Continue);
        }
        if errors_clone.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(ArrowError::ComputeError("42".into()));
        }
        Ok(TaskStatus::Cancelled)
    });
    let trigger = thread::spawn(move || {
        thread::sleep(Duration::from_millis(20));
        gate.store(true, Ordering::SeqCst);
    });
    let error = run_task_group::<S>(task, 8, None).unwrap_err();
    assert!(error.to_string().contains("42"));
    trigger.join().unwrap();
}

macro_rules! scheduler_suite {
    ($name:ident, $ty:ty) => {
        mod $name {
            use super::*;
            #[test]
            fn all_scheduler_behaviors_match() {
                run_scheduler_suite::<$ty>();
            }
        }
    };
}

scheduler_suite!(naive_parallel, NaiveParallelScheduler);
scheduler_suite!(async_dual_pool, AsyncDualPoolScheduler);
scheduler_suite!(parallel_coro, ParallelCoroScheduler);
scheduler_suite!(sequential_coro, SequentialCoroScheduler);
