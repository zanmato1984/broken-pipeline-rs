mod support;

use std::sync::Arc;

use broken_pipeline::{
    Continuation, SharedAwaiter, SharedResumer, Task, TaskContext, TaskGroup, TaskHintType,
    TaskStatus,
};

use support::{test_context, RecordedAwaiter, TestResumer, TestTypes};

struct NoopAwaiter;

impl broken_pipeline::Awaiter for NoopAwaiter {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[test]
fn task_context_and_status_helpers_match_cpp_contract() {
    let ctx = TaskContext::<TestTypes>::new(
        41usize,
        Arc::new(|| Ok(Arc::new(TestResumer::default()) as SharedResumer)),
        Arc::new(|resumers| Ok(Arc::new(RecordedAwaiter::new(resumers)) as SharedAwaiter)),
    );

    assert_eq!(ctx.context(), &41usize);
    assert_eq!(*ctx.shared_context(), 41usize);
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

#[test]
fn borrowed_and_owned_factory_public_apis_preserve_identity_and_inputs() {
    use broken_pipeline::{AwaiterFactory, BorrowedAwaiterFactory, ResumerFactory};
    let shared = Arc::new(91usize);
    let signal: SharedResumer = Arc::new(TestResumer::default());
    let expected = signal.clone();
    let ready: SharedAwaiter = Arc::new(NoopAwaiter);
    let returned = ready.clone();
    let borrowed: BorrowedAwaiterFactory<TestTypes> = Arc::new(move |slice| {
        assert_eq!(slice.len(), 1);
        assert!(Arc::ptr_eq(&slice[0], &expected));
        Ok(returned.clone())
    });
    let r = signal.clone();
    let rf: ResumerFactory<TestTypes> = Arc::new(move || Ok(r.clone()));
    let ctx = TaskContext::<TestTypes>::from_shared_context_with_borrowed_awaiter(
        shared.clone(),
        rf.clone(),
        borrowed,
    );
    let input = ctx.with_input_id(17);
    assert_eq!(ctx.input_id(), None);
    assert_eq!(input.input_id(), Some(17));
    assert!(Arc::ptr_eq(&shared, &input.shared_context()));
    assert!(Arc::ptr_eq(
        &ready,
        &input
            .make_awaiter_from_slice(std::slice::from_ref(&signal))
            .unwrap()
    ));
    assert!(Arc::ptr_eq(
        &ready,
        &input.make_awaiter(vec![signal.clone()]).unwrap()
    ));
    let owned: AwaiterFactory<TestTypes> = Arc::new(|r| Ok(Arc::new(RecordedAwaiter::new(r))));
    let legacy = TaskContext::<TestTypes>::from_shared_context(shared.clone(), rf, owned);
    let old_owned = legacy.make_awaiter(vec![signal.clone()]).unwrap();
    let old_slice = legacy.make_awaiter_from_slice(&[signal]).unwrap();
    for a in [old_owned, old_slice] {
        assert_eq!(
            a.as_any()
                .downcast_ref::<RecordedAwaiter>()
                .unwrap()
                .resumers()
                .len(),
            1
        );
    }
    assert!(Arc::ptr_eq(&shared, &legacy.shared_context()));
}

#[test]
fn borrowed_context_retains_existing_clone_contract() {
    #[derive(Clone)]
    struct Types;
    impl broken_pipeline::PipelineTypes for Types {
        type Batch = ();
        type Error = ();
        type Context = usize;
    }
    let shared = Arc::new(12);
    let ctx = TaskContext::<Types>::from_shared_context_with_borrowed_awaiter(
        shared.clone(),
        Arc::new(|| Ok(Arc::new(TestResumer::default()))),
        Arc::new(|_| Ok(Arc::new(NoopAwaiter))),
    );
    let cloned = ctx.with_input_id(4).clone();
    assert_eq!(cloned.input_id(), Some(4));
    assert!(Arc::ptr_eq(&shared, &cloned.shared_context()));
}
