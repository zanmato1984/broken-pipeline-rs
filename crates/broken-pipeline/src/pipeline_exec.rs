use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::operator::{OpOutput, SharedPipeOp, SharedSinkOp, SharedSourceOp};
use crate::pipeline::PipelineChannel;
use crate::task::{SharedResumer, Task, TaskContext, TaskGroup, TaskStatus};
use crate::types::{BpResult, PipelineTypes, TaskId, ThreadId};

#[derive(Clone)]
pub struct SourceExec<T: PipelineTypes> {
    pub frontend: Vec<TaskGroup<T>>,
    pub backend: Option<TaskGroup<T>>,
}

#[derive(Clone)]
pub struct SinkExec<T: PipelineTypes> {
    pub frontend: Vec<TaskGroup<T>>,
    pub backend: Option<TaskGroup<T>>,
}

pub struct Pipelinexe<T: PipelineTypes> {
    name: String,
    channels: Vec<PipelineChannel<T>>,
    implicit_sources_keepalive: Vec<SharedSourceOp<T>>,
    sink: SharedSinkOp<T>,
    dop: usize,
}

impl<T: PipelineTypes> Clone for Pipelinexe<T> {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            channels: self.channels.clone(),
            implicit_sources_keepalive: self.implicit_sources_keepalive.to_vec(),
            sink: Arc::clone(&self.sink),
            dop: self.dop,
        }
    }
}

