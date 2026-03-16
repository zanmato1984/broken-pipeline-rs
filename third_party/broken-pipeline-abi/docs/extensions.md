# Extensions

## Philosophy

The core ABI should stay small and generic. Most innovation should happen in provider
repositories and language-specific ports first.

## Extension lanes

The current draft expects extensions to happen through a few explicit lanes:

- provider API name/version pairs
- provider-defined request and artifact payload formats
- capability flags
- additional provider-defined symbols or sidecar APIs
- companion documentation in provider repositories

## Rules for extensions

An extension should:

- use a provider-specific namespace or clearly named sidecar surface
- avoid redefining any core task/task-group or ownership semantics
- be documented alongside the provider that owns it
- be versioned independently from the generic ABI when appropriate
- degrade cleanly when a host does not understand it

## What should stay out of the core ABI

The generic ABI should not absorb features that are specific to a single provider, such as:

- domain-specific operators
- expression DSLs
- serialization formats tied to one provider
- language-specific async runtime types
- scheduler tuning knobs that do not generalize across ports

## Promotion into the core

A provider or port feature is a candidate for the generic ABI only when it is:

- implemented independently in more than one port or provider
- useful to generic hosts rather than only to one provider API
- expressible without leaking language-specific runtime machinery
- stable enough that vendored consumers can rely on it

Until then, the feature should remain an extension outside the shared core.
