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
            .map(|_| Mutex::new(ExecTaskLocal::new(channel_runtimes.len())))
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
        self.assert_not_poisoned();
        let result = local.step(ctx, task_id, &self.channels, &self.cancelled);
        self.assert_not_poisoned();
        if result.is_err() {
            self.cancelled.store(true, Ordering::Release);
        }
        if self.cancelled.load(Ordering::Acquire) {
            local.resumers.fill(None);
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
        let mut blocked_resumers = Vec::new();
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
                    blocked_resumers.push(Arc::clone(resumer));
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
                blocked_resumers.push(Arc::clone(resumer));
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

        if all_unfinished_blocked && !blocked_resumers.is_empty() {
            if cancelled.load(Ordering::Acquire) {
                return Ok(TaskStatus::Cancelled);
            }
            let awaiter = ctx.make_awaiter(blocked_resumers)?;
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
}

impl ExecTaskLocal {
    fn new(num_channels: usize) -> Self {
        Self {
            finished: vec![false; num_channels],
            resumers: vec![None; num_channels],
            channels: (0..num_channels)
                .map(|_| ChannelTaskLocal::default())
                .collect(),
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
                local.pipe_stack.push(index);
                local.yielded = false;
                return Ok(OpOutput::PipeYieldBack);
            }
            match out {
                OpOutput::PipeYield => {
                    local.pipe_stack.push(index);
                    local.yielded = true;
                    return Ok(OpOutput::PipeYield);
                }
                OpOutput::Blocked(resumer) => {
                    local.pipe_stack.push(index);
                    return Ok(OpOutput::Blocked(resumer));
                }
                OpOutput::PipeSinkNeedsMore => return Ok(OpOutput::PipeSinkNeedsMore),
                OpOutput::PipeEven(batch) => input = Some(batch),
                OpOutput::SourcePipeHasMore(batch) => {
                    local.pipe_stack.push(index);
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

#[derive(Default)]
struct ChannelTaskLocal {
    sinking: bool,
    pipe_stack: Vec<usize>,
    source_done: bool,
    draining: usize,
    yielded: bool,
}
