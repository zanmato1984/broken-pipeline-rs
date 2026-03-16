#![allow(dead_code)]

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use broken_pipeline::{
    Awaiter, OpOutput, PipeOperator, PipelineTypes, Resumer, SharedAwaiter, SharedResumer,
    SinkOperator, SourceOperator, TaskContext, TaskGroup, ThreadId,
};

pub struct TestTypes;

impl PipelineTypes for TestTypes {
    type Batch = i32;
    type Error = String;
}

#[derive(Default)]
pub struct TestResumer {
    resumed: AtomicBool,
}

impl Resumer for TestResumer {
    fn resume(&self) {
        self.resumed.store(true, Ordering::SeqCst);
    }

    fn is_resumed(&self) -> bool {
        self.resumed.load(Ordering::SeqCst)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub struct RecordedAwaiter {
    resumers: Vec<SharedResumer>,
}

impl RecordedAwaiter {
    pub fn new(resumers: Vec<SharedResumer>) -> Self {
        Self { resumers }
    }

    pub fn resumers(&self) -> &[SharedResumer] {
        &self.resumers
    }
}

impl Awaiter for RecordedAwaiter {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub fn test_context() -> TaskContext<TestTypes> {
    TaskContext::without_context(
        Arc::new(|| Ok(Arc::new(TestResumer::default()) as SharedResumer)),
        Arc::new(|resumers| Ok(Arc::new(RecordedAwaiter::new(resumers)) as SharedAwaiter)),
    )
}

pub fn awaiter_resumers(awaiter: &SharedAwaiter) -> Vec<SharedResumer> {
    awaiter
        .as_any()
        .downcast_ref::<RecordedAwaiter>()
        .expect("expected RecordedAwaiter")
        .resumers()
        .to_vec()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Trace {
    pub op: String,
    pub method: &'static str,
    pub input: Option<Option<i32>>,
    pub output: String,
}

pub type TraceLog = Arc<Mutex<Vec<Trace>>>;

pub fn trace_log() -> TraceLog {
    Arc::new(Mutex::new(Vec::new()))
}

pub fn traces(log: &TraceLog) -> Vec<Trace> {
    log.lock().expect("trace log mutex poisoned").clone()
}

pub fn trace(
    op: impl Into<String>,
    method: &'static str,
    input: Option<Option<i32>>,
    output: impl Into<String>,
) -> Trace {
    Trace {
        op: op.into(),
        method,
        input,
        output: output.into(),
    }
}

#[derive(Clone)]
pub enum ScriptOutput {
    PipeSinkNeedsMore,
    PipeEven(i32),
    SourcePipeHasMore(i32),
    PipeYield,
    PipeYieldBack,
    Finished(Option<i32>),
    Cancelled,
}

impl ScriptOutput {
    fn label(&self) -> &'static str {
        match self {
            Self::PipeSinkNeedsMore => "PIPE_SINK_NEEDS_MORE",
            Self::PipeEven(_) => "PIPE_EVEN",
            Self::SourcePipeHasMore(_) => "SOURCE_PIPE_HAS_MORE",
            Self::PipeYield => "PIPE_YIELD",
            Self::PipeYieldBack => "PIPE_YIELD_BACK",
            Self::Finished(_) => "FINISHED",
            Self::Cancelled => "CANCELLED",
        }
    }

    fn into_output(self) -> OpOutput<i32> {
        match self {
            Self::PipeSinkNeedsMore => OpOutput::PipeSinkNeedsMore,
            Self::PipeEven(batch) => OpOutput::PipeEven(batch),
            Self::SourcePipeHasMore(batch) => OpOutput::SourcePipeHasMore(batch),
            Self::PipeYield => OpOutput::PipeYield,
            Self::PipeYieldBack => OpOutput::PipeYieldBack,
            Self::Finished(batch) => OpOutput::Finished(batch),
            Self::Cancelled => OpOutput::Cancelled,
        }
    }
}

#[derive(Clone)]
pub enum Step {
    Output {
        expected_input: Option<Option<i32>>,
        output: ScriptOutput,
    },
    Blocked {
        expected_input: Option<Option<i32>>,
    },
    Error {
        expected_input: Option<Option<i32>>,
        message: &'static str,
    },
}

impl Step {
    pub fn output(output: ScriptOutput) -> Self {
        Self::Output {
            expected_input: None,
            output,
        }
    }

    pub fn blocked() -> Self {
        Self::Blocked {
            expected_input: None,
        }
    }

    pub fn error(message: &'static str) -> Self {
        Self::Error {
            expected_input: None,
            message,
        }
    }

    pub fn with_expected_input(self, input: Option<i32>) -> Self {
        self.with_expected_raw(input)
    }

    pub fn with_expected_null_input(self) -> Self {
        self.with_expected_raw(None)
    }

    fn with_expected_raw(self, input: Option<i32>) -> Self {
        match self {
            Self::Output { output, .. } => Self::Output {
                expected_input: Some(input),
                output,
            },
            Self::Blocked { .. } => Self::Blocked {
                expected_input: Some(input),
            },
            Self::Error { message, .. } => Self::Error {
                expected_input: Some(input),
                message,
            },
        }
    }
}

pub struct ScriptedSource {
    name: String,
    steps: Mutex<Vec<VecDeque<Step>>>,
    traces: TraceLog,
}

impl ScriptedSource {
    pub fn new(name: impl Into<String>, steps: Vec<Vec<Step>>, traces: TraceLog) -> Self {
        Self {
            name: name.into(),
            steps: Mutex::new(steps.into_iter().map(VecDeque::from).collect()),
            traces,
        }
    }
}

impl SourceOperator<TestTypes> for ScriptedSource {
    fn name(&self) -> &str {
        &self.name
    }

    fn source(
        &self,
        ctx: &TaskContext<TestTypes>,
        thread_id: ThreadId,
    ) -> Result<OpOutput<i32>, String> {
        let step = pop_step(&self.steps, thread_id, &self.name, "Source");
        execute_step(&self.name, "Source", None, step, ctx, &self.traces)
    }
}

pub struct ScriptedPipe {
    name: String,
    pipe_steps: Mutex<Vec<VecDeque<Step>>>,
    drain_steps: Mutex<Vec<VecDeque<Step>>>,
    has_drain: bool,
    implicit_source: Option<Arc<dyn SourceOperator<TestTypes>>>,
    traces: TraceLog,
}

impl ScriptedPipe {
    pub fn new(
        name: impl Into<String>,
        pipe_steps: Vec<Vec<Step>>,
        drain_steps: Vec<Vec<Step>>,
        implicit_source: Option<Arc<dyn SourceOperator<TestTypes>>>,
        traces: TraceLog,
    ) -> Self {
        let has_drain = !drain_steps.is_empty();
        Self {
            name: name.into(),
            pipe_steps: Mutex::new(pipe_steps.into_iter().map(VecDeque::from).collect()),
            drain_steps: Mutex::new(drain_steps.into_iter().map(VecDeque::from).collect()),
            has_drain,
            implicit_source,
            traces,
        }
    }
}

impl PipeOperator<TestTypes> for ScriptedPipe {
    fn name(&self) -> &str {
        &self.name
    }

    fn pipe(
        &self,
        ctx: &TaskContext<TestTypes>,
        thread_id: ThreadId,
        input: Option<i32>,
    ) -> Result<OpOutput<i32>, String> {
        let step = pop_step(&self.pipe_steps, thread_id, &self.name, "Pipe");
        execute_step(&self.name, "Pipe", Some(input), step, ctx, &self.traces)
    }

    fn has_drain(&self) -> bool {
        self.has_drain
    }

    fn drain(
        &self,
        ctx: &TaskContext<TestTypes>,
        thread_id: ThreadId,
    ) -> Result<OpOutput<i32>, String> {
        let step = pop_step(&self.drain_steps, thread_id, &self.name, "Drain");
        execute_step(&self.name, "Drain", None, step, ctx, &self.traces)
    }

    fn implicit_source(&self) -> Option<Arc<dyn SourceOperator<TestTypes>>> {
        self.implicit_source.as_ref().cloned()
    }
}

pub struct ScriptedSink {
    name: String,
    steps: Mutex<Vec<VecDeque<Step>>>,
    traces: TraceLog,
}

impl ScriptedSink {
    pub fn new(name: impl Into<String>, steps: Vec<Vec<Step>>, traces: TraceLog) -> Self {
        Self {
            name: name.into(),
            steps: Mutex::new(steps.into_iter().map(VecDeque::from).collect()),
            traces,
        }
    }
}

impl SinkOperator<TestTypes> for ScriptedSink {
    fn name(&self) -> &str {
        &self.name
    }

    fn sink(
        &self,
        ctx: &TaskContext<TestTypes>,
        thread_id: ThreadId,
        input: Option<i32>,
    ) -> Result<OpOutput<i32>, String> {
        let step = pop_step(&self.steps, thread_id, &self.name, "Sink");
        execute_step(&self.name, "Sink", Some(input), step, ctx, &self.traces)
    }
}

fn pop_step(
    steps: &Mutex<Vec<VecDeque<Step>>>,
    thread_id: ThreadId,
    op: &str,
    method: &str,
) -> Step {
    let mut steps = steps.lock().expect("script step mutex poisoned");
    steps
        .get_mut(thread_id)
        .and_then(VecDeque::pop_front)
        .unwrap_or_else(|| panic!("missing scripted step for {op}::{method} on lane {thread_id}"))
}

fn execute_step(
    op: &str,
    method: &'static str,
    input: Option<Option<i32>>,
    step: Step,
    ctx: &TaskContext<TestTypes>,
    traces: &TraceLog,
) -> Result<OpOutput<i32>, String> {
    match step {
        Step::Output {
            expected_input,
            output,
        } => {
            if let Some(expected_input) = expected_input {
                assert_eq!(
                    input,
                    Some(expected_input),
                    "unexpected {op}::{method} input"
                );
            }
            record_trace(traces, op, method, input, output.label());
            Ok(output.into_output())
        }
        Step::Blocked { expected_input } => {
            if let Some(expected_input) = expected_input {
                assert_eq!(
                    input,
                    Some(expected_input),
                    "unexpected {op}::{method} input"
                );
            }
            record_trace(traces, op, method, input, "BLOCKED");
            Ok(OpOutput::Blocked(ctx.make_resumer()?))
        }
        Step::Error {
            expected_input,
            message,
        } => {
            if let Some(expected_input) = expected_input {
                assert_eq!(
                    input,
                    Some(expected_input),
                    "unexpected {op}::{method} input"
                );
            }
            record_trace(traces, op, method, input, format!("ERROR({message})"));
            Err(message.to_string())
        }
    }
}

fn record_trace(
    traces: &TraceLog,
    op: &str,
    method: &'static str,
    input: Option<Option<i32>>,
    output: impl Into<String>,
) {
    traces
        .lock()
        .expect("trace log mutex poisoned")
        .push(Trace {
            op: op.to_string(),
            method,
            input,
            output: output.into(),
        });
}

pub fn shared_source(
    source: impl SourceOperator<TestTypes> + 'static,
) -> Arc<dyn SourceOperator<TestTypes>> {
    Arc::new(source)
}

pub fn shared_pipe(
    pipe: impl PipeOperator<TestTypes> + 'static,
) -> Arc<dyn PipeOperator<TestTypes>> {
    Arc::new(pipe)
}

pub fn shared_sink(
    sink: impl SinkOperator<TestTypes> + 'static,
) -> Arc<dyn SinkOperator<TestTypes>> {
    Arc::new(sink)
}

pub fn empty_task_groups() -> Vec<TaskGroup<TestTypes>> {
    Vec::new()
}
