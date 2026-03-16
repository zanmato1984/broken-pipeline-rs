mod support;

use std::sync::Arc;

use broken_pipeline_core::{compile, Pipeline, PipelineChannel};

use support::{
    awaiter_resumers, shared_pipe, shared_sink, shared_source, test_context, trace, trace_log,
    traces, ScriptOutput, ScriptedPipe, ScriptedSink, ScriptedSource, Step, TestTypes,
};

#[test]
fn empty_source_finishes_without_calling_sink() {
    let trace_log = trace_log();
    let source = shared_source(ScriptedSource::new(
        "Source",
        vec![vec![Step::output(ScriptOutput::Finished(None))]],
        Arc::clone(&trace_log),
    ));
    let sink = shared_sink(ScriptedSink::new(
        "Sink",
        vec![vec![]],
        Arc::clone(&trace_log),
    ));
    let pipeline = Pipeline::<TestTypes>::new(
        "EmptySource",
        vec![PipelineChannel::new(source, vec![])],
        sink,
    );

    let exec = compile(&pipeline, 1);
    let runtime = exec.pipelinexes()[0].pipe_exec();
    let status = runtime.step(&test_context(), 0).unwrap();

    assert!(status.is_finished());
    assert_eq!(
        traces(&trace_log),
        vec![trace("Source", "Source", None, "FINISHED")]
    );
}

#[test]
fn one_pass_with_pipe_matches_source_pipe_sink_flow() {
    let trace_log = trace_log();
    let source = shared_source(ScriptedSource::new(
        "Source",
        vec![vec![
            Step::output(ScriptOutput::SourcePipeHasMore(10)),
            Step::output(ScriptOutput::Finished(None)),
        ]],
        Arc::clone(&trace_log),
    ));
    let pipe = shared_pipe(ScriptedPipe::new(
        "Pipe",
        vec![vec![
            Step::output(ScriptOutput::PipeEven(20)).with_expected_input(Some(10))
        ]],
        vec![],
        None,
        Arc::clone(&trace_log),
    ));
    let sink = shared_sink(ScriptedSink::new(
        "Sink",
        vec![vec![
            Step::output(ScriptOutput::PipeSinkNeedsMore).with_expected_input(Some(20))
        ]],
        Arc::clone(&trace_log),
    ));
    let pipeline = Pipeline::<TestTypes>::new(
        "OnePass",
        vec![PipelineChannel::new(source, vec![pipe])],
        sink,
    );
    let runtime = compile(&pipeline, 1).pipelinexes()[0].pipe_exec();
    let ctx = test_context();

    assert!(runtime.step(&ctx, 0).unwrap().is_continue());
    assert!(runtime.step(&ctx, 0).unwrap().is_finished());
    assert_eq!(
        traces(&trace_log),
        vec![
            trace("Source", "Source", None, "SOURCE_PIPE_HAS_MORE"),
            trace("Pipe", "Pipe", Some(Some(10)), "PIPE_EVEN"),
            trace("Sink", "Sink", Some(Some(20)), "PIPE_SINK_NEEDS_MORE"),
            trace("Source", "Source", None, "FINISHED"),
        ]
    );
}

#[test]
fn pipe_needs_more_reenters_source_not_pipe() {
    let trace_log = trace_log();
    let source = shared_source(ScriptedSource::new(
        "Source",
        vec![vec![
            Step::output(ScriptOutput::SourcePipeHasMore(1)),
            Step::output(ScriptOutput::SourcePipeHasMore(2)),
            Step::output(ScriptOutput::Finished(None)),
        ]],
        Arc::clone(&trace_log),
    ));
    let pipe = shared_pipe(ScriptedPipe::new(
        "Pipe",
        vec![vec![
            Step::output(ScriptOutput::PipeSinkNeedsMore).with_expected_input(Some(1)),
            Step::output(ScriptOutput::PipeSinkNeedsMore).with_expected_input(Some(2)),
        ]],
        vec![],
        None,
        Arc::clone(&trace_log),
    ));
    let sink = shared_sink(ScriptedSink::new(
        "Sink",
        vec![vec![]],
        Arc::clone(&trace_log),
    ));
    let pipeline = Pipeline::<TestTypes>::new(
        "PipeNeedsMore",
        vec![PipelineChannel::new(source, vec![pipe])],
        sink,
    );
    let runtime = compile(&pipeline, 1).pipelinexes()[0].pipe_exec();
    let ctx = test_context();

    assert!(runtime.step(&ctx, 0).unwrap().is_continue());
    assert!(runtime.step(&ctx, 0).unwrap().is_continue());
    assert!(runtime.step(&ctx, 0).unwrap().is_finished());
    assert_eq!(
        traces(&trace_log),
        vec![
            trace("Source", "Source", None, "SOURCE_PIPE_HAS_MORE"),
            trace("Pipe", "Pipe", Some(Some(1)), "PIPE_SINK_NEEDS_MORE"),
            trace("Source", "Source", None, "SOURCE_PIPE_HAS_MORE"),
            trace("Pipe", "Pipe", Some(Some(2)), "PIPE_SINK_NEEDS_MORE"),
            trace("Source", "Source", None, "FINISHED"),
        ]
    );
}

