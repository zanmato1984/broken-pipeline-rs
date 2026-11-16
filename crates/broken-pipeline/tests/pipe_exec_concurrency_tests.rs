mod support;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use broken_pipeline::{
    compile, OpOutput, PipeOperator, Pipeline, PipelineChannel, SinkOperator, SourceOperator, Task,
    TaskContext, TaskStatus, ThreadId,
};
use support::{test_context, TestTypes};

const DOP: usize = 4;
const RENDEZVOUS: Duration = Duration::from_secs(2);

#[derive(Default)]
struct GateState {
    entries: Vec<(ThreadId, Option<usize>)>,
    active: usize,
    max_active: usize,
    released: bool,
}

#[derive(Default)]
struct Gate {
    state: Mutex<GateState>,
    changed: Condvar,
}

impl Gate {
    fn enter<T: broken_pipeline::PipelineTypes>(&self, ctx: &TaskContext<T>, task: ThreadId) {
        let mut state = self.state.lock().unwrap();
        state.entries.push((task, ctx.input_id()));
        state.active += 1;
        state.max_active = state.max_active.max(state.active);
        self.changed.notify_all();
        // A fail-safe bounds fixture failures even if the driver unwinds. The
        // driver normally releases every callback before making assertions.
        let deadline = Instant::now() + Duration::from_secs(10);
        while !state.released {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            state = self.changed.wait_timeout(state, deadline - now).unwrap().0;
        }
        state.active -= 1;
    }

    fn observe(&self, count: usize, limit: Duration) -> Vec<(ThreadId, Option<usize>)> {
        let mut state = self.state.lock().unwrap();
        let deadline = Instant::now() + limit;
        while state.entries.len() < count {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            state = self.changed.wait_timeout(state, deadline - now).unwrap().0;
        }
        state.entries.clone()
    }

    fn release(&self) {
        self.state.lock().unwrap().released = true;
        self.changed.notify_all();
    }
}

struct ProbeSource {
    gate: Option<Arc<Gate>>,
    empty: bool,
}

impl SourceOperator<TestTypes> for ProbeSource {
    fn name(&self) -> &str {
        "probe-source"
    }

    fn source(
        &self,
        ctx: &TaskContext<TestTypes>,
        task: ThreadId,
    ) -> Result<OpOutput<i32>, String> {
        if let Some(gate) = &self.gate {
            gate.enter(ctx, task);
        }
        Ok(OpOutput::Finished(
            (!self.empty).then_some(100 + task as i32),
        ))
    }
}

struct ProbePipe {
    pipe_gate: Option<Arc<Gate>>,
    drain_gate: Option<Arc<Gate>>,
}

impl PipeOperator<TestTypes> for ProbePipe {
    fn name(&self) -> &str {
        "probe-pipe"
    }

    fn pipe(
        &self,
        ctx: &TaskContext<TestTypes>,
        task: ThreadId,
        input: Option<i32>,
    ) -> Result<OpOutput<i32>, String> {
        if let Some(gate) = &self.pipe_gate {
            gate.enter(ctx, task);
        }
        Ok(OpOutput::PipeEven(input.unwrap() + 10))
    }

    fn has_drain(&self) -> bool {
        self.drain_gate.is_some()
    }

    fn drain(&self, ctx: &TaskContext<TestTypes>, task: ThreadId) -> Result<OpOutput<i32>, String> {
        self.drain_gate.as_ref().unwrap().enter(ctx, task);
        Ok(OpOutput::Finished(Some(200 + task as i32)))
    }
}

type OutputLog = Arc<Mutex<Vec<(ThreadId, Option<usize>, i32)>>>;

struct ProbeSink {
    gate: Option<Arc<Gate>>,
    outputs: OutputLog,
}

impl SinkOperator<TestTypes> for ProbeSink {
    fn name(&self) -> &str {
        "probe-sink"
    }

    fn sink(
        &self,
        ctx: &TaskContext<TestTypes>,
        task: ThreadId,
        input: Option<i32>,
    ) -> Result<OpOutput<i32>, String> {
        if let Some(gate) = &self.gate {
            gate.enter(ctx, task);
        }
        self.outputs
            .lock()
            .unwrap()
            .push((task, ctx.input_id(), input.unwrap()));
        Ok(OpOutput::PipeSinkNeedsMore)
    }
}

