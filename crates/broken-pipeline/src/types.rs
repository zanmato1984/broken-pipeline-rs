pub type TaskId = usize;
pub type ThreadId = usize;

pub trait PipelineTypes {
    type Batch;
    type Error;
}

pub type BpResult<T, Types> = std::result::Result<T, <Types as PipelineTypes>::Error>;
