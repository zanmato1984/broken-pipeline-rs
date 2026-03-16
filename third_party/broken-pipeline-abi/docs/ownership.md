# Ownership

## Core rule

The generic ABI must never rely on ambient allocator assumptions such as "the other side
can always call `free()`". Ownership must be explicit at every host/provider boundary.

## Borrowed views

The draft header uses borrowed views for non-owning references:

- `bp_string_view`
- `bp_bytes_view`

A borrowed view does not transfer ownership. The producer keeps the storage alive for the
required call or object lifetime documented by the API.

## Owned or shared payloads

When ownership crosses the boundary, the draft header uses:

- `bp_string`
- `bp_buffer`

These pair a view with:

- an owner tag (`BORROWED`, `HOST`, `PROVIDER`, or `SHARED`)
- an optional release callback
- an optional release context

This keeps memory management explicit and avoids forcing one allocator or runtime onto all
ports.

## Wait handles and interface objects

The runtime draft uses `bp_awaiter_iface` for blocked work. Implementations should follow
these rules:

- a blocked task must return a valid wait handle
- a non-blocked task should return a zeroed or otherwise inert wait handle
- `retain` and `release` govern the handle lifetime when implemented
- hosts must not guess ownership; they must follow the exposed callbacks

## Error ownership

`bp_error.message` follows the same ownership rules as any other transferred string. The
consumer must release it only when a release callback is supplied.

## Threading note

The generic ABI does not assume that every handle is thread-safe. Each port or provider
must document any thread-affinity or synchronization requirements for the handles it
returns.
