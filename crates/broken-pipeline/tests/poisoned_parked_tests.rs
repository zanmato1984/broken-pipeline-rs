use broken_pipeline::{
    compile, Awaiter, OpOutput, Pipeline, PipelineChannel, PipelineTypes, Resumer, SharedResumer,
    SinkOperator, SourceOperator, TaskContext, ThreadId,
};
use std::{
    any::Any,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
struct Types;
impl PipelineTypes for Types {
    type Batch = ();
    type Error = ();
    type Context = ();
}
#[derive(Default)]
struct Signal(AtomicBool);
impl Resumer for Signal {
    fn resume(&self) {
        self.0.store(true, Ordering::SeqCst)
    }
    fn is_resumed(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
struct Ready;
impl Awaiter for Ready {
    fn as_any(&self) -> &dyn Any {
        self
    }
}
struct Source(SharedResumer);
impl SourceOperator<Types> for Source {
    fn name(&self) -> &str {
        "park-or-panic"
    }
    fn source(&self, _: &TaskContext<Types>, id: ThreadId) -> Result<OpOutput<()>, ()> {
        if id == 1 {
            panic!("intentional cross-task callback panic");
        }
        Ok(OpOutput::Blocked(self.0.clone()))
    }
}
struct Sink;
impl SinkOperator<Types> for Sink {
    fn name(&self) -> &str {
        "unreachable"
    }
    fn sink(&self, _: &TaskContext<Types>, _: ThreadId, _: Option<()>) -> Result<OpOutput<()>, ()> {
        panic!("sink is unreachable")
    }
}
#[test]
fn parked_task_reentry_after_other_task_poison_releases_observed_fatal_handles() {
    let signal: SharedResumer = Arc::new(Signal::default());
    let pipeline = Pipeline::new(
        "poisoned-parked",
        vec![PipelineChannel::new(
            Arc::new(Source(signal.clone())),
            vec![],
        )],
        Arc::new(Sink),
    );
    let compiled = compile(&pipeline, 2);
    let exec = compiled.pipelinexes()[0].pipe_exec();
    let task = exec.task_group().task().clone();
    let ctx = TaskContext::<Types>::from_shared_context_with_borrowed_awaiter(
        Arc::new(()),
        Arc::new(|| panic!("unused")),
        Arc::new(|_| Ok(Arc::new(Ready))),
    );
    let baseline = Arc::strong_count(&signal);
    assert!(task.run(&ctx, 0).unwrap().is_blocked());
    assert_eq!(
        Arc::strong_count(&signal),
        baseline + 1,
        "legitimate parked owner absent"
    );
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| task.run(&ctx, 1))).is_err());
    let before_reentry = Arc::strong_count(&signal);
    assert_eq!(
        before_reentry,
        baseline + 1,
        "do not require immediate global cleanup before task reentry"
    );
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| task.run(&ctx, 0))).is_err(),
        "poison fail-fast must be preserved"
    );
    let after_reentry = Arc::strong_count(&signal);
    drop(task);
    drop(exec);
    let after_executor_drop = Arc::strong_count(&signal);
    println!("parked strong counts: baseline={baseline}; after other task panic before reentry={before_reentry}; after parked task reentry={after_reentry}; after executor drop={after_executor_drop}");
    assert_eq!(
        after_executor_drop, baseline,
        "bounded fixture must release after executor ownership ends"
    );
    assert_eq!(after_reentry,baseline,"a parked task reentered after cross-task fatal poison retained its resumer; early poison assertion bypassed its local cleanup");
}

use std::sync::atomic::AtomicUsize;

struct ObservedParkSource {
    signal: SharedResumer,
    calls: [AtomicUsize; 3],
}

impl SourceOperator<Types> for ObservedParkSource {
    fn name(&self) -> &str {
        "observed-park-or-panic"
    }
    fn source(&self, _: &TaskContext<Types>, id: ThreadId) -> Result<OpOutput<()>, ()> {
        self.calls[id].fetch_add(1, Ordering::SeqCst);
        if id == 2 {
            panic!("intentional cross-task callback panic");
        }
        Ok(OpOutput::Blocked(self.signal.clone()))
    }
}