#[test]
fn pipe_has_more_reenters_pipe_before_source() {
    let trace_log = trace_log();
    let source = shared_source(ScriptedSource::new(
        "Source",
        vec![vec![
            Step::output(ScriptOutput::SourcePipeHasMore(10)),
            Step::output(ScriptOutput::Finished(None)),
        ]],
        Arc::clone(&trace_log),
    ));
    let pipe = shared_pipe(ScriptedPipe::new(
        "Pipe",
        vec![vec![
            Step::output(ScriptOutput::SourcePipeHasMore(20)).with_expected_input(Some(10)),
            Step::output(ScriptOutput::PipeSinkNeedsMore).with_expected_null_input(),
        ]],
        vec![],
        None,
        Arc::clone(&trace_log),
    ));
    let sink = shared_sink(ScriptedSink::new(
        "Sink",
        vec![vec![
            Step::output(ScriptOutput::PipeSinkNeedsMore).with_expected_input(Some(20))
        ]],
        Arc::clone(&trace_log),
    ));
    let pipeline = Pipeline::<TestTypes>::new(
        "PipeHasMore",
        vec![PipelineChannel::new(source, vec![pipe])],
        sink,
    );
    let runtime = compile(&pipeline, 1).pipelinexes()[0].pipe_exec();
    let ctx = test_context();

    assert!(runtime.step(&ctx, 0).unwrap().is_continue());
    assert!(runtime.step(&ctx, 0).unwrap().is_continue());
    assert!(runtime.step(&ctx, 0).unwrap().is_finished());
    assert_eq!(
        traces(&trace_log),
        vec![
            trace("Source", "Source", None, "SOURCE_PIPE_HAS_MORE"),
            trace("Pipe", "Pipe", Some(Some(10)), "SOURCE_PIPE_HAS_MORE"),
            trace("Sink", "Sink", Some(Some(20)), "PIPE_SINK_NEEDS_MORE"),
            trace("Pipe", "Pipe", Some(None), "PIPE_SINK_NEEDS_MORE"),
            trace("Source", "Source", None, "FINISHED"),
        ]
    );
}

#[test]
fn pipe_yield_handshake_maps_to_task_yield_then_continue() {
    let trace_log = trace_log();
    let source = shared_source(ScriptedSource::new(
        "Source",
        vec![vec![
            Step::output(ScriptOutput::SourcePipeHasMore(1)),
            Step::output(ScriptOutput::Finished(None)),
        ]],
        Arc::clone(&trace_log),
    ));
    let pipe = shared_pipe(ScriptedPipe::new(
        "Pipe",
        vec![vec![
            Step::output(ScriptOutput::PipeYield).with_expected_input(Some(1)),
            Step::output(ScriptOutput::PipeYieldBack).with_expected_null_input(),
            Step::output(ScriptOutput::PipeSinkNeedsMore).with_expected_null_input(),
        ]],
        vec![],
        None,
        Arc::clone(&trace_log),
    ));
    let sink = shared_sink(ScriptedSink::new(
        "Sink",
        vec![vec![]],
        Arc::clone(&trace_log),
    ));
    let pipeline = Pipeline::<TestTypes>::new(
        "PipeYield",
        vec![PipelineChannel::new(source, vec![pipe])],
        sink,
    );
    let runtime = compile(&pipeline, 1).pipelinexes()[0].pipe_exec();
    let ctx = test_context();

    assert!(runtime.step(&ctx, 0).unwrap().is_yield());
    assert!(runtime.step(&ctx, 0).unwrap().is_continue());
    assert!(runtime.step(&ctx, 0).unwrap().is_continue());
    assert!(runtime.step(&ctx, 0).unwrap().is_finished());
    assert_eq!(
        traces(&trace_log),
        vec![
            trace("Source", "Source", None, "SOURCE_PIPE_HAS_MORE"),
            trace("Pipe", "Pipe", Some(Some(1)), "PIPE_YIELD"),
            trace("Pipe", "Pipe", Some(None), "PIPE_YIELD_BACK"),
            trace("Pipe", "Pipe", Some(None), "PIPE_SINK_NEEDS_MORE"),
            trace("Source", "Source", None, "FINISHED"),
        ]
    );
}

