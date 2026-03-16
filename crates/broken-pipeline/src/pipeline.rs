use std::sync::Arc;

use crate::operator::{SharedPipeOp, SharedSinkOp, SharedSourceOp};
use crate::types::PipelineTypes;

pub struct PipelineChannel<T: PipelineTypes> {
    source: SharedSourceOp<T>,
    pipes: Vec<SharedPipeOp<T>>,
}

impl<T: PipelineTypes> Clone for PipelineChannel<T> {
    fn clone(&self) -> Self {
        Self {
            source: Arc::clone(&self.source),
            pipes: self.pipes.to_vec(),
        }
    }
}

impl<T: PipelineTypes> PipelineChannel<T> {
    pub fn new(source: SharedSourceOp<T>, pipes: Vec<SharedPipeOp<T>>) -> Self {
        Self { source, pipes }
    }

    pub fn source(&self) -> &SharedSourceOp<T> {
        &self.source
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
