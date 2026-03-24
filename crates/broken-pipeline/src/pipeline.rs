use std::sync::Arc;

use crate::operator::{SharedPipeOp, SharedSinkOp, SharedSourceOp};
use crate::types::PipelineTypes;

pub struct PipelineChannel<T: PipelineTypes> {
    input_id: Option<usize>,
    source: SharedSourceOp<T>,
    pipes: Vec<SharedPipeOp<T>>,
}

impl<T: PipelineTypes> Clone for PipelineChannel<T> {
    fn clone(&self) -> Self {
        Self {
            input_id: self.input_id,
            source: Arc::clone(&self.source),
            pipes: self.pipes.to_vec(),
        }
    }
}

impl<T: PipelineTypes> PipelineChannel<T> {
    pub fn new(source: SharedSourceOp<T>, pipes: Vec<SharedPipeOp<T>>) -> Self {
        Self {
            input_id: None,
            source,
            pipes,
        }
    }

    pub fn with_input_id(
        input_id: usize,
        source: SharedSourceOp<T>,
        pipes: Vec<SharedPipeOp<T>>,
    ) -> Self {
        Self {
            input_id: Some(input_id),
            source,
            pipes,
        }
    }

    pub fn source(&self) -> &SharedSourceOp<T> {
        &self.source
    }

    pub fn input_id(&self) -> Option<usize> {
        self.input_id
    }

    pub fn pipes(&self) -> &[SharedPipeOp<T>] {
        &self.pipes
    }
}

pub struct Pipeline<T: PipelineTypes> {
    name: String,
    channels: Vec<PipelineChannel<T>>,
    sink: SharedSinkOp<T>,
}

impl<T: PipelineTypes> Clone for Pipeline<T> {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            channels: self.channels.clone(),
            sink: Arc::clone(&self.sink),
        }
    }
}

impl<T: PipelineTypes> Pipeline<T> {
    pub fn new(
        name: impl Into<String>,
        channels: Vec<PipelineChannel<T>>,
        sink: SharedSinkOp<T>,
    ) -> Self {
        Self {
            name: name.into(),
            channels,
            sink,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn channels(&self) -> &[PipelineChannel<T>] {
        &self.channels
    }

    pub fn sink(&self) -> &SharedSinkOp<T> {
        &self.sink
    }
}
