# Conformance

## Draft status

This repository defines draft conformance expectations. It is not yet a certification
program, but it does define the minimum behavior expected from conforming hosts and
providers.

## Conformance profiles

### Runtime profile

A runtime-conforming implementation should:

- negotiate the generic ABI version before use
- honor the task/task-group state machine from the draft header
- implement ownership and lifetime rules correctly
- keep provider-specific op/expr APIs outside the generic runtime surface
- document any runtime extensions it requires

### Build profile

A build-conforming implementation should additionally:

- keep provider API naming/versioning separate from the generic ABI version
- treat build request payloads as provider-defined rather than generic core semantics
- produce executable artifacts that can be handed to the runtime seam
- report build-time diagnostics through the documented host/provider contract

## Host checklist

A conforming host should:

- implement the generic host-side API directly or through a language adapter
- schedule `bp_task_group_descriptor` according to the draft semantics
- handle `BLOCKED`, `YIELD`, `FINISHED`, and `CANCELLED` outcomes correctly
- respect every ownership callback and never free foreign memory implicitly
- avoid assuming any provider-specific request or artifact format unless it explicitly
  speaks that provider API

## Provider checklist

A conforming provider should:

- export the generic discovery entrypoint `bp_query_provider_v0`
- advertise its provider API name/version separately from the generic ABI version
- define concrete op/expr/build APIs in its own repository
- depend on the relevant language-specific Broken Pipeline port
- bind executable artifacts into the generic runtime task-group form
- document extensions, payload formats, and behavioral assumptions outside the core ABI

## Lightweight validation

This repository ships `scripts/validate-docs.sh` for a simple structural check. That script
is not a full conformance suite; it only helps catch missing required files and obvious
spec regressions.
