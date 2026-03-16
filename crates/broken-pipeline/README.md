# broken-pipeline

`broken-pipeline` is the Rust port of the Broken Pipeline core protocol.
It preserves the C++ layering around:

- task/task-group scheduling contracts
- source/pipe/sink operator protocols
- pipeline compilation with implicit-source stage splitting
- the reference `PipeExec` runtime

The crate remains traits-oriented like the C++ core, and it now ships a first-class
Arrow binding in `broken_pipeline::traits::arrow` for `RecordBatch`-based
integrations.

## Key modules

- `types`: task and thread identifiers plus the `PipelineTypes` contract
- `task`: task context, status, resumer/awaiter protocol, task groups
- `operator`: source/pipe/sink traits and `OpOutput`
- `pipeline`: logical pipeline/channel definition
- `compile`: stage splitting around implicit sources
- `pipeline_exec`: small-step reference runtime
- `traits::arrow`: Apache Arrow `RecordBatch` binding

## Commands

```bash
cargo test -p broken-pipeline
cargo fmt --all
```