#[test]
fn fatal_reentry_cleans_only_its_own_parked_slot_and_drop_cleans_unobserved_slot() {
    for reenter_second in [false, true] {
        let signal: SharedResumer = Arc::new(Signal::default());
        let source = Arc::new(ObservedParkSource {
            signal: signal.clone(),
            calls: std::array::from_fn(|_| AtomicUsize::new(0)),
        });
        let pipeline = Pipeline::new(
            "parked-slots",
            vec![PipelineChannel::new(source.clone(), vec![])],
            Arc::new(Sink),
        );
        let exec = compile(&pipeline, 3).pipelinexes()[0].pipe_exec();
        let task = exec.task_group().task().clone();
        let factories = Arc::new(AtomicUsize::new(0));
        let observed_factories = factories.clone();
        let ctx = TaskContext::<Types>::from_shared_context_with_borrowed_awaiter(
            Arc::new(()),
            Arc::new(|| panic!("unused")),
            Arc::new(move |_| {
                observed_factories.fetch_add(1, Ordering::SeqCst);
                Ok(Arc::new(Ready))
            }),
        );
        let baseline = Arc::strong_count(&signal);
        assert!(task.run(&ctx, 0).unwrap().is_blocked());
        assert_eq!(Arc::strong_count(&signal), baseline + 1);
        assert!(task.run(&ctx, 1).unwrap().is_blocked());
        assert_eq!(Arc::strong_count(&signal), baseline + 2);
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| task.run(&ctx, 2))).is_err()
        );
        assert_eq!(
            Arc::strong_count(&signal),
            baseline + 2,
            "no global eager cleanup"
        );
        for _ in 0..2 {
            assert!(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| task.run(&ctx, 0)))
                    .is_err()
            );
            assert_eq!(
                Arc::strong_count(&signal),
                baseline + 1,
                "only reentered task releases its owner"
            );
        }
        if reenter_second {
            assert!(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| task.run(&ctx, 1)))
                    .is_err()
            );
            assert_eq!(Arc::strong_count(&signal), baseline);
        }
        assert_eq!(
            factories.load(Ordering::SeqCst),
            2,
            "no factory after poison"
        );
        for calls in &source.calls {
            assert_eq!(
                calls.load(Ordering::SeqCst),
                1,
                "no source callback after poison"
            );
        }
        drop(task);
        drop(exec);
        assert_eq!(
            Arc::strong_count(&signal),
            baseline,
            "drop releases even an unobserved parked owner"
        );
    }
}

struct DropPanicSignal(Arc<AtomicUsize>);
impl Resumer for DropPanicSignal {
    fn resume(&self) {}
    fn is_resumed(&self) -> bool {
        false
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
impl Drop for DropPanicSignal {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
        panic!("intentional resumer cleanup panic");
    }
}
struct DropPanicSource {
    drops: Arc<AtomicUsize>,
    calls: AtomicUsize,
}
impl SourceOperator<Types> for DropPanicSource {
    fn name(&self) -> &str {
        "cleanup-panic"
    }
    fn source(&self, _: &TaskContext<Types>, _: ThreadId) -> Result<OpOutput<()>, ()> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(OpOutput::Blocked(Arc::new(DropPanicSignal(
            self.drops.clone(),
        ))))
    }
}

#[test]
fn panic_while_discarding_fatal_resumer_still_poisons_all_task_entries() {
    let drops = Arc::new(AtomicUsize::new(0));
    let source = Arc::new(DropPanicSource {
        drops: drops.clone(),
        calls: AtomicUsize::new(0),
    });
    let pipeline = Pipeline::new(
        "cleanup-panic",
        vec![PipelineChannel::new(source.clone(), vec![])],
        Arc::new(Sink),
    );
    let exec = compile(&pipeline, 2).pipelinexes()[0].pipe_exec();
    let task = exec.task_group().task().clone();
    let factories = Arc::new(AtomicUsize::new(0));
    let observed_factories = factories.clone();
    let ctx = TaskContext::<Types>::from_shared_context_with_borrowed_awaiter(
        Arc::new(()),
        Arc::new(|| panic!("unused")),
        Arc::new(move |_| {
            observed_factories.fetch_add(1, Ordering::SeqCst);
            Err(())
        }),
    );
    for id in [0, 1, 0, 1] {
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| task.run(&ctx, id))).is_err()
        );
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }
    assert_eq!(source.calls.load(Ordering::SeqCst), 1);
    assert_eq!(factories.load(Ordering::SeqCst), 1);
    drop(task);
    drop(exec);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}
