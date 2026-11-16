use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use arrow_array::{Array, ArrayRef, Int32Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use broken_pipeline::traits::arrow::{ArrowTypes, Batch, Result};
use broken_pipeline::{
    compile, OpOutput, PipeOperator, Pipeline, PipelineChannel, SinkOperator, SourceOperator,
    TaskContext, ThreadId,
};

struct BatchSource {
    batches: Mutex<VecDeque<Batch>>,
    finish_with_batch: bool,
    calls: AtomicUsize,
}

impl SourceOperator<ArrowTypes> for BatchSource {
    fn name(&self) -> &str {
        "ArrowSource"
    }

    fn source(
        &self,
        ctx: &TaskContext<ArrowTypes>,
        thread_id: ThreadId,
    ) -> Result<OpOutput<Batch>> {
        assert_eq!(thread_id, 0);
        assert_eq!(ctx.input_id(), Some(7));
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut batches = self.batches.lock().unwrap();
        Ok(match batches.pop_front() {
            Some(batch) if self.finish_with_batch && batches.is_empty() => {
                OpOutput::Finished(Some(batch))
            }
            Some(batch) => OpOutput::SourcePipeHasMore(batch),
            None => OpOutput::Finished(None),
        })
    }
}

#[derive(Default)]
struct PassThrough {
    calls: AtomicUsize,
}

impl PipeOperator<ArrowTypes> for PassThrough {
    fn name(&self) -> &str {
        "ArrowPassThrough"
    }

    fn pipe(
        &self,
        ctx: &TaskContext<ArrowTypes>,
        thread_id: ThreadId,
        input: Option<Batch>,
    ) -> Result<OpOutput<Batch>> {
        assert_eq!(thread_id, 0);
        assert_eq!(ctx.input_id(), Some(7));
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(OpOutput::PipeEven(input.expect(
            "pass-through must receive a batch, including empty batches",
        )))
    }
}

#[derive(Default)]
struct CollectSink {
    batches: Mutex<Vec<Batch>>,
    calls: AtomicUsize,
}

impl SinkOperator<ArrowTypes> for CollectSink {
    fn name(&self) -> &str {
        "ArrowSink"
    }

    fn sink(
        &self,
        ctx: &TaskContext<ArrowTypes>,
        thread_id: ThreadId,
        input: Option<Batch>,
    ) -> Result<OpOutput<Batch>> {
        assert_eq!(thread_id, 0);
        assert_eq!(ctx.input_id(), Some(7));
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.batches
            .lock()
            .unwrap()
            .push(input.expect("nonblocking sink must receive a batch"));
        Ok(OpOutput::PipeSinkNeedsMore)
    }
}

fn sliced_batches() -> Vec<Batch> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("number", DataType::Int32, true),
        Field::new("text", DataType::Utf8, true),
    ]));
    let numbers = Arc::new(Int32Array::from(vec![
        Some(-99),
        Some(10),
        None,
        Some(30),
        Some(40),
        None,
        Some(60),
        None,
        Some(80),
        Some(999),
    ]));
    let strings = Arc::new(StringArray::from(vec![
        Some("sentinel"),
        Some("ten"),
        None,
        Some("thirty"),
        None,
        Some("fifty"),
        Some("sixty"),
        None,
        Some("eighty"),
        Some("sentinel"),
    ]));
    let columns: Vec<ArrayRef> = vec![numbers.clone(), strings.clone()];
    let parent = RecordBatch::try_new(schema, columns).unwrap();
    let slices = [(1, 4), (5, 0), (5, 3), (8, 1)];
    let batches: Vec<Batch> = slices
        .iter()
        .map(|&(offset, len)| Arc::new(parent.slice(offset, len)))
        .collect();

    // Verify the fixtures really share parent storage and have sliced validity.
    for (&(offset, _), batch) in slices.iter().zip(&batches) {
        let actual_numbers = batch
            .column(0)
            .as_any()
            .downcast_ref::<Int32Array>()
            .unwrap();
        let actual_strings = batch
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(
            actual_numbers.values().as_ptr(),
            numbers.values()[offset..].as_ptr()
        );
        assert_eq!(
            actual_strings.value_offsets().as_ptr(),
            strings.value_offsets()[offset..].as_ptr()
        );
        assert_eq!(
            actual_strings.value_data().as_ptr(),
            strings.value_data().as_ptr()
        );
        for (actual, original) in batch.columns().iter().zip(parent.columns()) {
            let actual_nulls = actual.nulls().unwrap();
            let original_nulls = original.nulls().unwrap();
            assert_eq!(
                actual_nulls.buffer().as_ptr(),
                original_nulls.buffer().as_ptr()
            );
            assert_eq!(actual_nulls.offset(), offset);
        }
    }
    batches
}