impl<T: PipelineTypes> Pipelinexe<T> {
    pub(crate) fn new(
        name: String,
        channels: Vec<PipelineChannel<T>>,
        implicit_sources_keepalive: Vec<SharedSourceOp<T>>,
        sink: SharedSinkOp<T>,
        dop: usize,
    ) -> Self {
        Self {
            name,
            channels,
            implicit_sources_keepalive,
            sink,
            dop,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn dop(&self) -> usize {
        self.dop
    }

    pub fn channels(&self) -> &[PipelineChannel<T>] {
        &self.channels
    }

    pub fn num_implicit_sources(&self) -> usize {
        self.implicit_sources_keepalive.len()
    }

    pub fn source_execs(&self) -> Vec<SourceExec<T>> {
        self.channels
            .iter()
            .map(|channel| SourceExec {
                frontend: channel.source().frontend(),
                backend: channel.source().backend(),
            })
            .collect()
    }

    pub fn pipe_exec(&self) -> PipeExec<T> {
        PipeExec::new(self.channels.clone(), Arc::clone(&self.sink), self.dop)
    }
}

#[derive(Clone)]
pub struct PipelineExec<T: PipelineTypes> {
    name: String,
    sink: SinkExec<T>,
    pipelinexes: Vec<Pipelinexe<T>>,
    dop: usize,
}

impl<T: PipelineTypes> PipelineExec<T> {
    pub(crate) fn new(
        name: String,
        sink: SinkExec<T>,
        pipelinexes: Vec<Pipelinexe<T>>,
        dop: usize,
    ) -> Self {
        Self {
            name,
            sink,
            pipelinexes,
            dop,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn dop(&self) -> usize {
        self.dop
    }

    pub fn sink(&self) -> &SinkExec<T> {
        &self.sink
    }

    pub fn pipelinexes(&self) -> &[Pipelinexe<T>] {
        &self.pipelinexes
    }
}

pub struct PipeExec<T: PipelineTypes> {
    dop: usize,
    exec: Arc<ExecState<T>>,
}

impl<T: PipelineTypes> Clone for PipeExec<T> {
    fn clone(&self) -> Self {
        Self {
            dop: self.dop,
            exec: Arc::clone(&self.exec),
        }
    }
}

impl<T: PipelineTypes> PipeExec<T> {
    pub(crate) fn new(
        channels: Vec<PipelineChannel<T>>,
        sink: SharedSinkOp<T>,
        dop: usize,
    ) -> Self {
        Self {
            dop,
            exec: Arc::new(ExecState::new(channels, sink, dop)),
        }
    }

    pub fn dop(&self) -> usize {
        self.dop
    }

    pub fn task_group(&self) -> TaskGroup<T>
    where
        T: 'static,
    {
        let exec = self.clone();
        let task = Task::new(String::new(), move |ctx, task_id| exec.step(ctx, task_id));
        TaskGroup::new(String::new(), task, self.dop)
    }

    pub fn step(&self, ctx: &TaskContext<T>, task_id: TaskId) -> BpResult<TaskStatus, T> {
        self.exec.step(ctx, task_id)
    }
}

struct ExecState<T: PipelineTypes> {
    channels: Vec<ChannelRuntime<T>>,
    task_locals: Vec<Mutex<ExecTaskLocal>>,
    cancelled: AtomicBool,
    poisoned: AtomicBool,
}

impl<T: PipelineTypes> ExecState<T> {
    fn new(channels: Vec<PipelineChannel<T>>, sink: SharedSinkOp<T>, dop: usize) -> Self {
        let channel_runtimes = channels
            .into_iter()
            .enumerate()
            .map(|(channel_index, channel)| {
                let input_id = channel.input_id().unwrap_or(channel_index);
                ChannelRuntime::new(channel, input_id, Arc::clone(&sink))
            })
            .collect::<Vec<_>>();

        let task_locals = (0..dop)
            .map(|_| Mutex::new(ExecTaskLocal::new(&channel_runtimes)))
            .collect::<Vec<_>>();

        Self {
            channels: channel_runtimes,
            task_locals,
            cancelled: AtomicBool::new(false),
            poisoned: AtomicBool::new(false),
        }
    }

    fn step(&self, ctx: &TaskContext<T>, task_id: TaskId) -> BpResult<TaskStatus, T> {
        self.assert_not_poisoned();
        // Lock only this logical task, never all callbacks in the executor.
        // Acquire the lock even after cancellation to retain poison fail-fast.
        let mut local = self.task_locals[task_id]
            .lock()
            .expect("pipe exec mutex poisoned");
        let _panic_guard = CancelOnPanic {
            cancelled: &self.cancelled,
            poisoned: &self.poisoned,
        };
        let cleanup = StepCleanup {
            local: &mut local,
            cancelled: &self.cancelled,
        };
        self.assert_not_poisoned();
        let result = cleanup
            .local
            .step(ctx, task_id, &self.channels, &self.cancelled);
        self.assert_not_poisoned();
        if result.is_err() {
            self.cancelled.store(true, Ordering::Release);
        }
        if self.cancelled.load(Ordering::Acquire) {
            cleanup.local.resumers.fill(None);
            if result.is_ok() {
                return Ok(TaskStatus::Cancelled);
            }
        }
        result
    }

    fn assert_not_poisoned(&self) {
        assert!(
            !self.poisoned.load(Ordering::Acquire),
            "pipe exec mutex poisoned"
        );
    }
}

// Clear temporary handles on every exit, including a factory/callback unwind.
// Persistent continuation state is discarded only on fatal cancellation/unwind.
struct StepCleanup<'a> {
    local: &'a mut ExecTaskLocal,
    cancelled: &'a AtomicBool,
}

impl Drop for StepCleanup<'_> {
    fn drop(&mut self) {
        self.local.blocked_resumers.clear();
        if self.cancelled.load(Ordering::Acquire) || std::thread::panicking() {
            self.local.resumers.fill(None);
            for channel in &mut self.local.channels {
                channel.pipe_stack.clear();
            }
        }
    }
}

// Preserve unwinding/poisoning while preventing unaffected task slots from
// continuing after a callback or protocol panic.
struct CancelOnPanic<'a> {
    cancelled: &'a AtomicBool,
    poisoned: &'a AtomicBool,
}

impl Drop for CancelOnPanic<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.cancelled.store(true, Ordering::Release);
            self.poisoned.store(true, Ordering::Release);
        }
    }
}

