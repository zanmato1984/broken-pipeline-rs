use std::sync::Arc;

use crate::task::{SharedResumer, TaskContext, TaskGroup};
use crate::types::{BpResult, PipelineTypes, ThreadId};

pub enum OpOutput<B> {
    PipeSinkNeedsMore,
    PipeEven(B),
    SourcePipeHasMore(B),
    Blocked(SharedResumer),
    PipeYield,
    PipeYieldBack,
    Finished(Option<B>),
    Cancelled,
}

impl<B> OpOutput<B> {
    pub fn is_pipe_sink_needs_more(&self) -> bool {
        matches!(self, Self::PipeSinkNeedsMore)
    }

    pub fn is_pipe_even(&self) -> bool {
        matches!(self, Self::PipeEven(_))
    }

    pub fn is_source_pipe_has_more(&self) -> bool {
        matches!(self, Self::SourcePipeHasMore(_))
    }

    pub fn is_blocked(&self) -> bool {
        matches!(self, Self::Blocked(_))
    }

    pub fn is_pipe_yield(&self) -> bool {
        matches!(self, Self::PipeYield)
    }

    pub fn is_pipe_yield_back(&self) -> bool {
        matches!(self, Self::PipeYieldBack)
    }

    pub fn is_finished(&self) -> bool {
        matches!(self, Self::Finished(_))
    }

    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }

    pub fn resumer(&self) -> Option<&SharedResumer> {
        match self {
            Self::Blocked(resumer) => Some(resumer),
            _ => None,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::PipeSinkNeedsMore => "PIPE_SINK_NEEDS_MORE",
            Self::PipeEven(_) => "PIPE_EVEN",
            Self::SourcePipeHasMore(_) => "SOURCE_PIPE_HAS_MORE",
            Self::Blocked(_) => "BLOCKED",
            Self::PipeYield => "PIPE_YIELD",
            Self::PipeYieldBack => "PIPE_YIELD_BACK",
            Self::Finished(_) => "FINISHED",
            Self::Cancelled => "CANCELLED",
        }
    }
}

pub type SharedSourceOp<T> = Arc<dyn SourceOperator<T>>;
pub type SharedPipeOp<T> = Arc<dyn PipeOperator<T>>;
pub type SharedSinkOp<T> = Arc<dyn SinkOperator<T>>;

pub trait SourceOperator<T: PipelineTypes>: Send + Sync {
    fn name(&self) -> &str;
    fn source(&self, ctx: &TaskContext<T>, thread_id: ThreadId) -> BpResult<OpOutput<T::Batch>, T>;

    fn frontend(&self) -> Vec<TaskGroup<T>> {
        Vec::new()
    }

    fn backend(&self) -> Option<TaskGroup<T>> {
        None
    }
}

pub trait PipeOperator<T: PipelineTypes>: Send + Sync {
    fn name(&self) -> &str;
    fn pipe(
        &self,
        ctx: &TaskContext<T>,
        thread_id: ThreadId,
        input: Option<T::Batch>,
    ) -> BpResult<OpOutput<T::Batch>, T>;

    fn has_drain(&self) -> bool {
        false
    }

    fn drain(
        &self,
        _ctx: &TaskContext<T>,
        _thread_id: ThreadId,
    ) -> BpResult<OpOutput<T::Batch>, T> {
        panic!("drain() called for a pipe without drain support")
    }

    fn implicit_source(&self) -> Option<SharedSourceOp<T>> {
        None
    }
}

pub trait SinkOperator<T: PipelineTypes>: Send + Sync {
    fn name(&self) -> &str;
    fn sink(
        &self,
        ctx: &TaskContext<T>,
        thread_id: ThreadId,
        input: Option<T::Batch>,
    ) -> BpResult<OpOutput<T::Batch>, T>;

    fn frontend(&self) -> Vec<TaskGroup<T>> {
        Vec::new()
    }

    fn backend(&self) -> Option<TaskGroup<T>> {
        None
    }

    fn implicit_source(&self) -> Option<SharedSourceOp<T>> {
        None
    }
}
