pub use broken_pipeline::compile;
pub use broken_pipeline::traits::arrow::{ArrowTypes as Traits, Batch, Error as Status, Result};
pub use broken_pipeline::{
    Awaiter, Continuation, OpOutput, PipeExec, PipeOperator, Pipeline, PipelineChannel,
    PipelineExec, Pipelinexe, Resumer, SharedAwaiter, SharedPipeOp, SharedResumer, SharedSinkOp,
    SharedSourceOp, SinkExec, SinkOperator, SourceExec, SourceOperator, Task, TaskHint,
    TaskHintType, TaskId, TaskStatus, ThreadId,
};

pub type TaskContext = broken_pipeline::TaskContext<Traits>;
pub type TaskGroup = broken_pipeline::TaskGroup<Traits>;
pub type OpResult = Result<OpOutput<Batch>>;
pub type PipelineSource = SharedSourceOp<Traits>;
pub type PipelinePipe = SharedPipeOp<Traits>;
pub type PipelineSink = SharedSinkOp<Traits>;

pub type SourceOp = dyn SourceOperator<Traits>;
pub type PipeOp = dyn PipeOperator<Traits>;
pub type SinkOp = dyn SinkOperator<Traits>;
