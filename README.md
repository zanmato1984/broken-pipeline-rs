# broken-pipeline-rs

`broken-pipeline-rs` is a native Rust port of the C++ Broken Pipeline project.
It keeps the same conceptual layering:

- `broken-pipeline`: the core task/operator/pipeline protocol and `PipeExec`
- `broken-pipeline-schedule`: an optional Arrow-bound schedule layer with ready-made schedulers
- `broken-pipeline-c`: the Rust port's current C ABI implementation crate
- `third_party/broken-pipeline-abi`: a pinned vendored snapshot of the shared generic ABI/protocol repo

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
  - Rust implementation crate for the port's current C ABI surface
  - public header in `crates/broken-pipeline-c/include/broken_pipeline_c.h`
  - standalone C smoke tests in `crates/broken-pipeline-c/tests/c`
- `third_party/broken-pipeline-abi`
  - pinned vendored snapshot of the generic ABI/protocol source-of-truth repository
  - duplicated in-tree on purpose rather than synced by script

## Vendored ABI snapshot

This repository carries a pinned hard copy of `broken-pipeline-abi` under
`third_party/broken-pipeline-abi/`, currently vendored from commit
`8f3b133e057d41fecc77ae9c31d25e36529008f5`.

`broken-pipeline-abi` remains the source-of-truth for the generic Broken Pipeline ABI
and protocol documentation. This repository intentionally carries a hard-duplicated
snapshot instead of an update script, so refreshes are manual and reviewable.

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

The repository also ships a GitHub Actions workflow that runs the same formatting,
clippy, Rust test, and C API test checks on pushes and pull requests.

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
- `third_party/broken-pipeline-abi/` is a pinned local snapshot of the canonical
  `broken-pipeline-abi` repository at commit `8f3b133e057d41fecc77ae9c31d25e36529008f5`.
  It documents the intended generic host/provider ABI.
- The current `broken-pipeline-c` crate is still narrower than that generic draft: it is a
  Rust-port implementation surface for task-group-oriented interop, not yet a full generic
  ABI implementation.
