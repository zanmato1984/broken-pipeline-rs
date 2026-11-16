// The allocator exists only in this integration-test executable, never in BP.
mod support;
use broken_pipeline::{
    compile, Awaiter, OpOutput, Pipeline, PipelineChannel, SharedAwaiter, SharedResumer,
    SinkOperator, SourceOperator, TaskContext, ThreadId,
};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use support::{TestResumer, TestTypes};

#[derive(Clone, Copy, Debug, Default)]
struct Requests {
    calls: usize,
    bytes: usize,
    sizes: [usize; 16],
}
thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static REQUESTS: Cell<Requests> = const { Cell::new(Requests { calls: 0, bytes: 0, sizes: [0;16] }) };
}
struct ObservedSystem;
fn requested(size: usize) {
    let _ = TRACK.try_with(|track| {
        if track.get() {
            let _ = REQUESTS.try_with(|cell| {
                let mut r = cell.get();
                if r.calls < r.sizes.len() {
                    r.sizes[r.calls] = size;
                }
                r.calls += 1;
                r.bytes += size;
                cell.set(r);
            });
        }
    });
}
// SAFETY: all operations forward the original pointer and layout to System.
unsafe impl GlobalAlloc for ObservedSystem {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        requested(l.size());
        unsafe { System.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        requested(l.size());
        unsafe { System.alloc_zeroed(l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        requested(n);
        unsafe { System.realloc(p, l, n) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
}
#[global_allocator]
static ALLOCATOR: ObservedSystem = ObservedSystem;
fn measured<R>(f: impl FnOnce() -> R) -> (R, Requests) {
    struct Disarm;
    impl Drop for Disarm {
        fn drop(&mut self) {
            TRACK.with(|t| t.set(false));
        }
    }
    REQUESTS.with(|r| r.set(Requests::default()));
    TRACK.with(|t| t.set(true));
    let guard = Disarm;
    let result = f();
    drop(guard);
    (result, REQUESTS.with(Cell::get))
}

struct FrozenAwaiter {
    signals: [SharedResumer; 2],
}
impl Awaiter for FrozenAwaiter {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
struct ParkedSource {
    signal: SharedResumer,
    calls: AtomicUsize,
}
impl SourceOperator<TestTypes> for ParkedSource {
    fn name(&self) -> &str {
        "parked"
    }
    fn source(
        &self,
        ctx: &TaskContext<TestTypes>,
        task: ThreadId,
    ) -> Result<OpOutput<i32>, String> {
        assert_eq!(task, 0);
        assert!([Some(7), Some(19)].contains(&ctx.input_id()));
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(OpOutput::Blocked(self.signal.clone()))
    }
}
struct NoSink;
impl SinkOperator<TestTypes> for NoSink {
    fn name(&self) -> &str {
        "unused"
    }
    fn sink(
        &self,
        _: &TaskContext<TestTypes>,
        _: ThreadId,
        _: Option<i32>,
    ) -> Result<OpOutput<i32>, String> {
        panic!("parked source reached sink")
    }
}

#[test]
fn cold_first_task_run_and_repeated_borrowed_scratch_have_no_vec_requests() {
    let signals: [SharedResumer; 2] =
        std::array::from_fn(|_| Arc::new(TestResumer::default()) as SharedResumer);
    let sources: [Arc<ParkedSource>; 2] = std::array::from_fn(|id| {
        Arc::new(ParkedSource {
            signal: signals[id].clone(),
            calls: AtomicUsize::new(0),
        })
    });
    let pipeline = Pipeline::new(
        "cold",
        vec![
            PipelineChannel::with_input_id(7, sources[0].clone(), vec![]),
            PipelineChannel::with_input_id(19, sources[1].clone(), vec![]),
        ],
        Arc::new(NoSink),
    );
    let exec = compile(&pipeline, 1).pipelinexes()[0].pipe_exec();
    let group = exec.task_group();
    let task = group.task();
    let frozen: SharedAwaiter = Arc::new(FrozenAwaiter {
        signals: signals.clone(),
    });
    let precreated = frozen.clone();
    let pointer = Arc::new(AtomicUsize::new(0));
    let seen = pointer.clone();
    let baseline = Arc::new([AtomicUsize::new(0), AtomicUsize::new(0)]);
    let inside_counts = Arc::new([AtomicUsize::new(0), AtomicUsize::new(0)]);
    let observed_counts = inside_counts.clone();
    let called = Arc::new(AtomicUsize::new(0));
    let factory_calls = called.clone();
    let ctx = TaskContext::from_shared_context_with_borrowed_awaiter(
        Arc::new(0usize),
        Arc::new(|| panic!("sources use independently owned precreated resumers")),
        Arc::new(move |slice| {
            assert_eq!(slice.len(), 2);
            let owned = precreated.as_any().downcast_ref::<FrozenAwaiter>().unwrap();
            for (id, r) in slice.iter().enumerate() {
                assert!(Arc::ptr_eq(r, &owned.signals[id]));
                observed_counts[id].store(Arc::strong_count(r), Ordering::SeqCst);
            }
            let address = slice.as_ptr() as usize;
            let previous = seen.swap(address, Ordering::SeqCst);
            assert!(
                previous == 0 || previous == address,
                "scratch backing changed"
            );
            factory_calls.fetch_add(1, Ordering::SeqCst);
            Ok(precreated.clone())
        }),
    );
    for (id, r) in signals.iter().enumerate() {
        baseline[id].store(Arc::strong_count(r), Ordering::SeqCst);
    }
    // No Task::run or task-local lock has happened before this measurement.
    let (first, cold) = measured(|| task.run(&ctx, 0));
    assert!(first.unwrap().is_blocked());
    println!("cold first-use Task::run standard-heap requests: {cold:?}");
    // A separate, untouched native mutex attributes the cold lock request;
    // this does not pre-warm the executor lock before its first-use measurement.
    let native_mutex = std::sync::Mutex::new(());
    let (_, native) = measured(|| *native_mutex.lock().unwrap());
    println!("separate cold native mutex first-lock requests: {native:?}");
    assert_eq!(
        (cold.calls, cold.bytes),
        (native.calls, native.bytes),
        "unattributed cold request"
    );
    assert!(cold.calls <= cold.sizes.len(), "request sample overflow");
    let vec_bytes = 2 * std::mem::size_of::<SharedResumer>();
    assert!(
        !cold.sizes[..cold.calls.min(16)].contains(&vec_bytes),
        "cold executor cloned resumers into Vec: {cold:?}"
    );
    for (id, count) in inside_counts.iter().enumerate() {
        assert_eq!(
            count.load(Ordering::SeqCst),
            baseline[id].load(Ordering::SeqCst) + 2,
            "only parked and temporary scratch handles may be added"
        );
    }
    for _ in 0..8 {
        let (result, requests) = measured(|| task.run(&ctx, 0));
        assert!(result.unwrap().is_blocked());
        assert_eq!(
            requests.calls, 0,
            "repeated executor container request: {requests:?}"
        );
        for (id, r) in signals.iter().enumerate() {
            assert_eq!(
                Arc::strong_count(r),
                baseline[id].load(Ordering::SeqCst) + 1,
                "temporary scratch handles escaped step"
            );
        }
    }
    assert_eq!(called.load(Ordering::SeqCst), 9);
    for source in &sources {
        assert_eq!(source.calls.load(Ordering::SeqCst), 1);
    }
    let (positive, requests) = measured(|| std::hint::black_box(vec![0u8; 257]));
    assert!(
        requests.calls > 0 && requests.bytes >= 257,
        "allocator observer failed positive control"
    );
    println!("allocating positive control: {requests:?}");
    drop(positive);
    drop(group);
    drop(exec);
    for (id, r) in signals.iter().enumerate() {
        assert_eq!(Arc::strong_count(r), baseline[id].load(Ordering::SeqCst));
    }
    let returned = frozen.clone();
    let legacy = TaskContext::<TestTypes>::from_shared_context(
        Arc::new(0usize),
        Arc::new(|| panic!("unused")),
        Arc::new(move |r| {
            assert_eq!(r.len(), 2);
            Ok(returned.clone())
        }),
    );
    let (_, conversion) = measured(|| legacy.make_awaiter_from_slice(&signals).unwrap());
    assert_eq!(conversion.calls, 1);
    assert_eq!(conversion.bytes, vec_bytes);
    println!("explicit legacy slice-to-owned-Vec conversion: {conversion:?}");
    let owned = signals.to_vec();
    let (_, owned_call) = measured(|| legacy.make_awaiter(owned).unwrap());
    assert_eq!(
        owned_call.calls, 0,
        "legacy owned API must use caller-owned Vec"
    );
    drop(frozen);
}

struct OtherSource {
    action: u8,
    calls: AtomicUsize,
}
impl SourceOperator<TestTypes> for OtherSource {
    fn name(&self) -> &str {
        "other"
    }
    fn source(&self, ctx: &TaskContext<TestTypes>, _: ThreadId) -> Result<OpOutput<i32>, String> {
        assert_eq!(ctx.input_id(), Some(19));
        self.calls.fetch_add(1, Ordering::SeqCst);
        match self.action {
            0 => Ok(OpOutput::Finished(Some(5))),
            1 => Err("fatal source".into()),
            2 => panic!("intentional callback unwind with earlier scratch handle"),
            _ => Ok(OpOutput::Finished(None)),
        }
    }
}
struct AcceptSink {
    calls: AtomicUsize,
}
impl SinkOperator<TestTypes> for AcceptSink {
    fn name(&self) -> &str {
        "accept"
    }
    fn sink(
        &self,
        ctx: &TaskContext<TestTypes>,
        _: ThreadId,
        input: Option<i32>,
    ) -> Result<OpOutput<i32>, String> {
        assert_eq!(ctx.input_id(), Some(19));
        assert_eq!(input, Some(5));
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(OpOutput::PipeSinkNeedsMore)
    }
}

#[test]
fn scratch_releases_handles_on_progress_fatal_error_and_callback_unwind() {
    for action in [0, 1, 2] {
        let signal: SharedResumer = Arc::new(TestResumer::default());
        let first = Arc::new(ParkedSource {
            signal: signal.clone(),
            calls: AtomicUsize::new(0),
        });
        let other = Arc::new(OtherSource {
            action,
            calls: AtomicUsize::new(0),
        });
        let sink = Arc::new(AcceptSink {
            calls: AtomicUsize::new(0),
        });
        let pipeline = Pipeline::new(
            "cleanup",
            vec![
                PipelineChannel::with_input_id(7, first, vec![]),
                PipelineChannel::with_input_id(19, other.clone(), vec![]),
            ],
            sink.clone(),
        );
        let exec = compile(&pipeline, 2).pipelinexes()[0].pipe_exec();
        let task = exec.task_group().task().clone();
        let ctx = TaskContext::from_shared_context_with_borrowed_awaiter(
            Arc::new(0usize),
            Arc::new(|| panic!("unused")),
            Arc::new(|_| panic!("progress/error must not create awaiter")),
        );
        let baseline = Arc::strong_count(&signal);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| task.run(&ctx, 0)));
        match action {
            0 => {
                assert!(result.unwrap().unwrap().is_continue());
                assert_eq!(sink.calls.load(Ordering::SeqCst), 1);
                assert_eq!(
                    Arc::strong_count(&signal),
                    baseline + 1,
                    "only persistent parked handle remains"
                );
            }
            1 => {
                assert_eq!(result.unwrap().unwrap_err(), "fatal source");
                assert_eq!(Arc::strong_count(&signal), baseline);
                assert!(task.run(&ctx, 1).unwrap().is_cancelled());
                assert!(task.run(&ctx, 0).unwrap().is_cancelled());
            }
            _ => {
                assert!(result.is_err());
                assert_eq!(Arc::strong_count(&signal), baseline);
                assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                    || task.run(&ctx, 1)
                ))
                .is_err());
            }
        }
        assert_eq!(other.calls.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn borrowed_factory_error_and_unwind_release_scratch_and_parked_handles() {
    for panic in [false, true] {
        let signal: SharedResumer = Arc::new(TestResumer::default());
        let pipeline = Pipeline::new(
            "factory-cleanup",
            vec![PipelineChannel::with_input_id(
                7,
                Arc::new(ParkedSource {
                    signal: signal.clone(),
                    calls: AtomicUsize::new(0),
                }),
                vec![],
            )],
            Arc::new(NoSink),
        );
        let exec = compile(&pipeline, 2).pipelinexes()[0].pipe_exec();
        let task = exec.task_group().task().clone();
        let baseline = Arc::strong_count(&signal);
        let ctx = TaskContext::from_shared_context_with_borrowed_awaiter(
            Arc::new(0usize),
            Arc::new(|| panic!("unused")),
            Arc::new(move |slice| {
                assert_eq!(slice.len(), 1);
                if panic {
                    panic!("intentional factory unwind");
                }
                Err("fatal factory".into())
            }),
        );
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| task.run(&ctx, 0)));
        assert_eq!(Arc::strong_count(&signal), baseline);
        if panic {
            assert!(result.is_err());
        } else {
            assert_eq!(result.unwrap().unwrap_err(), "fatal factory");
            assert!(task.run(&ctx, 1).unwrap().is_cancelled());
        }
    }
}

struct TaskFatalSource {
    signal: SharedResumer,
    fail: AtomicBool,
}
impl SourceOperator<TestTypes> for TaskFatalSource {
    fn name(&self) -> &str {
        "task-fatal"
    }
    fn source(&self, _: &TaskContext<TestTypes>, id: ThreadId) -> Result<OpOutput<i32>, String> {
        if id == 1 && self.fail.load(Ordering::SeqCst) {
            Err("other task failed".into())
        } else {
            Ok(OpOutput::Blocked(self.signal.clone()))
        }
    }
}
#[test]
fn previously_parked_task_releases_handles_when_observing_fatal_cancel() {
    let signal: SharedResumer = Arc::new(TestResumer::default());
    let source = Arc::new(TaskFatalSource {
        signal: signal.clone(),
        fail: AtomicBool::new(false),
    });
    let pipeline = Pipeline::new(
        "cross-task-cancel",
        vec![PipelineChannel::new(source.clone(), vec![])],
        Arc::new(NoSink),
    );
    let exec = compile(&pipeline, 2).pipelinexes()[0].pipe_exec();
    let task = exec.task_group().task().clone();
    let a: SharedAwaiter = Arc::new(support::RecordedAwaiter::new(vec![]));
    let ctx = TaskContext::from_shared_context_with_borrowed_awaiter(
        Arc::new(0usize),
        Arc::new(|| panic!("unused")),
        Arc::new(move |_| Ok(a.clone())),
    );
    let baseline = Arc::strong_count(&signal);
    assert!(task.run(&ctx, 0).unwrap().is_blocked());
    assert_eq!(Arc::strong_count(&signal), baseline + 1);
    source.fail.store(true, Ordering::SeqCst);
    assert_eq!(task.run(&ctx, 1).unwrap_err(), "other task failed");
    assert_eq!(
        Arc::strong_count(&signal),
        baseline + 1,
        "task 0 has not observed cancellation yet"
    );
    assert!(task.run(&ctx, 0).unwrap().is_cancelled());
    assert_eq!(Arc::strong_count(&signal), baseline);
}

struct OneBatchSource;
impl SourceOperator<TestTypes> for OneBatchSource {
    fn name(&self) -> &str {
        "one-batch"
    }
    fn source(&self, ctx: &TaskContext<TestTypes>, _: ThreadId) -> Result<OpOutput<i32>, String> {
        assert_eq!(ctx.input_id(), Some(53));
        Ok(OpOutput::Finished(Some(1)))
    }
}
struct NestedPipe {
    calls: AtomicUsize,
    index: usize,
}
impl broken_pipeline::PipeOperator<TestTypes> for NestedPipe {
    fn name(&self) -> &str {
        "nested"
    }
    fn pipe(
        &self,
        ctx: &TaskContext<TestTypes>,
        _: ThreadId,
        input: Option<i32>,
    ) -> Result<OpOutput<i32>, String> {
        assert_eq!(ctx.input_id(), Some(53));
        match self.calls.fetch_add(1, Ordering::SeqCst) {
            0 => {
                assert_eq!(input, Some(1 + self.index as i32));
                Ok(OpOutput::SourcePipeHasMore(input.unwrap() + 1))
            }
            1 => {
                assert_eq!(input, None);
                Ok(OpOutput::PipeSinkNeedsMore)
            }
            _ => panic!("duplicate nested continuation"),
        }
    }
}
struct NestedSink {
    calls: AtomicUsize,
}
impl SinkOperator<TestTypes> for NestedSink {
    fn name(&self) -> &str {
        "nested-sink"
    }
    fn sink(
        &self,
        ctx: &TaskContext<TestTypes>,
        _: ThreadId,
        input: Option<i32>,
    ) -> Result<OpOutput<i32>, String> {
        assert_eq!(ctx.input_id(), Some(53));
        assert_eq!(input, Some(4));
        assert_eq!(self.calls.fetch_add(1, Ordering::SeqCst), 0);
        Ok(OpOutput::PipeSinkNeedsMore)
    }
}
#[test]
fn cold_nested_continuation_steps_do_not_grow_executor_vectors() {
    let pipes: Vec<broken_pipeline::SharedPipeOp<TestTypes>> = (0..3)
        .map(|index| {
            Arc::new(NestedPipe {
                calls: AtomicUsize::new(0),
                index,
            }) as _
        })
        .collect();
    let pipeline = Pipeline::new(
        "cold-nested",
        vec![PipelineChannel::with_input_id(
            53,
            Arc::new(OneBatchSource),
            pipes,
        )],
        Arc::new(NestedSink {
            calls: AtomicUsize::new(0),
        }),
    );
    let exec = compile(&pipeline, 1).pipelinexes()[0].pipe_exec();
    let group = exec.task_group();
    let ctx = TaskContext::from_shared_context_with_borrowed_awaiter(
        Arc::new(0usize),
        Arc::new(|| panic!("unused")),
        Arc::new(|_| panic!("unused")),
    );
    let (cold_result, cold) = measured(|| group.task().run(&ctx, 0));
    assert!(cold_result.unwrap().is_continue());
    let native_mutex = std::sync::Mutex::new(());
    let (_, native) = measured(|| *native_mutex.lock().unwrap());
    assert_eq!(
        (cold.calls, cold.bytes),
        (native.calls, native.bytes),
        "nested cold container request: {cold:?}"
    );
    println!("cold nested first-use Task::run requests: {cold:?}");
    for expected in ["CONTINUE", "CONTINUE", "CONTINUE", "FINISHED"] {
        let (result, requests) = measured(|| group.task().run(&ctx, 0));
        assert_eq!(result.unwrap().label(), expected);
        assert_eq!(requests.calls, 0, "nested continuation grew container");
    }
}
