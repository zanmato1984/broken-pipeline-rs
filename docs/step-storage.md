# Bounded task-step container storage

`compile` -> `Pipelinexe::pipe_exec` -> `TaskGroup` -> `Task::run` remains the
execution path. Construction reserves blocked-resumer scratch for the actual
number of channels, independently for every logical task. At most one resumer
per channel is appended in one step. A structural assertion precedes each push.
The scratch vector is cleared by an unwind-safe guard on every return: continue,
blocked, yield, finished, returned error, cancellation, and panic. Its borrowed
slice is valid only during the factory call. Parked per-channel resumers remain
owned until resumed, fatal cancellation is observed by that task, or drop.
This includes reentry after another task panics: an idle parked task acquires
only its own slot, installs fatal cleanup, and then preserves the poison panic
without invoking callbacks. An already-poisoned reentry never waits for an
occupied slot; the active entry retains responsibility for its own cleanup.
Other parked slots are not globally cleared before they reenter or are dropped.

Every channel continuation stack is reserved at executor construction for its
actual pipe count. Its indexes are strictly increasing: forward traversal pushes
in increasing order, resume pops the largest index before pushing that same or
later index, and drain starts only with the stack empty. Thus depth is at most
the pipe count, including nested has-more, blocked and yield handshakes. Each
push checks index range, order and depth before it can grow storage. Stack
entries are indexes, not borrowed callback data. A continuation is retained
across successful steps; fatal cleanup discards it without invoking callbacks.

## Additive context API

`BorrowedAwaiterFactory<T>` accepts `&[SharedResumer]`, and
`TaskContext::from_shared_context_with_borrowed_awaiter` selects this path.
`make_awaiter_from_slice` does not copy its input on this path. The result remains
an independently owned, static `SharedAwaiter`; it cannot retain the scratch
borrow. A factory may clone handles into its own storage or return an already
created awaiter with independently owned resumers. It may not extend lifetimes.

The owned `AwaiterFactory`, both old constructors, `make_awaiter(Vec)`, cloning,
input IDs and context identity remain compatible. Calling the slice API on a
legacy context explicitly converts to an owned Vec and may allocate. Calling
the owned API on a borrowed context passes a slice of that caller-owned Vec.

## Limits and evidence

This removes only executor scratch/continuation container growth after
construction on the borrowed path. Compile, constructors, factories, Arc/Box,
strings, callbacks, native locks and the allocator can still allocate or fail.
Stable capacity/backing identity is not proof of zero heap requests. Tests
observe actual cold first-use Task::run heap requests, an allocating positive
control, repeated borrowed slice identity, internal constructor/step capacities,
real protocol traces and runtime-failing safe mutants. Native lock first-use
allocations, if observed, must be reported rather than warmed away.

This is not host admission control, ManagedReady, a full TiForth adapter,
performance acceptance, or resolution of TiForth memory issues M07/M08. Shipped
scheduler awaiter ownership is separately addressed; these tests can return a
precreated awaiter and never assume that separate repair has been integrated.

## Observed builder-only first-use requests

On the recorded macOS/Rust 1.100.0 run, the first actual blocked Task::run
requested one 64-byte standard-heap allocation. A separate untouched native
Mutex first lock requested the identical one 64-byte allocation; the executor
lock was not warmed before measurement. Repeated blocked steps requested zero.
The explicit legacy slice-to-Vec conversion requested one 32-byte allocation
for two trait-object Arc handles. The allocating positive requested 257 bytes.
These are observations for this source/toolchain/host, not a portable promise
that native locks never allocate or a claim that all metadata is admitted.
