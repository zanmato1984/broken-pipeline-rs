mod support;

use std::sync::Arc;

use broken_pipeline_core::{
    Continuation, SharedAwaiter, SharedResumer, Task, TaskContext, TaskGroup, TaskHintType,
    TaskStatus,
};

use support::{test_context, RecordedAwaiter, TestResumer, TestTypes};

struct NoopAwaiter;

impl broken_pipeline_core::Awaiter for NoopAwaiter {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[test]
fn task_context_and_status_helpers_match_cpp_contract() {
    let ctx = TaskContext::<TestTypes>::new(
        Some(Arc::new(41usize)),
        Arc::new(|| Ok(Arc::new(TestResumer::default()) as SharedResumer)),
        Arc::new(|resumers| Ok(Arc::new(RecordedAwaiter::new(resumers)) as SharedAwaiter)),
    );

    assert_eq!(ctx.context_ref::<usize>(), &41usize);
    assert_eq!(ctx.context_as::<usize>(), Some(&41usize));
    assert!(TaskStatus::Continue.is_continue());
    assert!(TaskStatus::Yield.is_yield());
    assert!(TaskStatus::Finished.is_finished());
    assert!(TaskStatus::Cancelled.is_cancelled());
    assert_eq!(TaskStatus::Continue.label(), "CONTINUE");
    assert_eq!(TaskStatus::Yield.label(), "YIELD");
    assert_eq!(TaskStatus::Finished.label(), "FINISHED");
    assert_eq!(TaskStatus::Cancelled.label(), "CANCELLED");

    let blocked = TaskStatus::Blocked(Arc::new(NoopAwaiter));
    assert!(blocked.is_blocked());
    assert!(blocked.awaiter().is_some());
    assert_eq!(blocked.label(), "BLOCKED");
}

#[test]
fn task_group_preserves_task_and_continuation_metadata() {
    let ctx = test_context();
    let task = Task::<TestTypes>::new("Task", |_, _| Ok(TaskStatus::Finished));
    let continuation = Continuation::<TestTypes>::new("Continuation", |_| Ok(TaskStatus::Finished));
    let task_group = TaskGroup::with_continuation("Group", task.clone(), 2, continuation.clone());

    assert_eq!(task.run(&ctx, 0).unwrap().label(), "FINISHED");
    assert_eq!(continuation.run(&ctx).unwrap().label(), "FINISHED");
    assert_eq!(task_group.name(), "Group");
    assert_eq!(task_group.num_tasks(), 2);
    assert_eq!(task_group.task().name(), "Task");
    assert_eq!(
        task_group
            .continuation()
            .expect("missing continuation")
            .name(),
        "Continuation"
    );
    assert_eq!(task_group.task().hint().kind, TaskHintType::Cpu);
}
