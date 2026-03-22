use std::sync::Arc;

use arrow_array::RecordBatch;
use arrow_schema::ArrowError;

use crate::types::PipelineTypes;

pub type Batch = Arc<RecordBatch>;
pub type Error = ArrowError;
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, Default)]
pub struct ArrowTypes;

impl PipelineTypes for ArrowTypes {
    type Batch = Batch;
    type Error = Error;
    type Context = ();
}
