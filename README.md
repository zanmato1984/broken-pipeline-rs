# broken-pipeline-rs

`broken-pipeline-rs` is a native Rust port of the C++ Broken Pipeline project.
It keeps the same conceptual layering:

- `broken-pipeline`: the core task/operator/pipeline protocol and `PipeExec`
- `broken-pipeline-schedule`: an optional Arrow-bound schedule layer with ready-made schedulers
- `broken-pipeline-c`: a focused C API for task-group interop

## Workspace layout

- `crates/broken-pipeline`
  - generic core protocol modeled after the C++ headers
  - pipeline compilation with implicit-source stage splitting
  - `PipeExec` reference runtime
  - Arrow binding under `broken_pipeline::traits::arrow`
- `crates/broken-pipeline-schedule`
  - re-exports Arrow-bound aliases like the C++ `schedule/traits.h`
  - detail awaiters/resumers (`callback`, `conditional`, `future`, `coro`, `single-thread`)
  - scheduler front-ends: `NaiveParallelScheduler`, `AsyncDualPoolScheduler`,
    `ParallelCoroScheduler`, and `SequentialCoroScheduler`
- `crates/broken-pipeline-c`
  - C ABI for running task groups through the Rust schedulers
  - public header in `crates/broken-pipeline-c/include/broken_pipeline_c.h`
  - standalone C smoke tests in `crates/broken-pipeline-c/tests/c`

## Arrow-bound design

Like the C++ tree, the Rust port keeps the core protocol traits-oriented while shipping
an explicit Arrow binding. The primary Arrow binding lives in:

- `broken_pipeline::traits::arrow::ArrowTypes`
- `broken_pipeline::traits::arrow::Batch`
- `broken_pipeline_schedule::Traits`

This keeps the protocol reusable while making Arrow `RecordBatch` the first-class bound
for the optional schedule layer and public integration examples.

## Build and test

From the repository root:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
./scripts/test-c-api.sh
```

## Ported functionality

- task, continuation, task-group, resumer, and awaiter contracts
- source/pipe/sink operator protocol and `OpOutput` state machine
- pipeline compilation with implicit-source stage splitting
- `PipeExec` small-step runtime semantics, including blocked/yield/drain handling
- optional schedule-layer awaiters, resumers, and scheduler façades
- C ABI for scheduler-driven task group execution

## Notes

- The schedule layer is kept separate from the core crate, mirroring the C++ repository.
- The Rust scheduler implementations focus on preserving protocol/runtime behavior rather
  than reproducing Folly or C++ coroutine internals literally.
- The C API currently targets task-group interop, which is the cleanest stable surface
  for non-Rust hosts.