#[test]
fn pipe_blocked_reenters_with_null_input_after_resume() {
    let trace_log = trace_log();
    let source = shared_source(ScriptedSource::new(
        "Source",
        vec![vec![
            Step::output(ScriptOutput::SourcePipeHasMore(1)),
            Step::output(ScriptOutput::Finished(None)),
        ]],
        Arc::clone(&trace_log),
    ));
    let pipe = shared_pipe(ScriptedPipe::new(
        "Pipe",
        vec![vec![
            Step::blocked().with_expected_input(Some(1)),
            Step::output(ScriptOutput::PipeSinkNeedsMore).with_expected_null_input(),
        ]],
        vec![],
        None,
        Arc::clone(&trace_log),
    ));
    let sink = shared_sink(ScriptedSink::new(
        "Sink",
        vec![vec![]],
        Arc::clone(&trace_log),
    ));
    let pipeline = Pipeline::<TestTypes>::new(
        "PipeBlocked",
        vec![PipelineChannel::new(source, vec![pipe])],
        sink,
    );
    let runtime = compile(&pipeline, 1).pipelinexes()[0].pipe_exec();
    let ctx = test_context();

    let blocked = runtime.step(&ctx, 0).unwrap();
    let resumers = awaiter_resumers(blocked.awaiter().expect("missing blocked awaiter"));
    assert!(blocked.is_blocked());
    assert_eq!(resumers.len(), 1);
    resumers[0].resume();

    assert!(runtime.step(&ctx, 0).unwrap().is_continue());
    assert!(runtime.step(&ctx, 0).unwrap().is_finished());
    assert_eq!(
        traces(&trace_log),
        vec![
            trace("Source", "Source", None, "SOURCE_PIPE_HAS_MORE"),
            trace("Pipe", "Pipe", Some(Some(1)), "BLOCKED"),
            trace("Pipe", "Pipe", Some(None), "PIPE_SINK_NEEDS_MORE"),
            trace("Source", "Source", None, "FINISHED"),
        ]
    );
}

#[test]
fn sink_backpressure_reenters_with_null_input_after_resume() {
    let trace_log = trace_log();
    let source = shared_source(ScriptedSource::new(
        "Source",
        vec![vec![
            Step::output(ScriptOutput::SourcePipeHasMore(1)),
            Step::output(ScriptOutput::Finished(None)),
        ]],
        Arc::clone(&trace_log),
    ));
    let sink = shared_sink(ScriptedSink::new(
        "Sink",
        vec![vec![
            Step::blocked().with_expected_input(Some(1)),
            Step::output(ScriptOutput::PipeSinkNeedsMore).with_expected_null_input(),
        ]],
        Arc::clone(&trace_log),
    ));
    let pipeline = Pipeline::<TestTypes>::new(
        "SinkBlocked",
        vec![PipelineChannel::new(source, vec![])],
        sink,
    );
    let runtime = compile(&pipeline, 1).pipelinexes()[0].pipe_exec();
    let ctx = test_context();

    let blocked = runtime.step(&ctx, 0).unwrap();
    let resumers = awaiter_resumers(blocked.awaiter().expect("missing blocked awaiter"));
    assert!(blocked.is_blocked());
    assert_eq!(resumers.len(), 1);
    resumers[0].resume();

    assert!(runtime.step(&ctx, 0).unwrap().is_continue());
    assert!(runtime.step(&ctx, 0).unwrap().is_finished());
    assert_eq!(
        traces(&trace_log),
        vec![
            trace("Source", "Source", None, "SOURCE_PIPE_HAS_MORE"),
            trace("Sink", "Sink", Some(Some(1)), "BLOCKED"),
            trace("Sink", "Sink", Some(None), "PIPE_SINK_NEEDS_MORE"),
            trace("Source", "Source", None, "FINISHED"),
        ]
    );
}

