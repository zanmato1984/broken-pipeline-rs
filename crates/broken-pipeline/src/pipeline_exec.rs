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
    exec: Arc<Mutex<ExecState<T>>>,
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
            exec: Arc::new(Mutex::new(ExecState::new(channels, sink, dop))),
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
        self.exec
            .lock()
            .expect("pipe exec mutex poisoned")
            .step(ctx, task_id)
    }
}

struct ExecState<T: PipelineTypes> {
    channels: Vec<ChannelRuntime<T>>,
    thread_locals: Vec<ExecThreadLocal>,
    cancelled: bool,
}

impl<T: PipelineTypes> ExecState<T> {
    fn new(channels: Vec<PipelineChannel<T>>, sink: SharedSinkOp<T>, dop: usize) -> Self {
        let channel_runtimes = channels
            .into_iter()
            .map(|channel| ChannelRuntime::new(channel, Arc::clone(&sink), dop))
            .collect::<Vec<_>>();

        let thread_locals = (0..dop)
            .map(|_| ExecThreadLocal::new(channel_runtimes.len()))
            .collect::<Vec<_>>();

        Self {
            channels: channel_runtimes,
            thread_locals,
            cancelled: false,
        }
    }

    fn step(&mut self, ctx: &TaskContext<T>, task_id: TaskId) -> BpResult<TaskStatus, T> {
        let thread_id = task_id as ThreadId;
        if self.cancelled {
            return Ok(TaskStatus::Cancelled);
        }

        let mut all_finished = true;
        let mut all_unfinished_blocked = true;
        let mut blocked_resumers = Vec::new();
        let mut last_outcome = LastOutcome::Continue;

        for channel_id in 0..self.channels.len() {
            if self.thread_locals[thread_id].finished[channel_id] {
                continue;
            }

            if let Some(resumer) = self.thread_locals[thread_id].resumers[channel_id].as_ref() {
                if resumer.is_resumed() {
                    self.thread_locals[thread_id].resumers[channel_id] = None;
                } else {
                    blocked_resumers.push(Arc::clone(resumer));
                    all_finished = false;
                    continue;
                }
            }

            let out = match self.channels[channel_id].step(ctx, thread_id, &mut self.cancelled) {
                Ok(out) => out,
                Err(error) => {
                    self.cancelled = true;
                    return Err(error);
                }
            };

            if let OpOutput::Finished(batch) = &out {
                debug_assert!(
                    batch.is_none(),
                    "channel finished output must not carry a batch"
                );
                self.thread_locals[thread_id].finished[channel_id] = true;
            } else {
                all_finished = false;
            }

            if let OpOutput::Blocked(resumer) = &out {
                self.thread_locals[thread_id].resumers[channel_id] = Some(Arc::clone(resumer));
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
            let awaiter = ctx.make_awaiter(blocked_resumers)?;
            return Ok(TaskStatus::Blocked(awaiter));
        }

        match last_outcome {
            LastOutcome::Yield => Ok(TaskStatus::Yield),
            LastOutcome::Cancelled => {
                self.cancelled = true;
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

struct ExecThreadLocal {
    finished: Vec<bool>,
    resumers: Vec<Option<SharedResumer>>,
}

impl ExecThreadLocal {
    fn new(num_channels: usize) -> Self {
        Self {
            finished: vec![false; num_channels],
            resumers: vec![None; num_channels],
        }
    }
}

struct ChannelRuntime<T: PipelineTypes> {
    source: SharedSourceOp<T>,
    pipes: Vec<SharedPipeOp<T>>,
    sink: SharedSinkOp<T>,
    drain_ids: Vec<usize>,
    thread_locals: Vec<ChannelThreadLocal>,
}

impl<T: PipelineTypes> ChannelRuntime<T> {
    fn new(channel: PipelineChannel<T>, sink: SharedSinkOp<T>, dop: usize) -> Self {
        let drain_ids = channel
            .pipes()
            .iter()
            .enumerate()
            .filter_map(|(index, pipe)| pipe.has_drain().then_some(index))
            .collect::<Vec<_>>();

        Self {
            source: Arc::clone(channel.source()),
            pipes: channel.pipes().to_vec(),
            sink,
            drain_ids,
            thread_locals: (0..dop).map(|_| ChannelThreadLocal::default()).collect(),
        }
    }

    fn step(
        &mut self,
        ctx: &TaskContext<T>,
        thread_id: ThreadId,
        cancelled: &mut bool,
    ) -> BpResult<OpOutput<T::Batch>, T> {
        if *cancelled {
            return Ok(OpOutput::Cancelled);
        }

        if self.thread_locals[thread_id].sinking {
            self.thread_locals[thread_id].sinking = false;
            return self.sink_step(ctx, thread_id, None, cancelled);
        }

        if let Some(pipe_id) = self.thread_locals[thread_id].pipe_stack.pop() {
            return self.pipe_step(ctx, thread_id, pipe_id, None, cancelled);
        }

        if !self.thread_locals[thread_id].source_done {
            let source_out = match self.source.source(ctx, thread_id) {
                Ok(out) => out,
                Err(error) => {
                    *cancelled = true;
                    return Err(error);
                }
            };

            match source_out {
                OpOutput::Blocked(resumer) => return Ok(OpOutput::Blocked(resumer)),
                OpOutput::Finished(batch) => {
                    self.thread_locals[thread_id].source_done = true;
                    if let Some(batch) = batch {
                        return self.pipe_step(ctx, thread_id, 0, Some(batch), cancelled);
                    }
                }
                OpOutput::SourcePipeHasMore(batch) => {
                    return self.pipe_step(ctx, thread_id, 0, Some(batch), cancelled);
                }
                other => panic!("invalid source output: {}", other.label()),
            }
        }

        while self.thread_locals[thread_id].draining < self.drain_ids.len() {
            let drain_slot = self.thread_locals[thread_id].draining;
            let drain_id = self.drain_ids[drain_slot];
            let drain_out = match self.pipes[drain_id].drain(ctx, thread_id) {
                Ok(out) => out,
                Err(error) => {
                    *cancelled = true;
                    return Err(error);
                }
            };

            if self.thread_locals[thread_id].yielded {
                if !matches!(drain_out, OpOutput::PipeYieldBack) {
                    panic!("drain yield handshake must resume with PIPE_YIELD_BACK");
                }
                self.thread_locals[thread_id].yielded = false;
                return Ok(OpOutput::PipeYieldBack);
            }

            match drain_out {
                OpOutput::PipeYield => {
                    self.thread_locals[thread_id].yielded = true;
                    return Ok(OpOutput::PipeYield);
                }
                OpOutput::Blocked(resumer) => return Ok(OpOutput::Blocked(resumer)),
                OpOutput::SourcePipeHasMore(batch) => {
                    return self.pipe_step(ctx, thread_id, drain_id + 1, Some(batch), cancelled);
                }
                OpOutput::Finished(batch) => {
                    self.thread_locals[thread_id].draining += 1;
                    if let Some(batch) = batch {
                        return self.pipe_step(
                            ctx,
                            thread_id,
                            drain_id + 1,
                            Some(batch),
                            cancelled,
                        );
                    }
                }
                other => panic!("invalid drain output: {}", other.label()),
            }
        }

        Ok(OpOutput::Finished(None))
    }

    fn pipe_step(
        &mut self,
        ctx: &TaskContext<T>,
        thread_id: ThreadId,
        pipe_id: usize,
        mut input: Option<T::Batch>,
        cancelled: &mut bool,
    ) -> BpResult<OpOutput<T::Batch>, T> {
        for index in pipe_id..self.pipes.len() {
            let out = match self.pipes[index].pipe(ctx, thread_id, input) {
                Ok(out) => out,
                Err(error) => {
                    *cancelled = true;
                    return Err(error);
                }
            };

            if self.thread_locals[thread_id].yielded {
                if !matches!(out, OpOutput::PipeYieldBack) {
                    panic!("pipe yield handshake must resume with PIPE_YIELD_BACK");
                }
                self.thread_locals[thread_id].pipe_stack.push(index);
                self.thread_locals[thread_id].yielded = false;
                return Ok(OpOutput::PipeYieldBack);
            }

            match out {
                OpOutput::PipeYield => {
                    self.thread_locals[thread_id].pipe_stack.push(index);
                    self.thread_locals[thread_id].yielded = true;
                    return Ok(OpOutput::PipeYield);
                }
                OpOutput::Blocked(resumer) => {
                    self.thread_locals[thread_id].pipe_stack.push(index);
                    return Ok(OpOutput::Blocked(resumer));
                }
                OpOutput::PipeSinkNeedsMore => return Ok(OpOutput::PipeSinkNeedsMore),
                OpOutput::PipeEven(batch) => {
                    input = Some(batch);
                }
                OpOutput::SourcePipeHasMore(batch) => {
                    self.thread_locals[thread_id].pipe_stack.push(index);
                    input = Some(batch);
                }
                other => panic!("invalid pipe output: {}", other.label()),
            }
        }

        self.sink_step(ctx, thread_id, input, cancelled)
    }

    fn sink_step(
        &mut self,
        ctx: &TaskContext<T>,
        thread_id: ThreadId,
        input: Option<T::Batch>,
        cancelled: &mut bool,
    ) -> BpResult<OpOutput<T::Batch>, T> {
        let out = match self.sink.sink(ctx, thread_id, input) {
            Ok(out) => out,
            Err(error) => {
                *cancelled = true;
                return Err(error);
            }
        };

        match out {
            OpOutput::PipeSinkNeedsMore => Ok(OpOutput::PipeSinkNeedsMore),
            OpOutput::Blocked(resumer) => {
                self.thread_locals[thread_id].sinking = true;
                Ok(OpOutput::Blocked(resumer))
            }
            other => panic!("invalid sink output: {}", other.label()),
        }
    }
}

#[derive(Default)]
struct ChannelThreadLocal {
    sinking: bool,
    pipe_stack: Vec<usize>,
    source_done: bool,
    draining: usize,
    yielded: bool,
}
