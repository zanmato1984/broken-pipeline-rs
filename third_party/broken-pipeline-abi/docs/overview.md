# Overview

## Scope

This repository defines the generic Broken Pipeline ABI and protocol that sits between a
host and a provider. It is the shared source of truth for the C-facing contract, not the
source of truth for any single language implementation.

## Terminology

- **Host**: the embedding process that loads a provider, negotiates ABI compatibility,
  and drives execution or adapter calls.
- **Provider**: an implementation that exposes concrete operators, expressions, or plan
  builders and lowers them into Broken Pipeline execution artifacts.
- **Port**: a language-specific Broken Pipeline implementation such as a future C++,
  Rust, or Go port.
- **Adapter**: a language-specific bridge that implements the generic host-side API on
  behalf of a host.
- **Build seam**: the construction-time contract that turns provider-defined requests
  into executable artifacts.
- **Runtime seam**: the execution-time contract that runs those artifacts through generic
  task/task-group semantics.

## Generic ABI vs provider-specific API

The generic ABI and a provider-specific API solve different problems.

| Layer | Defined in this repo | Defined in provider repos |
| --- | --- | --- |
| Version negotiation | Yes | No |
| Ownership and lifetime rules | Yes | Must follow core rules |
| Task-group runtime contract | Yes | Must implement or adapt |
| Build/runtime artifact handoff | Yes | Must use or document deviations |
| Concrete operators and expressions | No | Yes |
| Schemas, DSLs, serialized plan formats | No | Yes |
| Optimizer knobs and domain features | No | Yes |

A provider-specific API may be written in C, C++, Rust, Go, or another language, but it
still belongs in a provider repository and must depend on that language's Broken Pipeline
port. The generic ABI here stays intentionally smaller so that multiple ports can converge
on the same host/provider seam.

## Host-side vs provider-side responsibilities

### Host side

Hosts are responsible for:

- negotiating the generic ABI version
- owning the scheduler or adapter that drives `bp_task_group_descriptor`
- honoring ownership and lifetime rules from the core ABI
- treating provider API payloads as opaque unless the host intentionally speaks that
  provider-specific API
- implementing the generic host-side API directly or via a language adapter

### Provider side

Providers are responsible for:

- defining concrete op/expr/build APIs outside this repository
- depending on the appropriate language-specific Broken Pipeline port
- lowering provider-defined requests into executable build artifacts
- exposing runtime bindings that present generic task/task-group semantics to the host
- documenting any extensions that go beyond the generic core

## Why the ABI is split into build and runtime seams

The construction/build-time API is separate from the execution-time API because those two
surfaces change for different reasons.

- The build seam is where provider-specific operators, expressions, and serialized plan
  formats live. That surface is inherently provider-specific and often evolves quickly.
- The runtime seam is where hosts need a stable execution contract. Task/task-group
  semantics are smaller, more reusable, and more portable across languages and schedulers.

This split lets providers evolve rich builder APIs without forcing those details into the
shared runtime ABI.

## Pinning policy

Ports and providers should vendor or pin this repository at a specific commit or release.
They should not assume that tracking an unpinned branch is a stable integration strategy.
