pub use broken_pipeline_core::compile;
pub use broken_pipeline_core::traits::arrow::{
    ArrowTypes as Traits, Batch, Error as Status, Result,
};
pub use broken_pipeline_core::{
    Awaiter, Continuation, OpOutput, PipeExec, PipeOperator, Pipeline, PipelineChannel,
    PipelineExec, Pipelinexe, Resumer, SharedAwaiter, SharedPipeOp, SharedResumer, SharedSinkOp,
    SharedSourceOp, SinkExec, SinkOperator, SourceExec, SourceOperator, Task, TaskHint,
    TaskHintType, TaskId, TaskStatus, ThreadId,
};

pub type TaskContext = broken_pipeline_core::TaskContext<Traits>;
pub type TaskGroup = broken_pipeline_core::TaskGroup<Traits>;
pub type OpResult = Result<OpOutput<Batch>>;
pub type PipelineSource = SharedSourceOp<Traits>;
pub type PipelinePipe = SharedPipeOp<Traits>;
pub type PipelineSink = SharedSinkOp<Traits>;

pub type SourceOp = dyn SourceOperator<Traits>;
pub type PipeOp = dyn PipeOperator<Traits>;
pub type SinkOp = dyn SinkOperator<Traits>;
