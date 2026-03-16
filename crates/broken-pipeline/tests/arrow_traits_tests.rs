use std::sync::Arc;

use arrow_array::{ArrayRef, Int32Array, RecordBatch};
use arrow_schema::{DataType, Field, Schema};

use broken_pipeline::traits::arrow::Batch;
use broken_pipeline::OpOutput;

#[test]
fn arrow_traits_binding_supports_record_batches() {
    let schema = Arc::new(Schema::new(vec![Field::new(
        "value",
        DataType::Int32,
        false,
    )]));
    let values: ArrayRef = Arc::new(Int32Array::from(vec![1, 2, 3]));
    let batch: Batch = Arc::new(RecordBatch::try_new(schema, vec![values]).unwrap());

    let output: OpOutput<Batch> = OpOutput::Finished(Some(Arc::clone(&batch)));
    assert!(output.is_finished());

    match output {
        OpOutput::Finished(Some(actual)) => {
            assert_eq!(actual.num_rows(), 3);
            assert_eq!(actual.num_columns(), 1);
        }
        _ => panic!("unexpected output variant"),
    }
}