impl ExecTaskLocal {
    fn step<T: PipelineTypes>(
        &mut self,
        ctx: &TaskContext<T>,
        task_id: TaskId,
        channels: &[ChannelRuntime<T>],
        cancelled: &AtomicBool,
    ) -> BpResult<TaskStatus, T> {
        let thread_id = task_id as ThreadId;
        if cancelled.load(Ordering::Acquire) {
            return Ok(TaskStatus::Cancelled);
        }

        let mut all_finished = true;
        let mut all_unfinished_blocked = true;
        let mut last_outcome = LastOutcome::Continue;

        for (channel_id, channel) in channels.iter().enumerate() {
            if cancelled.load(Ordering::Acquire) {
                return Ok(TaskStatus::Cancelled);
            }
            if self.finished[channel_id] {
                continue;
            }

            if let Some(resumer) = self.resumers[channel_id].as_ref() {
                if resumer.is_resumed() {
                    self.resumers[channel_id] = None;
                } else {
                    assert!(self.blocked_resumers.len() < channels.len());
                    self.blocked_resumers.push(Arc::clone(resumer));
                    all_finished = false;
                    continue;
                }
            }

            let out = match channel.step(ctx, thread_id, &mut self.channels[channel_id], cancelled)
            {
                Ok(out) => out,
                Err(error) => {
                    cancelled.store(true, Ordering::Release);
                    return Err(error);
                }
            };

            if let OpOutput::Finished(batch) = &out {
                debug_assert!(
                    batch.is_none(),
                    "channel finished output must not carry a batch"
                );
                self.finished[channel_id] = true;
            } else {
                all_finished = false;
            }

            if let OpOutput::Blocked(resumer) = &out {
                self.resumers[channel_id] = Some(Arc::clone(resumer));
                assert!(self.blocked_resumers.len() < channels.len());
                self.blocked_resumers.push(Arc::clone(resumer));
            } else {
                all_unfinished_blocked = false;
            }

            let finished_or_blocked = matches!(&out, OpOutput::Finished(_) | OpOutput::Blocked(_));

            last_outcome = match &out {
                OpOutput::PipeYield => LastOutcome::Yield,
                OpOutput::Cancelled => LastOutcome::Cancelled,
                _ => LastOutcome::Continue,
            };

            if !finished_or_blocked {
                break;
            }
        }

        if all_finished {
            return Ok(TaskStatus::Finished);
        }

        if all_unfinished_blocked && !self.blocked_resumers.is_empty() {
            if cancelled.load(Ordering::Acquire) {
                return Ok(TaskStatus::Cancelled);
            }
            let awaiter = ctx.make_awaiter_from_slice(&self.blocked_resumers)?;
            return Ok(TaskStatus::Blocked(awaiter));
        }

        match last_outcome {
            LastOutcome::Yield => Ok(TaskStatus::Yield),
            LastOutcome::Cancelled => {
                cancelled.store(true, Ordering::Release);
                Ok(TaskStatus::Cancelled)
            }
            LastOutcome::Continue => Ok(TaskStatus::Continue),
        }
    }
}

enum LastOutcome {
    Continue,
    Yield,
    Cancelled,
}

struct ExecTaskLocal {
    finished: Vec<bool>,
    resumers: Vec<Option<SharedResumer>>,
    channels: Vec<ChannelTaskLocal>,
    blocked_resumers: Vec<SharedResumer>,
}

impl ExecTaskLocal {
    fn new<T: PipelineTypes>(channels: &[ChannelRuntime<T>]) -> Self {
        let num_channels = channels.len();
        Self {
            finished: vec![false; num_channels],
            resumers: vec![None; num_channels],
            channels: channels
                .iter()
                .map(|channel| ChannelTaskLocal::new(channel.pipes.len()))
                .collect(),
            blocked_resumers: Vec::with_capacity(num_channels),
        }
    }
}

struct ChannelRuntime<T: PipelineTypes> {
    input_id: usize,
    source: SharedSourceOp<T>,
    pipes: Vec<SharedPipeOp<T>>,
    sink: SharedSinkOp<T>,
    drain_ids: Vec<usize>,
}

impl<T: PipelineTypes> ChannelRuntime<T> {
    fn new(channel: PipelineChannel<T>, input_id: usize, sink: SharedSinkOp<T>) -> Self {
        let drain_ids = channel
            .pipes()
            .iter()
            .enumerate()
            .filter_map(|(index, pipe)| pipe.has_drain().then_some(index))
            .collect::<Vec<_>>();

        Self {
            input_id,
            source: Arc::clone(channel.source()),
            pipes: channel.pipes().to_vec(),
            sink,
            drain_ids,
        }
    }

