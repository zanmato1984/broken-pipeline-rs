# broken-pipeline-rs

`broken-pipeline-rs` is a native Rust port of the C++ Broken Pipeline project.
It keeps the same conceptual layering:

- `broken-pipeline`: the core task/operator/pipeline protocol and `PipeExec`
- `broken-pipeline-schedule`: an optional generic schedule layer with ready-made schedulers

## Workspace layout

- `crates/broken-pipeline`
  - generic core protocol modeled after the C++ headers
  - pipeline compilation with implicit-source stage splitting
  - `PipeExec` reference runtime
  - Arrow binding under `broken_pipeline::traits::arrow`
- `crates/broken-pipeline-schedule`
  - generic schedule layer over `PipelineTypes`
  - detail awaiters/resumers (`callback`, `conditional`, `future`, `coro`, `single-thread`)
  - scheduler front-ends: `NaiveParallelScheduler`, `AsyncDualPoolScheduler`,
    `ParallelCoroScheduler`, and `SequentialCoroScheduler`

## Arrow binding

Like the C++ tree, the Rust port keeps the core protocol traits-oriented while still
shipping an explicit Arrow binding. The primary Arrow binding lives in:

- `broken_pipeline::traits::arrow::ArrowTypes`
- `broken_pipeline::traits::arrow::Batch`

This keeps the protocol reusable while still making Arrow `RecordBatch` the first-class
integration type for the in-tree tests and examples.

## Build and test

From the repository root:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

The repository also ships a GitHub Actions workflow that runs the same formatting,
clippy, and Rust test checks on pushes and pull requests.

## Ported functionality

- task, continuation, task-group, resumer, and awaiter contracts
- source/pipe/sink operator protocol and `OpOutput` state machine
- pipeline compilation with implicit-source stage splitting
- `PipeExec` small-step runtime semantics, including blocked/yield/drain handling
- optional schedule-layer awaiters, resumers, and scheduler façades

## Notes

- The schedule layer is kept separate from the core crate, mirroring the C++ repository.
- The Rust scheduler implementations focus on preserving protocol/runtime behavior rather
  than reproducing Folly or C++ coroutine internals literally.
