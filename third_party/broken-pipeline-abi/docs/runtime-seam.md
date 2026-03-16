# Runtime Seam

## Goal

The runtime seam begins after a provider-specific build step has produced an executable
artifact. At this point the generic ABI takes over and the host only needs the execution
contract, not the provider's concrete operator or expression API.

## Why the seam centers on task/task-group semantics

Execution is centered on task/task-group semantics because that is the narrowest contract
that remains useful across multiple Broken Pipeline ports.

- A task group is a stable unit of work for schedulers and adapters.
- The host only needs to understand task count, task callbacks, optional continuation,
  status transitions, and ownership rules.
- Concrete operators, expressions, schemas, and lowering details do not need to cross the
  runtime ABI once an executable artifact already exists.
- Task/task-group semantics map naturally onto language-specific schedulers in C++, Rust,
  and Go without forcing a provider-specific graph API into the host.

## Draft runtime model

The current draft header models runtime execution around:

- `bp_task_group_descriptor` as the generic execution unit
- one task callback plus `task_count`
- an optional continuation callback that runs after task completion
- task states: `CONTINUE`, `BLOCKED`, `YIELD`, `FINISHED`, and `CANCELLED`
- a minimal awaiter-like wait handle for blocked work
- a host-side interface for diagnostics and task-group scheduling/awaiting

This draft intentionally keeps lower-level resumer wiring inside ports and providers where
possible. The generic header exposes enough to model blocked execution without forcing one
language's internal scheduler architecture onto every other port.

## Host responsibilities at runtime

A host is responsible for:

- validating ABI compatibility before use
- invoking the provider's runtime binding to obtain a `bp_task_group_descriptor`
- scheduling the task group directly or through an adapter
- honoring `BLOCKED` and `YIELD` semantics correctly
- awaiting blocked work through the supplied wait handle
- running or delegating continuation execution after the task group completes
- surfacing diagnostics and failures without assuming provider-specific internals

Hosts may implement the generic host-side API directly in C or another systems language,
or they may use a language adapter that wraps a Rust, C++, or Go implementation.

## Provider responsibilities at runtime

A provider is responsible for:

- binding its executable artifact into a generic task-group descriptor
- returning task callbacks whose semantics match the generic task state machine
- keeping provider-specific op/expr details out of the generic runtime ABI
- documenting any runtime extensions that are not part of the core draft
- depending on its language's Broken Pipeline port for the actual execution machinery

## What does not belong in the runtime seam

The runtime seam should not directly encode:

- concrete source/pipe/sink operator constructors
- expression builder APIs
- schema or type system details beyond what a provider-specific API documents
- optimizer controls, plan rewrites, or provider-specific lowering knobs
- language-specific scheduler types

Those belong in provider repositories and language ports, not in the shared runtime ABI.
