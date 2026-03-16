use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use crate::operator::SharedSourceOp;
use crate::pipeline::{Pipeline, PipelineChannel};
use crate::pipeline_exec::{PipelineExec, Pipelinexe, SinkExec};
use crate::types::PipelineTypes;

pub fn compile<T: PipelineTypes>(pipeline: &Pipeline<T>, dop: usize) -> PipelineExec<T> {
    let mut topology = HashMap::<usize, TopologyEntry<T>>::new();
    let mut sources_keep_order = Vec::<usize>::new();
    let mut implicit_sources_keepalive = HashMap::<usize, SharedSourceOp<T>>::new();
    let mut pipe_source_map = HashMap::<usize, SharedSourceOp<T>>::new();

    for channel in pipeline.channels() {
        let mut stage_id = 0usize;
        let source_id = arc_id(channel.source());
        topology.entry(source_id).or_insert_with(|| TopologyEntry {
            stage_id,
            channel: channel.clone(),
        });
        sources_keep_order.push(source_id);
        stage_id += 1;

        for (index, pipe) in channel.pipes().iter().enumerate() {
            let pipe_id = arc_id(pipe);
            if let Some(implicit_source) = pipe_source_map.get(&pipe_id) {
                let implicit_source_id = arc_id(implicit_source);
                if let Some(entry) = topology.get_mut(&implicit_source_id) {
                    if entry.stage_id < stage_id {
                        entry.stage_id = stage_id;
                        stage_id += 1;
                    }
                }
                continue;
            }

            if let Some(implicit_source) = pipe.implicit_source() {
                let implicit_source_id = arc_id(&implicit_source);
                pipe_source_map.insert(pipe_id, Arc::clone(&implicit_source));
                topology.insert(
                    implicit_source_id,
                    TopologyEntry {
                        stage_id,
                        channel: PipelineChannel::new(
                            Arc::clone(&implicit_source),
                            channel.pipes()[index + 1..].to_vec(),
                        ),
                    },
                );
                sources_keep_order.push(implicit_source_id);
                implicit_sources_keepalive.insert(implicit_source_id, implicit_source);
                stage_id += 1;
            }
        }
    }

    let mut pipelinexe_infos =
        BTreeMap::<usize, (Vec<SharedSourceOp<T>>, Vec<PipelineChannel<T>>)>::new();
    for source_id in sources_keep_order {
        if let Some(entry) = topology.remove(&source_id) {
            let stage = pipelinexe_infos.entry(entry.stage_id).or_default();
            if let Some(implicit_source) = implicit_sources_keepalive.remove(&source_id) {
                stage.0.push(implicit_source);
            }
            stage.1.push(entry.channel);
        }
    }

    let pipelinexes = pipelinexe_infos
        .into_iter()
        .map(|(stage_id, (implicit_sources, channels))| {
            Pipelinexe::new(
                format!("pipelinexe{}({})", stage_id, pipeline.name()),
                channels,
                implicit_sources,
                Arc::clone(pipeline.sink()),
                dop,
            )
        })
        .collect::<Vec<_>>();

    let sink = SinkExec {
        frontend: pipeline.sink().frontend(),
        backend: pipeline.sink().backend(),
    };

    PipelineExec::new(pipeline.name().to_string(), sink, pipelinexes, dop)
}

struct TopologyEntry<T: PipelineTypes> {
    stage_id: usize,
    channel: PipelineChannel<T>,
}

fn arc_id<T: ?Sized>(value: &Arc<T>) -> usize {
    Arc::as_ptr(value) as *const () as usize
}
