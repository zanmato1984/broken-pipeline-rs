# broken-pipeline-abi

`broken-pipeline-abi` is the source-of-truth repository for the generic Broken Pipeline
C ABI and protocol documentation. It is intentionally independent of any single C++,
Rust, or Go port implementation.

## Purpose

This repository exists to:

- define the shared generic ABI/protocol surface that multiple Broken Pipeline ports can implement
- describe the host/provider contract at build time and runtime
- provide a stable spec artifact that ports and providers can vendor or pin
- keep the generic contract separate from provider-specific operator and expression APIs

## What this repo is not

This repository is not:

- a scheduler or runtime implementation
- a provider implementation repository
- the home of concrete operator, expression, schema, or optimizer APIs
- a language-specific source of truth for Rust, C++, Go, or any single port

## Intended consumers

The primary consumers are:

- language ports that want a shared ABI and protocol reference
- provider repositories that expose concrete op/expr APIs on top of a language-specific Broken Pipeline port
- hosts that load providers directly or through language adapters

Providers are expected to depend on their language's Broken Pipeline port and to vendor
or pin this repository at a specific revision. Hosts may implement the generic host-side
API directly or consume it through language adapters.

## Draft status

The header in `include/broken_pipeline/broken_pipeline_c.h` is an initial draft. It is
meant to be careful, minimal, and spec-oriented rather than a claim that the full ABI is
already final. The current draft focuses on:

- ABI version negotiation
- ownership and lifetime rules
- a generic runtime seam centered on task/task-group execution semantics
- a separate build seam for provider-defined executable artifacts
- explicit room for extensions and provider-specific APIs outside the generic core

## Repository layout

- `include/broken_pipeline/broken_pipeline_c.h` - initial generic C ABI draft
- `docs/overview.md` - scope, terminology, and layering
- `docs/runtime-seam.md` - execution-time host/provider contract
- `docs/build-seam.md` - build-time construction and artifact contract
- `docs/ownership.md` - memory ownership and lifetime rules
- `docs/extensions.md` - extension strategy and promotion rules
- `docs/versioning.md` - ABI, provider API, and pinning/version guidance
- `docs/conformance.md` - draft conformance expectations for hosts and providers
- `docs/examples/host-provider-sketch.md` - tiny end-to-end interaction sketch
- `scripts/validate-docs.sh` - lightweight repository validation

## Working model

The generic ABI in this repository defines the shared seam between a host and a provider.
That seam is intentionally smaller than any provider-specific API.

- The generic ABI covers version negotiation, task-group execution, build/runtime handoff,
  ownership rules, and extension discipline.
- Provider-specific repositories define concrete operators, expressions, schemas, plan
  formats, serialized payloads, and higher-level builder APIs.
- The runtime seam is centered on task/task-group semantics because that is the smallest
  execution contract that stays stable across languages, schedulers, and providers.
- The build seam is separate because provider-specific construction APIs are richer,
  evolve faster, and should not leak into the generic runtime contract.

## Validation

Run `scripts/validate-docs.sh` from the repository root for a quick structural check.

## License

Apache-2.0. See `LICENSE`.
