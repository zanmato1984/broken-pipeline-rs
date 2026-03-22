use std::fmt;

use arrow_schema::ArrowError;
use broken_pipeline::traits::arrow::ArrowTypes;
use broken_pipeline::PipelineTypes;

pub use broken_pipeline::compile;
pub use broken_pipeline::{
    Awaiter, Continuation, OpOutput, PipeExec, PipeOperator, Pipeline, PipelineChannel,
    PipelineExec, Pipelinexe, Resumer, SharedAwaiter, SharedPipeOp, SharedResumer, SharedSinkOp,
    SharedSourceOp, SinkExec, SinkOperator, SourceExec, SourceOperator, Task, TaskHint,
    TaskHintType, TaskId, TaskStatus, ThreadId,
};

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ScheduleError {
    EmptyResumers {
        awaiter: &'static str,
    },
    InvalidReadyCount {
        awaiter: &'static str,
        num_readies: usize,
    },
    UnexpectedResumerType {
        awaiter: &'static str,
        expected: &'static str,
    },
    UnexpectedAwaiterType {
        expected: &'static str,
    },
    TaskGroupThreadPanicked,
}

impl fmt::Display for ScheduleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyResumers { awaiter } => {
                write!(f, "{awaiter}: empty resumers")
            }
            Self::InvalidReadyCount {
                awaiter,
                num_readies,
            } => write!(f, "{awaiter}: num_readies must be > 0, got {num_readies}"),
            Self::UnexpectedResumerType { awaiter, expected } => {
                write!(f, "{awaiter}: unexpected resumer type, expected {expected}")
            }
            Self::UnexpectedAwaiterType { expected } => {
                write!(f, "unexpected awaiter type, expected {expected}")
            }
            Self::TaskGroupThreadPanicked => write!(f, "task group thread panicked"),
        }
    }
}

impl std::error::Error for ScheduleError {}

pub trait ScheduleTypes: PipelineTypes + 'static
where
    Self::Error: Send + 'static,
{
    fn from_schedule_error(error: ScheduleError) -> Self::Error;
}

impl ScheduleTypes for ArrowTypes {
    fn from_schedule_error(error: ScheduleError) -> Self::Error {
        ArrowError::ComputeError(error.to_string())
    }
}