fn assert_same_batch(actual: &Batch, expected: &Batch) {
    assert!(Arc::ptr_eq(actual, expected), "RecordBatch was replaced");
    assert_eq!(actual.schema(), expected.schema());
    assert_eq!(actual.num_rows(), expected.num_rows());
    assert_eq!(actual.num_columns(), expected.num_columns());
    for (actual, expected) in actual.columns().iter().zip(expected.columns()) {
        assert!(Arc::ptr_eq(actual, expected), "column was replaced");
        let actual_data = actual.to_data();
        let expected_data = expected.to_data();
        assert!(actual_data.ptr_eq(&expected_data), "Arrow data was copied");
        for (actual, expected) in actual_data.buffers().iter().zip(expected_data.buffers()) {
            assert_eq!(
                actual.as_ptr(),
                expected.as_ptr(),
                "value buffer was copied"
            );
            assert_eq!(actual.len(), expected.len());
        }
        let actual_nulls = actual.nulls().unwrap();
        let expected_nulls = expected.nulls().unwrap();
        assert_eq!(
            actual_nulls.buffer().as_ptr(),
            expected_nulls.buffer().as_ptr()
        );
        assert_eq!(actual_nulls.offset(), expected_nulls.offset());
        assert_eq!(actual_nulls.len(), expected_nulls.len());
    }
}

fn run_compiled_arrow_pipeline(finish_with_batch: bool) {
    let expected = sliced_batches();
    let source = Arc::new(BatchSource {
        batches: Mutex::new(expected.iter().cloned().collect()),
        finish_with_batch,
        calls: AtomicUsize::new(0),
    });
    let pipe = Arc::new(PassThrough::default());
    let sink = Arc::new(CollectSink::default());
    let pipeline = Pipeline::<ArrowTypes>::new(
        "Arrow60Compatibility",
        vec![PipelineChannel::with_input_id(
            7,
            source.clone(),
            vec![pipe.clone()],
        )],
        sink.clone(),
    );
    let compiled = compile(&pipeline, 1);
    assert_eq!(compiled.pipelinexes().len(), 1);
    let exec = compiled.pipelinexes()[0].pipe_exec();
    let group = exec.task_group();
    assert_eq!(group.num_tasks(), 1);
    let ctx = TaskContext::<ArrowTypes>::new(
        (),
        Arc::new(|| panic!("streaming pass-through must not block")),
        Arc::new(|_| panic!("streaming pass-through must not create an awaiter")),
    );

    // This drives BP's existing Task/PipeExec, not an alternate executor.
    // Each batch must be forwarded before the next step, including the empty slice.
    for (index, batch) in expected.iter().enumerate() {
        assert!(group.task().run(&ctx, 0).unwrap().is_continue());
        let actual = sink.batches.lock().unwrap();
        assert_eq!(actual.len(), index + 1);
        assert_same_batch(&actual[index], batch);
    }
    assert!(group.task().run(&ctx, 0).unwrap().is_finished());
    let source_calls = expected.len() + usize::from(!finish_with_batch);
    assert_eq!(source.calls.load(Ordering::SeqCst), source_calls);
    assert!(source.batches.lock().unwrap().is_empty());
    assert_eq!(pipe.calls.load(Ordering::SeqCst), expected.len());
    assert_eq!(sink.calls.load(Ordering::SeqCst), expected.len());

    // Completion is sticky and must not poll the source or duplicate output.
    assert!(group.task().run(&ctx, 0).unwrap().is_finished());
    assert_eq!(source.calls.load(Ordering::SeqCst), source_calls);
    assert_eq!(pipe.calls.load(Ordering::SeqCst), expected.len());
    assert_eq!(sink.calls.load(Ordering::SeqCst), expected.len());

    let actual = sink.batches.lock().unwrap();
    assert_eq!(actual.len(), 4);
    assert_eq!(
        actual.iter().map(|b| b.num_rows()).collect::<Vec<_>>(),
        vec![4, 0, 3, 1]
    );
    assert_eq!(actual.iter().map(|b| b.num_rows()).sum::<usize>(), 8);
    let mut numbers = Vec::new();
    let mut strings = Vec::new();
    let mut null_counts = Vec::new();
    for batch in actual.iter() {
        let ints = batch
            .column(0)
            .as_any()
            .downcast_ref::<Int32Array>()
            .unwrap();
        let text = batch
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        numbers.extend(ints.iter());
        strings.extend(text.iter());
        null_counts.push((ints.null_count(), text.null_count()));
    }
    assert_eq!(
        numbers,
        vec![
            Some(10),
            None,
            Some(30),
            Some(40),
            None,
            Some(60),
            None,
            Some(80)
        ]
    );
    assert_eq!(
        strings,
        vec![
            Some("ten"),
            None,
            Some("thirty"),
            None,
            Some("fifty"),
            Some("sixty"),
            None,
            Some("eighty")
        ]
    );
    assert_eq!(null_counts, vec![(1, 2), (0, 0), (2, 1), (0, 0)]);
}

#[test]
fn sliced_nullable_batches_reach_sink_before_separate_source_finish() {
    run_compiled_arrow_pipeline(false);
}

#[test]
fn sliced_nullable_final_batch_is_forwarded_exactly_once() {
    run_compiled_arrow_pipeline(true);
}