    fn step(
        &self,
        ctx: &TaskContext<T>,
        thread_id: ThreadId,
        local: &mut ChannelTaskLocal,
        cancelled: &AtomicBool,
    ) -> BpResult<OpOutput<T::Batch>, T> {
        let input_ctx = ctx.with_input_id(self.input_id);
        let call = CallbackContext {
            ctx: &input_ctx,
            thread_id,
            cancelled,
        };
        if cancelled.load(Ordering::Acquire) {
            return Ok(OpOutput::Cancelled);
        }

        if local.sinking {
            local.sinking = false;
            return self.sink_step(&call, local, None);
        }
        if let Some(pipe_id) = local.pipe_stack.pop() {
            return self.pipe_step(&call, local, pipe_id, None);
        }
        if !local.source_done {
            let Some(source_out) = call.invoke(|| self.source.source(call.ctx, thread_id))? else {
                return Ok(OpOutput::Cancelled);
            };
            match source_out {
                OpOutput::Blocked(resumer) => return Ok(OpOutput::Blocked(resumer)),
                OpOutput::Finished(batch) => {
                    local.source_done = true;
                    if let Some(batch) = batch {
                        return self.pipe_step(&call, local, 0, Some(batch));
                    }
                }
                OpOutput::SourcePipeHasMore(batch) => {
                    return self.pipe_step(&call, local, 0, Some(batch));
                }
                other => panic!("invalid source output: {}", other.label()),
            }
        }
        while local.draining < self.drain_ids.len() {
            let drain_id = self.drain_ids[local.draining];
            let Some(drain_out) =
                call.invoke(|| self.pipes[drain_id].drain(call.ctx, thread_id))?
            else {
                return Ok(OpOutput::Cancelled);
            };
            if local.yielded {
                if !matches!(drain_out, OpOutput::PipeYieldBack) {
                    panic!("drain yield handshake must resume with PIPE_YIELD_BACK");
                }
                local.yielded = false;
                return Ok(OpOutput::PipeYieldBack);
            }
            match drain_out {
                OpOutput::PipeYield => {
                    local.yielded = true;
                    return Ok(OpOutput::PipeYield);
                }
                OpOutput::Blocked(resumer) => return Ok(OpOutput::Blocked(resumer)),
                OpOutput::SourcePipeHasMore(batch) => {
                    return self.pipe_step(&call, local, drain_id + 1, Some(batch));
                }
                OpOutput::Finished(batch) => {
                    local.draining += 1;
                    if let Some(batch) = batch {
                        return self.pipe_step(&call, local, drain_id + 1, Some(batch));
                    }
                }
                other => panic!("invalid drain output: {}", other.label()),
            }
        }
        Ok(OpOutput::Finished(None))
    }

    fn pipe_step(
        &self,
        call: &CallbackContext<'_, T>,
        local: &mut ChannelTaskLocal,
        pipe_id: usize,
        mut input: Option<T::Batch>,
    ) -> BpResult<OpOutput<T::Batch>, T> {
        for (index, pipe) in self.pipes.iter().enumerate().skip(pipe_id) {
            let Some(out) = call.invoke(|| pipe.pipe(call.ctx, call.thread_id, input))? else {
                return Ok(OpOutput::Cancelled);
            };
            if local.yielded {
                if !matches!(out, OpOutput::PipeYieldBack) {
                    panic!("pipe yield handshake must resume with PIPE_YIELD_BACK");
                }
                local.push_pipe(index, self.pipes.len());
                local.yielded = false;
                return Ok(OpOutput::PipeYieldBack);
            }
            match out {
                OpOutput::PipeYield => {
                    local.push_pipe(index, self.pipes.len());
                    local.yielded = true;
                    return Ok(OpOutput::PipeYield);
                }
                OpOutput::Blocked(resumer) => {
                    local.push_pipe(index, self.pipes.len());
                    return Ok(OpOutput::Blocked(resumer));
                }
                OpOutput::PipeSinkNeedsMore => return Ok(OpOutput::PipeSinkNeedsMore),
                OpOutput::PipeEven(batch) => input = Some(batch),
                OpOutput::SourcePipeHasMore(batch) => {
                    local.push_pipe(index, self.pipes.len());
                    input = Some(batch);
                }
                other => panic!("invalid pipe output: {}", other.label()),
            }
        }
        self.sink_step(call, local, input)
    }

