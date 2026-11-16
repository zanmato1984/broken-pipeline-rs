# Task-local callback concurrency

`PipeExec` keeps the existing `compile` → `Pipelinexe::pipe_exec` →
`TaskGroup` → `Task::run` execution path and public callback protocol. This is a
generic executor change, not a replacement scheduler or a query-specific adapter.

## Ownership and exclusion

Previously, one `Arc<Mutex<ExecState>>` protected every channel and every task,
including all user source, pipe, sink and drain callbacks. Increasing DOP could
not make callbacks in that executor overlap.

The immutable channel topology (operator Arcs, input IDs and drain indexes) is
shared. A fixed vector has one mutex-protected mutable runtime per logical task.
Each runtime owns that task's per-channel finished bits, resumers, sink resume
flag, pipe continuation stack, source completion, drain position and yield
handshake. A call holds only its logical task's mutex across callbacks. Distinct
tasks can therefore overlap; concurrent entries for the same task serialize and
cannot consume the same continuation twice. Reentrant invocation of the same
task from its callback is unsupported and can deadlock, as before.

Callback `ThreadId` remains the existing logical `TaskId` converted to the
callback ID type, **not** the current OS thread's identity. Tasks may migrate
between OS threads without moving state slots. Channel `input_id` is retained
on initial, continuation, blocked-resume and drain calls. Operators remain
shared `Send + Sync` objects: their own shared mutable data still needs the
operator's synchronization. No executor lock is transferred to a shared operator.

Topology storage is O(channels + pipes); mutable storage is O(DOP × channels)
plus each task's continuation stack (bounded by its channel's pipe topology).
There are no DOP copies of an already per-DOP runtime. Each `pipe_exec()` creates
an independent executor; clones/task groups of that executor share its state.

## Fatal errors, in-flight work and panics

A shared atomic fatal-cancel bit is set on a returned execution error. Every
task checks it after obtaining its own mutex, before each operator
callback and after callbacks/awaiter creation. Once observed, cancellation
prevents further callbacks in that execution path. Sequential calls after an
error return `TaskStatus::Cancelled`, preserving the original invariant.

Cancellation is cooperative, not forcible interruption. A callback admitted by
a cancellation check can race with another task's error and finish afterward.
Its returned batch/resumer is dropped rather than passed to a new callback when
cancellation is observed. A sink already running may have published a result;
BP cannot roll that back. A callback can itself retain resources, so BP does not
promise application cleanup inside that callback. Remaining runtime-owned
resumers are released when their task next observes cancellation, or when the
executor is dropped. Values are moved/dropped normally, not cloned or replayed.
The host must separately implement terminal/publication arbitration, waking
blocked tasks, semantic cleanup and shutdown. This bit is not a host terminal gate.

Panics and invalid callback outputs still unwind; they are not converted into
successful completion or SQL errors. An unwind guard marks the executor
cancelled and records shared poison before a task mutex is poisoned. Subsequent
steps for **any** task still panic with the original executor-wide fail-fast
policy; an already in-flight task also fails fast when it observes the poison
on return. No poisoned state is recovered. This does not catch panics or guarantee recovery with
`panic=abort`.

The legal source/pipe/sink/drain outputs, `None` continuation inputs,
`PipeYield`/`PipeYieldBack` handshake and source-finished-before-drain ordering are
unchanged. No scheduler, dependency, Arrow representation or public API changes
are part of this patch. This work does not establish memory admission, host
pool bounds, performance targets, complete adapter cancellation or an owner gate.

## Verification

The dedicated integration tests execute the actual compiled task machinery on
separate OS threads. Bounded condition-variable rendezvous hold callback entries
until the test driver observes overlap and releases them. All releases occur
before assertions; the rendezvous deadline only detects unavailable overlap,
not a speed target. Source, pipe, sink and drain are checked separately, with
exact task/input identities and output values. Additional cases check
multi-channel continuations, same-task exclusion and an in-flight callback
finishing after a fatal error without downstream publication. The identical
source-overlap test also runs against the pinned original serialized executor
as a controlled failing negative control. Raw commands/results are retained in
the task report; this document alone is not a claim that checks have passed.
