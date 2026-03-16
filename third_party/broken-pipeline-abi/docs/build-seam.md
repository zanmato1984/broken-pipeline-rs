# Build Seam

## Purpose

The build seam covers construction-time interactions between a host and a provider. Its
job is to transform a provider-defined request into an executable artifact that can later
be bound into the generic runtime seam.

## Why construction is separate from execution

Construction/build-time APIs are separate because they are richer and more provider-specific
than the runtime execution contract.

- Providers define concrete operator and expression APIs.
- Providers choose their own plan or payload formats.
- Providers may validate, optimize, or lower plans in provider-specific ways.
- Hosts often want to cache or transport executable artifacts independently of execution.

Keeping build-time APIs separate prevents the generic runtime ABI from becoming polluted by
provider-specific graph construction details.

## Generic responsibilities at build time

The generic build seam is intentionally narrow. It covers:

- ABI-compatible negotiation between host and provider
- a provider API name/version alongside the generic ABI version
- opaque request payload ownership rules
- production of an executable artifact for the runtime seam
- diagnostic reporting during build-time lowering

## Provider-specific responsibilities

Providers must define their concrete build APIs in their own repositories. That includes:

- operator catalogs
- expression builders
- schemas and type systems
- serialized request formats
- optimizer controls
- provider-specific validation rules

Those provider-specific APIs must depend on the corresponding language-specific Broken
Pipeline port. This repository does not replace that dependency; it only defines the
shared generic seam around it.

## Host options

A host can interact with the build seam in more than one way:

- call a provider-specific API directly and then enter the generic runtime seam
- use a generic build adapter that speaks the provider's request format
- skip build-time entirely when consuming a prebuilt executable artifact

In every case, the provider-specific request semantics remain outside the generic ABI.
