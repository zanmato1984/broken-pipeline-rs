mod support;

use std::sync::Arc;

use broken_pipeline_core::{compile, Pipeline, PipelineChannel};

use support::{
    shared_pipe, shared_sink, shared_source, trace_log, ScriptedPipe, ScriptedSink, ScriptedSource,
    TestTypes,
};

#[test]
fn compile_empty_pipeline_keeps_layout_non_invasive() {
    let traces = trace_log();
    let sink = shared_sink(ScriptedSink::new("Sink", vec![vec![]], traces));
    let pipeline = Pipeline::<TestTypes>::new("EmptyPipeline", vec![], sink);
    let exec = compile(&pipeline, 1);

    assert_eq!(exec.name(), "EmptyPipeline");
    assert_eq!(exec.dop(), 1);
    assert!(exec.pipelinexes().is_empty());
}

#[test]
fn compile_single_channel_pipeline_preserves_original_channel() {
    let traces = trace_log();
    let source = shared_source(ScriptedSource::new(
        "Source",
        vec![vec![]],
        Arc::clone(&traces),
    ));
    let pipe = shared_pipe(ScriptedPipe::new(
        "Pipe",
        vec![vec![]],
        vec![],
        None,
        Arc::clone(&traces),
    ));
    let sink = shared_sink(ScriptedSink::new("Sink", vec![vec![]], traces));
    let pipeline = Pipeline::<TestTypes>::new(
        "SingleChannelPipeline",
        vec![PipelineChannel::new(
            Arc::clone(&source),
            vec![Arc::clone(&pipe)],
        )],
        Arc::clone(&sink),
    );

    let exec = compile(&pipeline, 2);
    let stage = &exec.pipelinexes()[0];

    assert_eq!(exec.pipelinexes().len(), 1);
    assert_eq!(stage.dop(), 2);
    assert_eq!(stage.channels().len(), 1);
    assert_eq!(stage.num_implicit_sources(), 0);
    assert!(Arc::ptr_eq(stage.channels()[0].source(), &source));
    assert!(Arc::ptr_eq(&stage.channels()[0].pipes()[0], &pipe));
    assert!(Arc::ptr_eq(pipeline.sink(), &sink));
}

#[test]
fn compile_triple_stage_pipeline_matches_cpp_stage_splitting() {
    let traces = trace_log();
    let source1 = shared_source(ScriptedSource::new(
        "Source1",
        vec![vec![]],
        Arc::clone(&traces),
    ));
    let source2 = shared_source(ScriptedSource::new(
        "Source2",
        vec![vec![]],
        Arc::clone(&traces),
    ));
    let implicit1 = shared_source(ScriptedSource::new(
        "ImplicitSource1",
        vec![vec![]],
        Arc::clone(&traces),
    ));
    let implicit2 = shared_source(ScriptedSource::new(
        "ImplicitSource2",
        vec![vec![]],
        Arc::clone(&traces),
    ));
    let implicit3 = shared_source(ScriptedSource::new(
        "ImplicitSource3",
        vec![vec![]],
        Arc::clone(&traces),
    ));

    let pipe1 = shared_pipe(ScriptedPipe::new(
        "Pipe1",
        vec![vec![]],
        vec![],
        Some(Arc::clone(&implicit1)),
        Arc::clone(&traces),
    ));
    let pipe2 = shared_pipe(ScriptedPipe::new(
        "Pipe2",
        vec![vec![]],
        vec![],
        Some(Arc::clone(&implicit2)),
        Arc::clone(&traces),
    ));
    let pipe3 = shared_pipe(ScriptedPipe::new(
        "Pipe3",
        vec![vec![]],
        vec![],
        Some(Arc::clone(&implicit3)),
        Arc::clone(&traces),
    ));
    let sink = shared_sink(ScriptedSink::new("Sink", vec![vec![]], traces));

    let pipeline = Pipeline::<TestTypes>::new(
        "TriplePhysicalPipeline",
        vec![
            PipelineChannel::new(
                Arc::clone(&source1),
                vec![Arc::clone(&pipe1), Arc::clone(&pipe3)],
            ),
            PipelineChannel::new(
                Arc::clone(&source2),
                vec![Arc::clone(&pipe2), Arc::clone(&pipe3)],
            ),
        ],
        sink,
    );

    let exec = compile(&pipeline, 1);

    assert_eq!(exec.pipelinexes().len(), 3);

    let stage0 = &exec.pipelinexes()[0];
    assert_eq!(stage0.channels().len(), 2);
    assert_eq!(stage0.num_implicit_sources(), 0);
    assert!(Arc::ptr_eq(stage0.channels()[0].source(), &source1));
    assert!(Arc::ptr_eq(stage0.channels()[1].source(), &source2));

    let stage1 = &exec.pipelinexes()[1];
    assert_eq!(stage1.channels().len(), 2);
    assert_eq!(stage1.num_implicit_sources(), 2);
    assert!(Arc::ptr_eq(stage1.channels()[0].source(), &implicit1));
    assert!(Arc::ptr_eq(stage1.channels()[1].source(), &implicit2));
    assert_eq!(stage1.channels()[0].pipes().len(), 1);
    assert_eq!(stage1.channels()[1].pipes().len(), 1);
    assert!(Arc::ptr_eq(&stage1.channels()[0].pipes()[0], &pipe3));
    assert!(Arc::ptr_eq(&stage1.channels()[1].pipes()[0], &pipe3));

    let stage2 = &exec.pipelinexes()[2];
    assert_eq!(stage2.channels().len(), 1);
    assert_eq!(stage2.num_implicit_sources(), 1);
    assert!(Arc::ptr_eq(stage2.channels()[0].source(), &implicit3));
    assert!(stage2.channels()[0].pipes().is_empty());
}