fn complete(task: Task<TestTypes>, id: usize) -> Result<TaskStatus, String> {
    for _ in 0..100 {
        let status = task.run(&test_context(), id)?;
        if status.is_finished() || status.is_cancelled() {
            return Ok(status);
        }
    }
    Err("fixture exceeded bounded task steps".into())
}

fn overlap(phase: &str) {
    let gate = Arc::new(Gate::default());
    let outputs = OutputLog::default();
    let source = Arc::new(ProbeSource {
        gate: (phase == "source").then(|| gate.clone()),
        empty: phase == "drain",
    });
    let pipe = Arc::new(ProbePipe {
        pipe_gate: (phase == "pipe").then(|| gate.clone()),
        drain_gate: (phase == "drain").then(|| gate.clone()),
    });
    let sink = Arc::new(ProbeSink {
        gate: (phase == "sink").then(|| gate.clone()),
        outputs: outputs.clone(),
    });
    let pipeline = Pipeline::new(
        "real-overlap",
        vec![PipelineChannel::with_input_id(37, source, vec![pipe])],
        sink,
    );
    let compiled = compile(&pipeline, DOP);
    let exec = compiled.pipelinexes()[0].pipe_exec();
    let group = exec.task_group();
    let workers: Vec<_> = (0..group.num_tasks())
        .map(|id| {
            let task = group.task().clone();
            thread::spawn(move || complete(task, id))
        })
        .collect();
    let mut observed = gate.observe(DOP, RENDEZVOUS);
    gate.release();
    let results: Vec<_> = workers.into_iter().map(|w| w.join()).collect();
    // Release and join even on the exact serialized old executor, before any
    // assertion. Its failure is missing overlap, not a hung barrier/process.
    for result in results {
        assert!(result.unwrap().unwrap().is_finished());
    }
    observed.sort_unstable();
    assert_eq!(
        observed,
        (0..DOP).map(|id| (id, Some(37))).collect::<Vec<_>>(),
        "missing {phase} callback overlap"
    );
    let mut actual = outputs.lock().unwrap().clone();
    actual.sort_unstable();
    let base = if phase == "drain" { 200 } else { 110 };
    assert_eq!(
        actual,
        (0..DOP)
            .map(|id| (id, Some(37), base + id as i32))
            .collect::<Vec<_>>()
    );
}

#[test]
fn source_callbacks_overlap_on_compiled_tasks() {
    overlap("source");
}

#[test]
fn pipe_callbacks_overlap_on_compiled_tasks() {
    overlap("pipe");
}

#[test]
fn sink_callbacks_overlap_on_compiled_tasks() {
    overlap("sink");
}

#[test]
fn drain_callbacks_overlap_on_compiled_tasks() {
    overlap("drain");
}

struct CountingSource {
    gate: Arc<Gate>,
    calls: AtomicUsize,
}

impl SourceOperator<TestTypes> for CountingSource {
    fn name(&self) -> &str {
        "counting"
    }

    fn source(
        &self,
        ctx: &TaskContext<TestTypes>,
        task: ThreadId,
    ) -> Result<OpOutput<i32>, String> {
        self.gate.enter(ctx, task);
        let ordinal = self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(if ordinal == 0 {
            OpOutput::SourcePipeHasMore(0)
        } else {
            OpOutput::Finished(Some(1))
        })
    }
}