    fn sink_step(
        &self,
        call: &CallbackContext<'_, T>,
        local: &mut ChannelTaskLocal,
        input: Option<T::Batch>,
    ) -> BpResult<OpOutput<T::Batch>, T> {
        let Some(out) = call.invoke(|| self.sink.sink(call.ctx, call.thread_id, input))? else {
            return Ok(OpOutput::Cancelled);
        };
        match out {
            OpOutput::PipeSinkNeedsMore => Ok(OpOutput::PipeSinkNeedsMore),
            OpOutput::Blocked(resumer) => {
                local.sinking = true;
                Ok(OpOutput::Blocked(resumer))
            }
            other => panic!("invalid sink output: {}", other.label()),
        }
    }
}

struct CallbackContext<'a, T: PipelineTypes> {
    ctx: &'a TaskContext<T>,
    thread_id: ThreadId,
    cancelled: &'a AtomicBool,
}

impl<T: PipelineTypes> CallbackContext<'_, T> {
    // None means cancellation, distinct from an operator returning the invalid
    // OpOutput::Cancelled. In-flight outputs are dropped if cancellation wins.
    fn invoke(
        &self,
        callback: impl FnOnce() -> BpResult<OpOutput<T::Batch>, T>,
    ) -> BpResult<Option<OpOutput<T::Batch>>, T> {
        if self.cancelled.load(Ordering::Acquire) {
            return Ok(None);
        }
        let out = match callback() {
            Ok(out) => out,
            Err(error) => {
                self.cancelled.store(true, Ordering::Release);
                return Err(error);
            }
        };
        if self.cancelled.load(Ordering::Acquire) {
            return Ok(None);
        }
        Ok(Some(out))
    }
}

struct ChannelTaskLocal {
    sinking: bool,
    pipe_stack: Vec<usize>,
    source_done: bool,
    draining: usize,
    yielded: bool,
}

impl ChannelTaskLocal {
    fn new(num_pipes: usize) -> Self {
        Self {
            sinking: false,
            pipe_stack: Vec::with_capacity(num_pipes),
            source_done: false,
            draining: 0,
            yielded: false,
        }
    }

    fn push_pipe(&mut self, index: usize, num_pipes: usize) {
        assert!(index < num_pipes, "pipe continuation out of range");
        assert!(
            self.pipe_stack.len() < num_pipes,
            "pipe continuation depth exceeded"
        );
        assert!(
            self.pipe_stack
                .last()
                .is_none_or(|previous| *previous < index),
            "pipe continuations must be strictly increasing"
        );
        self.pipe_stack.push(index);
    }
}

#[cfg(test)]
#[path = "../tests/support/mod.rs"]
mod storage_support;

#[cfg(test)]
mod storage_tests {
    use super::*;
    use crate::{compile, Pipeline};
    use storage_support::{
        shared_pipe, shared_sink, shared_source, trace_log, RecordedAwaiter, ScriptOutput as O,
        ScriptedPipe, ScriptedSink, ScriptedSource, Step, TestResumer, TestTypes,
    };

    fn layout(exec: &PipeExec<TestTypes>, task: usize) -> (usize, usize, Vec<(usize, usize)>) {
        let local = exec.exec.task_locals[task].lock().unwrap();
        assert!(
            local.blocked_resumers.is_empty(),
            "scratch retained temporary handles"
        );
        (
            local.blocked_resumers.capacity(),
            local.blocked_resumers.as_ptr() as usize,
            local
                .channels
                .iter()
                .map(|c| (c.pipe_stack.capacity(), c.pipe_stack.as_ptr() as usize))
                .collect(),
        )
    }

