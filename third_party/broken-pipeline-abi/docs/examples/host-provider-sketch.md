# Host/Provider Sketch

This is a tiny interaction sketch showing where the generic ABI ends and a provider-specific
API begins.

## Step 1: discover the provider

A host loads a provider and calls `bp_query_provider_v0` to obtain:

- the generic ABI version the provider speaks
- the provider's own API name and version
- build/runtime vtables for the generic seam

## Step 2: use a provider-specific build API

The host (or an adapter) uses the provider's concrete API to describe operators,
expressions, schemas, or plans. That API is not defined by this repository.

The provider-specific API eventually yields a request payload that the generic build seam
can carry as an opaque buffer.

## Step 3: build an executable artifact

The provider's build vtable turns that opaque request payload into an executable artifact.
The artifact is still provider-defined internally, but it is now ready for the generic
runtime seam.

## Step 4: bind into a task group

The provider's runtime vtable binds the executable artifact into a
`bp_task_group_descriptor`.

At this point the host only needs generic execution semantics:

- task count
- task callback
- optional continuation
- task states and ownership rules

## Step 5: execute through the host runtime

The host or adapter schedules the task group, reacts to `YIELD` or `BLOCKED`, waits on any
returned wait handle, and finally runs the continuation if present.

The host never needs to understand the provider's concrete operator API at runtime.