#[test]
fn simultaneous_same_task_entries_are_exclusive_and_consume_once() {
    let gate = Arc::new(Gate::default());
    let outputs = OutputLog::default();
    let source = Arc::new(CountingSource {
        gate: gate.clone(),
        calls: AtomicUsize::new(0),
    });
    let pipeline = Pipeline::new(
        "same-task",
        vec![PipelineChannel::with_input_id(37, source.clone(), vec![])],
        Arc::new(ProbeSink {
            gate: None,
            outputs: outputs.clone(),
        }),
    );
    let compiled = compile(&pipeline, 1);
    let exec = compiled.pipelinexes()[0].pipe_exec();
    let task = exec.task_group().task().clone();
    let first_task = task.clone();
    let first = thread::spawn(move || first_task.run(&test_context(), 0));
    let first_observed = gate.observe(1, RENDEZVOUS);
    let second_task = task.clone();
    let (started_tx, started_rx) = mpsc::channel();
    let second = thread::spawn(move || {
        started_tx.send(()).unwrap();
        second_task.run(&test_context(), 0)
    });
    let started = started_rx.recv_timeout(RENDEZVOUS);
    let held_entries = gate.observe(2, Duration::from_millis(200));
    gate.release();
    let results = [first.join(), second.join()];
    assert!(started.is_ok());
    assert_eq!(first_observed, vec![(0, Some(37))]);
    assert_eq!(
        held_entries.len(),
        1,
        "same logical task entered two callbacks"
    );
    for result in results {
        assert!(result.unwrap().unwrap().is_continue());
    }
    assert!(task.run(&test_context(), 0).unwrap().is_finished());
    assert!(task.run(&test_context(), 0).unwrap().is_finished());
    assert_eq!(source.calls.load(Ordering::SeqCst), 2);
    assert_eq!(gate.state.lock().unwrap().max_active, 1);
    assert_eq!(
        *outputs.lock().unwrap(),
        vec![(0, Some(37), 0), (0, Some(37), 1)]
    );
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Call {
    method: &'static str,
    task: ThreadId,
    input_id: usize,
    input: Option<i32>,
}

type Calls = Arc<Mutex<Vec<Call>>>;
type Positions = Mutex<BTreeMap<(ThreadId, usize), usize>>;

fn record(
    calls: &Calls,
    method: &'static str,
    ctx: &TaskContext<TestTypes>,
    task: ThreadId,
    input: Option<i32>,
) {
    calls.lock().unwrap().push(Call {
        method,
        task,
        input_id: ctx.input_id().unwrap(),
        input,
    });
}

fn advance(positions: &Positions, ctx: &TaskContext<TestTypes>, task: ThreadId) -> usize {
    let mut positions = positions.lock().unwrap();
    let position = positions
        .entry((task, ctx.input_id().unwrap()))
        .or_default();
    let old = *position;
    *position += 1;
    old
}

fn ready(ctx: &TaskContext<TestTypes>) -> OpOutput<i32> {
    let resumer = ctx.make_resumer().unwrap();
    resumer.resume();
    OpOutput::Blocked(resumer)
}

struct MultiSource {
    calls: Calls,
}

impl SourceOperator<TestTypes> for MultiSource {
    fn name(&self) -> &str {
        "multi-source"
    }
    fn source(
        &self,
        ctx: &TaskContext<TestTypes>,
        task: ThreadId,
    ) -> Result<OpOutput<i32>, String> {
        record(&self.calls, "source", ctx, task, None);
        Ok(OpOutput::Finished(Some(
            (ctx.input_id().unwrap() * 100 + task) as i32,
        )))
    }
}

struct MultiPipe {
    calls: Calls,
    pipe_positions: Positions,
    drain_positions: Positions,
}

impl PipeOperator<TestTypes> for MultiPipe {
    fn name(&self) -> &str {
        "multi-pipe"
    }
    fn pipe(
        &self,
        ctx: &TaskContext<TestTypes>,
        task: ThreadId,
        input: Option<i32>,
    ) -> Result<OpOutput<i32>, String> {
        record(&self.calls, "pipe", ctx, task, input);
        let value = (ctx.input_id().unwrap() * 100 + task) as i32;
        Ok(match advance(&self.pipe_positions, ctx, task) {
            0 => {
                assert_eq!(input, Some(value));
                OpOutput::SourcePipeHasMore(value + 10)
            }
            1 => {
                assert_eq!(input, None);
                ready(ctx)
            }
            2 => {
                assert_eq!(input, None);
                OpOutput::PipeYield
            }
            3 => {
                assert_eq!(input, None);
                OpOutput::PipeYieldBack
            }
            4 => {
                assert_eq!(input, None);
                OpOutput::PipeEven(value + 20)
            }
            _ => panic!("duplicate pipe consumption"),
        })
    }
    fn has_drain(&self) -> bool {
        true
    }
    fn drain(&self, ctx: &TaskContext<TestTypes>, task: ThreadId) -> Result<OpOutput<i32>, String> {
        record(&self.calls, "drain", ctx, task, None);
        let value = (ctx.input_id().unwrap() * 100 + task) as i32;
        Ok(match advance(&self.drain_positions, ctx, task) {
            0 => OpOutput::PipeYield,
            1 => OpOutput::PipeYieldBack,
            2 => ready(ctx),
            3 => OpOutput::SourcePipeHasMore(value + 30),
            4 => OpOutput::Finished(Some(value + 40)),
            _ => panic!("duplicate drain consumption"),
        })
    }
}

struct MultiSink {
    calls: Calls,
    outputs: OutputLog,
}

impl SinkOperator<TestTypes> for MultiSink {
    fn name(&self) -> &str {
        "multi-sink"
    }
    fn sink(
        &self,
        ctx: &TaskContext<TestTypes>,
        task: ThreadId,
        input: Option<i32>,
    ) -> Result<OpOutput<i32>, String> {
        record(&self.calls, "sink", ctx, task, input);
        if let Some(value) = input {
            self.outputs
                .lock()
                .unwrap()
                .push((task, ctx.input_id(), value));
            Ok(ready(ctx))
        } else {
            Ok(OpOutput::PipeSinkNeedsMore)
        }
    }
}

#[test]
fn task_and_input_identity_survive_multichannel_continuations_and_thread_migration() {
    let calls = Calls::default();
    let outputs = OutputLog::default();
    let pipe = Arc::new(MultiPipe {
        calls: calls.clone(),
        pipe_positions: Positions::default(),
        drain_positions: Positions::default(),
    });
    // Distinct source identities ensure compile retains both channels.
    let channels = [7, 19]
        .into_iter()
        .map(|input| {
            PipelineChannel::with_input_id(
                input,
                Arc::new(MultiSource {
                    calls: calls.clone(),
                }),
                vec![pipe.clone() as _],
            )
        })
        .collect();
    let pipeline = Pipeline::new(
        "multi",
        channels,
        Arc::new(MultiSink {
            calls: calls.clone(),
            outputs: outputs.clone(),
        }),
    );
    let compiled = compile(&pipeline, 2);
    let exec = compiled.pipelinexes()[0].pipe_exec();
    let task = exec.task_group().task().clone();
    // Each quantum migrates to a fresh OS thread, preserving logical slots.
    for id in [1, 0] {
        let mut finished = false;
        for _ in 0..100 {
            let task = task.clone();
            let status = thread::spawn(move || task.run(&test_context(), id))
                .join()
                .unwrap()
                .unwrap();
            if status.is_finished() {
                finished = true;
                break;
            }
            assert!(!status.is_cancelled());
        }
        assert!(finished);
    }
    let calls = calls.lock().unwrap();
    for task in 0..2 {
        for input_id in [7, 19] {
            let value = (input_id * 100 + task) as i32;
            let actual: Vec<_> = calls
                .iter()
                .filter(|c| c.task == task && c.input_id == input_id)
                .map(|c| (c.method, c.input))
                .collect();
            assert_eq!(
                actual,
                vec![
                    ("source", None),
                    ("pipe", Some(value)),
                    ("sink", Some(value + 10)),
                    ("sink", None),
                    ("pipe", None),
                    ("pipe", None),
                    ("pipe", None),
                    ("pipe", None),
                    ("sink", Some(value + 20)),
                    ("sink", None),
                    ("drain", None),
                    ("drain", None),
                    ("drain", None),
                    ("drain", None),
                    ("sink", Some(value + 30)),
                    ("sink", None),
                    ("drain", None),
                    ("sink", Some(value + 40)),
                    ("sink", None),
                ]
            );
        }
    }
    let mut actual = outputs.lock().unwrap().clone();
    actual.sort_unstable();
    let mut expected = Vec::new();
    for task in 0..2 {
        for input in [7, 19] {
            for offset in [10, 20, 30, 40] {
                expected.push((task, Some(input), (input * 100 + task) as i32 + offset));
            }
        }
    }
    expected.sort_unstable();
    assert_eq!(actual, expected);
}

struct OwnedTypes;

impl broken_pipeline::PipelineTypes for OwnedTypes {
    type Batch = OwnedBatch;
    type Error = String;
    type Context = ();
}

struct OwnedBatch {
    drops: Arc<AtomicUsize>,
}

impl Drop for OwnedBatch {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

fn owned_context() -> TaskContext<OwnedTypes> {
    TaskContext::new(
        (),
        Arc::new(
            || Ok(Arc::new(support::TestResumer::default()) as broken_pipeline::SharedResumer),
        ),
        Arc::new(|r| {
            Ok(Arc::new(support::RecordedAwaiter::new(r)) as broken_pipeline::SharedAwaiter)
        }),
    )
}

struct FatalSource {
    gate: Arc<Gate>,
    drops: Arc<AtomicUsize>,
    calls: [AtomicUsize; 2],
}

impl SourceOperator<OwnedTypes> for FatalSource {
    fn name(&self) -> &str {
        "fatal-source"
    }
    fn source(
        &self,
        ctx: &TaskContext<OwnedTypes>,
        task: ThreadId,
    ) -> Result<OpOutput<OwnedBatch>, String> {
        self.calls[task].fetch_add(1, Ordering::SeqCst);
        if task == 1 {
            return Err("fatal in task 1".into());
        }
        self.gate.enter(ctx, task);
        Ok(OpOutput::Finished(Some(OwnedBatch {
            drops: self.drops.clone(),
        })))
    }
}

struct OwnedSink {
    publications: Arc<AtomicUsize>,
}

impl SinkOperator<OwnedTypes> for OwnedSink {
    fn name(&self) -> &str {
        "owned-sink"
    }
    fn sink(
        &self,
        _ctx: &TaskContext<OwnedTypes>,
        _task: ThreadId,
        _input: Option<OwnedBatch>,
    ) -> Result<OpOutput<OwnedBatch>, String> {
        self.publications.fetch_add(1, Ordering::SeqCst);
        Ok(OpOutput::PipeSinkNeedsMore)
    }
}

#[test]
fn fatal_error_cancels_other_tasks_and_drops_inflight_output_once() {
    let gate = Arc::new(Gate::default());
    let drops = Arc::new(AtomicUsize::new(0));
    let publications = Arc::new(AtomicUsize::new(0));
    let source = Arc::new(FatalSource {
        gate: gate.clone(),
        drops: drops.clone(),
        calls: [AtomicUsize::new(0), AtomicUsize::new(0)],
    });
    let pipeline = Pipeline::new(
        "fatal",
        vec![PipelineChannel::with_input_id(41, source.clone(), vec![])],
        Arc::new(OwnedSink {
            publications: publications.clone(),
        }),
    );
    let compiled = compile(&pipeline, 2);
    let exec = compiled.pipelinexes()[0].pipe_exec();
    let task = exec.task_group().task().clone();
    let running_task = task.clone();
    let running = thread::spawn(move || running_task.run(&owned_context(), 0));
    let observed = gate.observe(1, RENDEZVOUS);
    let failing_task = task.clone();
    let (failed_tx, failed_rx) = mpsc::channel();
    let failing = thread::spawn(move || {
        let result = failing_task.run(&owned_context(), 1);
        failed_tx.send(result.as_ref().err().cloned()).unwrap();
        result
    });
    let failure_while_inflight = failed_rx.recv_timeout(RENDEZVOUS);
    gate.release();
    let running_result = running.join();
    let failing_result = failing.join();
    assert_eq!(observed, vec![(0, Some(41))]);
    assert_eq!(
        failure_while_inflight.unwrap(),
        Some("fatal in task 1".into())
    );
    assert_eq!(failing_result.unwrap().unwrap_err(), "fatal in task 1");
    assert!(running_result.unwrap().unwrap().is_cancelled());
    for id in [0, 1, 0, 1] {
        assert!(task.run(&owned_context(), id).unwrap().is_cancelled());
    }
    assert_eq!(source.calls[0].load(Ordering::SeqCst), 1);
    assert_eq!(source.calls[1].load(Ordering::SeqCst), 1);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(publications.load(Ordering::SeqCst), 0);
    drop(task);
    drop(exec);
    assert_eq!(
        drops.load(Ordering::SeqCst),
        1,
        "no duplicate ownership on executor drop"
    );
}

struct PanicSource {
    calls: AtomicUsize,
}

impl SourceOperator<TestTypes> for PanicSource {
    fn name(&self) -> &str {
        "panic-source"
    }
    fn source(
        &self,
        _ctx: &TaskContext<TestTypes>,
        _task: ThreadId,
    ) -> Result<OpOutput<i32>, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        panic!("intentional callback panic")
    }
}

#[test]
fn callback_panic_poison_is_not_recovered_by_any_task() {
    let source = Arc::new(PanicSource {
        calls: AtomicUsize::new(0),
    });
    let pipeline = Pipeline::new(
        "panic",
        vec![PipelineChannel::new(source.clone(), vec![])],
        Arc::new(ProbeSink {
            gate: None,
            outputs: OutputLog::default(),
        }),
    );
    let compiled = compile(&pipeline, 2);
    let exec = compiled.pipelinexes()[0].pipe_exec();
    let task = exec.task_group().task().clone();
    let panicking = task.clone();
    assert!(thread::spawn(move || panicking.run(&test_context(), 0))
        .join()
        .is_err());
    let other_task = task.clone();
    assert!(thread::spawn(move || other_task.run(&test_context(), 1))
        .join()
        .is_err());
    let poisoned = task.clone();
    assert!(thread::spawn(move || poisoned.run(&test_context(), 0))
        .join()
        .is_err());
    assert_eq!(source.calls.load(Ordering::SeqCst), 1);
}

struct BlockedSource {
    calls: AtomicUsize,
}

impl SourceOperator<TestTypes> for BlockedSource {
    fn name(&self) -> &str {
        "blocked-source"
    }
    fn source(
        &self,
        ctx: &TaskContext<TestTypes>,
        _task: ThreadId,
    ) -> Result<OpOutput<i32>, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(OpOutput::Blocked(ctx.make_resumer()?))
    }
}