    #[test]
    fn construction_bounds_and_backing_survive_nested_protocol() {
        for dop in [1, 2, 4] {
            let log = trace_log();
            let source = shared_source(ScriptedSource::new(
                "nested",
                vec![vec![Step::output(O::Finished(Some(1)))]],
                log.clone(),
            ));
            let parked = shared_source(ScriptedSource::new(
                "parked",
                vec![vec![Step::blocked(), Step::output(O::Finished(None))]],
                log.clone(),
            ));
            let p0 = shared_pipe(ScriptedPipe::new(
                "p0",
                vec![vec![
                    Step::output(O::SourcePipeHasMore(2)).with_expected_input(Some(1)),
                    Step::output(O::PipeSinkNeedsMore).with_expected_null_input(),
                ]],
                vec![vec![
                    Step::blocked(),
                    Step::output(O::PipeYield),
                    Step::output(O::PipeYieldBack),
                    Step::output(O::SourcePipeHasMore(7)),
                    Step::output(O::Finished(Some(10))),
                ]],
                None,
                log.clone(),
            ));
            let p1 = shared_pipe(ScriptedPipe::new(
                "p1",
                vec![vec![
                    Step::output(O::SourcePipeHasMore(3)).with_expected_input(Some(2)),
                    Step::output(O::PipeSinkNeedsMore).with_expected_null_input(),
                    Step::output(O::PipeEven(8)).with_expected_input(Some(7)),
                    Step::output(O::PipeEven(11)).with_expected_input(Some(10)),
                ]],
                vec![],
                None,
                log.clone(),
            ));
            let p2 = shared_pipe(ScriptedPipe::new(
                "p2",
                vec![vec![
                    Step::output(O::PipeYield).with_expected_input(Some(3)),
                    Step::output(O::PipeYieldBack).with_expected_null_input(),
                    Step::blocked().with_expected_null_input(),
                    Step::output(O::PipeSinkNeedsMore).with_expected_null_input(),
                    Step::output(O::PipeEven(9)).with_expected_input(Some(8)),
                    Step::output(O::PipeEven(12)).with_expected_input(Some(11)),
                ]],
                vec![],
                None,
                log.clone(),
            ));
            let sink = shared_sink(ScriptedSink::new(
                "sink",
                vec![vec![
                    Step::blocked().with_expected_input(Some(9)),
                    Step::output(O::PipeSinkNeedsMore).with_expected_null_input(),
                    Step::output(O::PipeSinkNeedsMore).with_expected_input(Some(12)),
                ]],
                log,
            ));
            let pipeline = Pipeline::new(
                "bounds",
                vec![
                    PipelineChannel::with_input_id(7, parked, vec![]),
                    PipelineChannel::with_input_id(19, source, vec![p0, p1, p2]),
                ],
                sink,
            );
            let exec = compile(&pipeline, dop).pipelinexes()[0].pipe_exec();
            let initial: Vec<_> = (0..dop).map(|id| layout(&exec, id)).collect();
            for (cap, _, stacks) in &initial {
                assert_eq!(*cap, 2);
                assert_eq!(stacks[0].0, 0);
                assert_eq!(stacks[1].0, 3);
            }
            let resumer = Arc::new(TestResumer::default()) as SharedResumer;
            let awaiter: crate::SharedAwaiter =
                Arc::new(RecordedAwaiter::new(vec![resumer.clone()]));
            let r = resumer.clone();
            let ctx = TaskContext::from_shared_context_with_borrowed_awaiter(
                Arc::new(0),
                Arc::new(move || Ok(r.clone())),
                Arc::new(move |_| Ok(awaiter.clone())),
            );
            let task = exec.task_group().task().clone();
            let mut labels = Vec::new();
            let mut maximum_depth = 0;
            for _ in 0..30 {
                let status = task.run(&ctx, 0).unwrap();
                labels.push(status.label());
                let local = exec.exec.task_locals[0].lock().unwrap();
                maximum_depth = maximum_depth.max(local.channels[1].pipe_stack.len());
                drop(local);
                assert_eq!(layout(&exec, 0), initial[0]);
                if status.is_blocked() {
                    resumer.resume();
                }
                if status.is_finished() {
                    break;
                }
            }
            assert_eq!(maximum_depth, 3);
            assert_eq!(
                labels,
                vec![
                    "YIELD", "CONTINUE", "BLOCKED", "CONTINUE", "CONTINUE", "CONTINUE", "BLOCKED",
                    "YIELD", "CONTINUE", "BLOCKED", "CONTINUE", "CONTINUE", "FINISHED"
                ]
            );
            for (id, expected) in initial.iter().enumerate() {
                assert_eq!(&layout(&exec, id), expected);
            }
        }
    }

