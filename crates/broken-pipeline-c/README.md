# broken-pipeline-c

`broken-pipeline-c` is the Rust port's current C ABI implementation crate.

It is intentionally kept separate from the generic ABI/protocol source of truth:

- Generic ABI/protocol draft: `../../third_party/broken-pipeline-abi/`
- Current Rust implementation header: `include/broken_pipeline_c.h`

The vendored generic ABI snapshot is currently pinned to upstream commit
`8f3b133e057d41fecc77ae9c31d25e36529008f5` and is refreshed manually on purpose.

## Scope today

The current crate exposes a small task-group-oriented interop surface used by the Rust
port's smoke tests and non-Rust integration experiments.

It does **not** yet claim to implement the full generic ABI draft from
`broken-pipeline-abi`.

This repository intentionally carries a hard-duplicated pinned snapshot rather than an
update script.

## Why both exist

- `broken-pipeline-abi` defines the shared, implementation-neutral contract that future
  C++, Rust, and Go ports can converge on.
- `broken-pipeline-c` is the Rust port's concrete exported C ABI library today.

As the generic ABI stabilizes, this crate can evolve toward implementing that shared
contract more completely.