#[test]
fn fatal_awaiter_factory_error_cancels_subsequent_task_entries() {
    let source = Arc::new(BlockedSource {
        calls: AtomicUsize::new(0),
    });
    let pipeline = Pipeline::new(
        "awaiter-error",
        vec![PipelineChannel::new(source.clone(), vec![])],
        Arc::new(ProbeSink {
            gate: None,
            outputs: OutputLog::default(),
        }),
    );
    let compiled = compile(&pipeline, 2);
    let exec = compiled.pipelinexes()[0].pipe_exec();
    let task = exec.task_group().task().clone();
    let ctx = TaskContext::new(
        0usize,
        Arc::new(
            || Ok(Arc::new(support::TestResumer::default()) as broken_pipeline::SharedResumer),
        ),
        Arc::new(|_| Err("fatal awaiter factory".into())),
    );
    assert_eq!(task.run(&ctx, 0).unwrap_err(), "fatal awaiter factory");
    assert!(task.run(&ctx, 1).unwrap().is_cancelled());
    assert!(task.run(&ctx, 0).unwrap().is_cancelled());
    assert_eq!(source.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn borrowed_context_callbacks_overlap_at_dop_one_two_four() {
    for dop in [1, 2, 4] {
        for phase in ["source", "pipe", "sink", "drain"] {
            let gate = Arc::new(Gate::default());
            let outputs = OutputLog::default();
            let pipeline = Pipeline::new(
                "borrowed-overlap",
                vec![PipelineChannel::with_input_id(
                    37,
                    Arc::new(ProbeSource {
                        gate: (phase == "source").then(|| gate.clone()),
                        empty: phase == "drain",
                    }),
                    vec![Arc::new(ProbePipe {
                        pipe_gate: (phase == "pipe").then(|| gate.clone()),
                        drain_gate: (phase == "drain").then(|| gate.clone()),
                    })],
                )],
                Arc::new(ProbeSink {
                    gate: (phase == "sink").then(|| gate.clone()),
                    outputs: outputs.clone(),
                }),
            );
            let exec = compile(&pipeline, dop).pipelinexes()[0].pipe_exec();
            let group = exec.task_group();
            let workers: Vec<_> = (0..group.num_tasks())
                .map(|id| {
                    let task = group.task().clone();
                    thread::spawn(move || {
                        let a: broken_pipeline::SharedAwaiter =
                            Arc::new(support::RecordedAwaiter::new(vec![]));
                        let ctx = TaskContext::from_shared_context_with_borrowed_awaiter(
                            Arc::new(0usize),
                            Arc::new(|| panic!("no blocked callbacks")),
                            Arc::new(move |_| Ok(a.clone())),
                        );
                        for _ in 0..10 {
                            if task.run(&ctx, id).unwrap().is_finished() {
                                return true;
                            }
                        }
                        false
                    })
                })
                .collect();
            let mut observed = gate.observe(dop, RENDEZVOUS);
            gate.release();
            let results: Vec<_> = workers.into_iter().map(|w| w.join()).collect();
            for result in results {
                assert!(result.unwrap());
            }
            observed.sort_unstable();
            assert_eq!(
                observed,
                (0..dop).map(|id| (id, Some(37))).collect::<Vec<_>>(),
                "{phase} DOP {dop}"
            );
            assert_eq!(gate.state.lock().unwrap().max_active, dop);
            let mut actual = outputs.lock().unwrap().clone();
            actual.sort_unstable();
            let base = if phase == "drain" { 200 } else { 110 };
            assert_eq!(
                actual,
                (0..dop)
                    .map(|id| (id, Some(37), base + id as i32))
                    .collect::<Vec<_>>()
            );
        }
    }
}
