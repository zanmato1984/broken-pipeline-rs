# Versioning

## Version axes

This repository distinguishes three different version axes.

### Generic ABI version

The generic ABI version is defined by the header macros:

- `BP_ABI_VERSION_MAJOR`
- `BP_ABI_VERSION_MINOR`
- `BP_ABI_VERSION_PATCH`
- `BP_ABI_VERSION_HEX`
- `BP_ABI_DRAFT`

This version describes the shared host/provider contract in this repository.

### Provider API version

Each provider also has its own API name and version. That version belongs to the provider's
concrete operator/expression/build API, not to the generic ABI.

### Repository revision

Ports and providers are expected to vendor or pin a specific repository revision. A pinned
commit or tag is part of the practical compatibility contract even when the ABI version is
unchanged.

## Compatibility rules

- **Patch**: documentation clarifications and non-semantic fixes.
- **Minor**: backward-compatible additions such as new optional flags or additive surface
  area that older consumers can safely ignore.
- **Major**: breaking layout, symbol, semantic, or ownership changes.

If the provider discovery symbol changes incompatibly, it should move to a new symbol name
rather than silently reusing `bp_query_provider_v0`.

## Draft expectations

This repository starts in draft form. Draft status does not mean "anything goes".
Breaking changes should still be explicit, documented, and versioned conservatively.

## Pinning guidance

Ports and providers should vendor or pin this repository at a known commit or release.
They should not depend on an unpinned branch tip for production integrations.