#[test]
fn drain_can_emit_tail_output_after_source_finishes() {
    let trace_log = trace_log();
    let source = shared_source(ScriptedSource::new(
        "Source",
        vec![vec![Step::output(ScriptOutput::Finished(None))]],
        Arc::clone(&trace_log),
    ));
    let pipe = shared_pipe(ScriptedPipe::new(
        "Pipe",
        vec![vec![]],
        vec![vec![
            Step::output(ScriptOutput::SourcePipeHasMore(7)),
            Step::output(ScriptOutput::Finished(None)),
        ]],
        None,
        Arc::clone(&trace_log),
    ));
    let sink = shared_sink(ScriptedSink::new(
        "Sink",
        vec![vec![
            Step::output(ScriptOutput::PipeSinkNeedsMore).with_expected_input(Some(7))
        ]],
        Arc::clone(&trace_log),
    ));
    let pipeline = Pipeline::<TestTypes>::new(
        "Drain",
        vec![PipelineChannel::new(source, vec![pipe])],
        sink,
    );
    let runtime = compile(&pipeline, 1).pipelinexes()[0].pipe_exec();
    let ctx = test_context();

    assert!(runtime.step(&ctx, 0).unwrap().is_continue());
    assert!(runtime.step(&ctx, 0).unwrap().is_finished());
    assert_eq!(
        traces(&trace_log),
        vec![
            trace("Source", "Source", None, "FINISHED"),
            trace("Pipe", "Drain", None, "SOURCE_PIPE_HAS_MORE"),
            trace("Sink", "Sink", Some(Some(7)), "PIPE_SINK_NEEDS_MORE"),
            trace("Pipe", "Drain", None, "FINISHED"),
        ]
    );
}

#[test]
fn all_blocked_channels_aggregate_resumers_into_task_blocked() {
    let trace_log = trace_log();
    let source1 = shared_source(ScriptedSource::new(
        "Source1",
        vec![vec![Step::blocked()]],
        Arc::clone(&trace_log),
    ));
    let source2 = shared_source(ScriptedSource::new(
        "Source2",
        vec![vec![Step::blocked()]],
        Arc::clone(&trace_log),
    ));
    let sink = shared_sink(ScriptedSink::new(
        "Sink",
        vec![vec![]],
        Arc::clone(&trace_log),
    ));
    let pipeline = Pipeline::<TestTypes>::new(
        "AllBlocked",
        vec![
            PipelineChannel::new(source1, vec![]),
            PipelineChannel::new(source2, vec![]),
        ],
        sink,
    );
    let runtime = compile(&pipeline, 1).pipelinexes()[0].pipe_exec();
    let blocked = runtime.step(&test_context(), 0).unwrap();

    assert!(blocked.is_blocked());
    assert_eq!(
        awaiter_resumers(blocked.awaiter().expect("missing blocked awaiter")).len(),
        2
    );
    assert_eq!(
        traces(&trace_log),
        vec![
            trace("Source1", "Source", None, "BLOCKED"),
            trace("Source2", "Source", None, "BLOCKED"),
        ]
    );
}

#[test]
fn error_cancels_subsequent_calls() {
    let trace_log = trace_log();
    let source = shared_source(ScriptedSource::new(
        "Source",
        vec![vec![Step::error("boom")]],
        Arc::clone(&trace_log),
    ));
    let sink = shared_sink(ScriptedSink::new(
        "Sink",
        vec![vec![]],
        Arc::clone(&trace_log),
    ));
    let pipeline =
        Pipeline::<TestTypes>::new("Error", vec![PipelineChannel::new(source, vec![])], sink);
    let runtime = compile(&pipeline, 1).pipelinexes()[0].pipe_exec();
    let ctx = test_context();

    let err = runtime.step(&ctx, 0).unwrap_err();
    assert_eq!(err, "boom");
    assert!(runtime.step(&ctx, 0).unwrap().is_cancelled());
    assert_eq!(
        traces(&trace_log),
        vec![trace("Source", "Source", None, "ERROR(boom)")]
    );
}