    #[test]
    fn fatal_returns_preserve_backing_and_release_scratch_and_continuations() {
        for mode in ["error", "unwind", "other-task-cancel"] {
            let log = trace_log();
            let parked = shared_source(ScriptedSource::new(
                "parked",
                vec![
                    vec![Step::blocked()],
                    vec![Step::error("other task failed")],
                ],
                log.clone(),
            ));
            let source = shared_source(ScriptedSource::new(
                "batch",
                vec![vec![Step::output(O::Finished(Some(1)))]],
                log.clone(),
            ));
            let final_step = if mode == "unwind" {
                // Invalid pipe output intentionally exercises protocol unwind.
                Step::output(O::Finished(None)).with_expected_null_input()
            } else {
                Step::error("continuation failed").with_expected_null_input()
            };
            let pipe = shared_pipe(ScriptedPipe::new(
                "continuation",
                vec![vec![
                    Step::output(O::SourcePipeHasMore(2)).with_expected_input(Some(1)),
                    final_step,
                ]],
                vec![],
                None,
                log.clone(),
            ));
            let sink = shared_sink(ScriptedSink::new(
                "sink",
                vec![vec![
                    Step::output(O::PipeSinkNeedsMore).with_expected_input(Some(2))
                ]],
                log,
            ));
            let pipeline = Pipeline::new(
                "fatal-storage",
                vec![
                    PipelineChannel::with_input_id(7, parked, vec![]),
                    PipelineChannel::with_input_id(19, source, vec![pipe]),
                ],
                sink,
            );
            let exec = compile(&pipeline, 2).pipelinexes()[0].pipe_exec();
            let initial = layout(&exec, 0);
            let signal = Arc::new(TestResumer::default()) as SharedResumer;
            let ctx = TaskContext::from_shared_context_with_borrowed_awaiter(
                Arc::new(0usize),
                Arc::new(move || Ok(signal.clone())),
                Arc::new(|_| panic!("no aggregate blocked return expected")),
            );
            let task = exec.task_group().task().clone();
            assert!(task.run(&ctx, 0).unwrap().is_continue());
            assert_eq!(layout(&exec, 0), initial);
            assert_eq!(
                exec.exec.task_locals[0].lock().unwrap().channels[1]
                    .pipe_stack
                    .len(),
                1
            );
            match mode {
                "error" => assert_eq!(task.run(&ctx, 0).unwrap_err(), "continuation failed"),
                "unwind" => {
                    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                        || task.run(&ctx, 0)
                    ))
                    .is_err())
                }
                _ => {
                    assert_eq!(task.run(&ctx, 1).unwrap_err(), "other task failed");
                    assert!(task.run(&ctx, 0).unwrap().is_cancelled());
                }
            }
            // A test-only observer may inspect poisoned bytes; production never recovers this lock.
            let local = exec.exec.task_locals[0]
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            assert!(local.blocked_resumers.is_empty());
            assert!(local.resumers.iter().all(Option::is_none));
            assert!(local.channels.iter().all(|c| c.pipe_stack.is_empty()));
            let final_layout = (
                local.blocked_resumers.capacity(),
                local.blocked_resumers.as_ptr() as usize,
                local
                    .channels
                    .iter()
                    .map(|c| (c.pipe_stack.capacity(), c.pipe_stack.as_ptr() as usize))
                    .collect::<Vec<_>>(),
            );
            assert_eq!(final_layout, initial, "storage changed on {mode}");
            assert_eq!(exec.exec.poisoned.load(Ordering::Acquire), mode == "unwind");
        }
    }
}
