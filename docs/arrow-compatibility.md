# Arrow 60.1.0 compatibility

## Goal and scope

Keep the real Broken Pipeline workspace at base
`62b816dbbb755af38776eb3988457fa3c3d87766` buildable with one coherent
arrow-rs **60.1.0** dependency identity. This is a generic dependency and
regression-test prerequisite, not an execution redesign or an application
integration. Both workspace crates remain enabled. Existing protocol tests and
expected results remain unchanged. The only implementation change permitted by
the subsequent CI compatibility repair is the behavior-equivalent continuation
error-propagation expression described below; all other protocol, executor and
scheduler implementation bytes are preserved from
`8261130f2b5951481f0b158ef19d821e3a291007`.

The original version of this document preceded the manifest, lockfile and
regression-test edits. This correction precedes the CI compatibility source edit.
It replaces the earlier blanket claim that all implementation files remain
unchanged; that claim describes the initial Arrow prerequisite, not the repaired
head.

## Dependency and API contract

- Pin `arrow-array = "=60.1.0"` and `arrow-schema = "=60.1.0"` in the
  core crate, and `arrow-schema = "=60.1.0"` in the optional scheduler crate.
  The scheduler's public Arrow error must be the same Rust type as the core
  binding's error.
- Generate and retain the actual workspace `Cargo.lock`, including registry
  checksums. Locked verification must not resolve a second Arrow major version.
  The original base snapshot had no lockfile; the initial prerequisite added
  one. The CI compatibility repair preserves that lockfile and every manifest
  byte; it does not regenerate dependencies to address a lint.
- Preserve `broken_pipeline::traits::arrow::ArrowTypes`: batch is
  `Arc<RecordBatch>`, error is `ArrowError`, context is `()`. No
  adapter-owned replacement batch, protocol or executor is introduced.
- Arrow physical layout, null validity and slicing retain Arrow's meaning.
  A pass-through batch keeps its `Arc`, column objects and backing buffers;
  it does not materialize/rebuild arrays. Schema equality alone is insufficient
  to prove Rust dependency or buffer identity.

Exact Arrow requirements intentionally reject silent minor upgrades. A later
upgrade requires an explicit dependency/lock change and the same checks.

## Mechanism and regression coverage

The regression uses real nullable Int32 and Utf8 arrays, creates sliced
RecordBatches with nonzero offsets, and passes multiple differently sized batches
through source -> pipe -> sink. Empty slices are batches, not end-of-input.
It constructs a real `Pipeline<ArrowTypes>`, invokes `compile`, obtains the
compiled stage's `PipeExec`, calls `task_group`, and executes that group's
existing `Task::run` until the protocol returns `Finished`.

The tests cover both legal source termination forms: a final
`Finished(Some(batch))` and a separate `Finished(None)` after streaming
batches via `SourcePipeHasMore`. Pipes return `PipeEven`; sinks return
`PipeSinkNeedsMore`. No callback treats `None` as a data NULL or invents
sink EOF. A bounded step count fails nontermination, and a post-finish task call
must remain finished without another callback or output.

Assertions compare exact ordered values, per-column NULL counts, per-batch and
total row counts, exact source/pipe/sink callback counts, schema, batch/column
Arc identity, and backing value/validity-buffer identity. Distinct batches,
a zero-row slice and a non-byte-aligned null slice detect skipped/duplicated
batches and offset/validity mistakes. This is transport/protocol regression,
not SQL computation. The CI compatibility repair changes no test or expectation.

## Diagnosed toolchain compatibility repair

Linux CI run
[43831350749](https://github.com/zanmato1984/broken-pipeline-rs/actions/runs/43831350749)
failed on source `8261130f2b5951481f0b158ef19d821e3a291007` during
`cargo clippy --workspace --all-targets --all-features -- -D warnings`.
The retained diagnostic identifies Rust 1.100.0 Clippy's `question_mark` lint at
`crates/broken-pipeline-schedule/src/naive_parallel_scheduler.rs:222`: the outer
`match continuation.run(&task_ctx)` returns `Err(error)` directly. The retained
raw log has SHA-256
`da8b39c9db42b209293f9baf01ea70f998ad94cd84511087a60fc112291782e9`.

Replace only that outer result match with
`let status = continuation.run(&task_ctx)?;` and keep its successful-status body,
dedented. `BpResult<TaskStatus, T>` is a standard `Result` with the same `T::Error`
on the call and enclosing function. An error therefore returns before locking
or appending to `statuses`, as before. Success still appends exactly one cloned
status before the same `TaskStatus` match: Continue/Yield yield, Blocked retains
the awaiter, and Finished/Cancelled return their original values. The task-group
error path, cancellation flag, readiness checks and scheduling loop are outside
this edit. This is static equivalence reasoning, not a runtime verification.

Do not suppress the warning, weaken `-D warnings`, disable CI, pin an older
toolchain to evade the lint, redesign the scheduler, or change expected tests.
The declared toolchain remains `stable`; selecting 1.100.0 for reproduction is
verification of the observed CI version, not a repository toolchain pin.

## Alternatives considered

Keeping Arrow 58 would leave consumers on incompatible Rust RecordBatch/error
identities. Allowing mixed 58/60 dependencies or changing only the core crate
does not establish interoperability. Broad semver requirements permit silent
dependency drift; exact pins plus a real lockfile make the tested identity
reviewable. Deleting the optional scheduler, replacing PipeExec, using a
fixture-only executor, or adding application semantics would evade the bounded
compatibility question and are excluded.

The exact CI evidence authorizes only the generic error-propagation rewrite
above, outside the original dependency-only implementation scope. Any additional
protocol or scheduler change requires its own evidence and authorization; this
exception does not permit a substitute implementation.

## Verification

Earlier reviews and checks tied to exact source
`8261130f2b5951481f0b158ef19d821e3a291007` remain historical evidence for that
revision and their recorded toolchains. They neither prove nor become fresh
passes for the repaired head. The failed Linux CI check remains a failure.
After integration, record the new exact head and obtain fresh independent code
review and verification, including Rust 1.100.0, before accepting compatibility.

Run on a patched copy of that exact head, retaining the original toolchain
declaration (`stable`, rustfmt and clippy):

1. Verify every source snapshot file's bytes/hash. Against `8261130`, allow only
   the documented continuation expression and associated documentation/report;
   preserve every other original source, test, manifest and lockfile byte.
2. Record actual toolchain versions and the unchanged lockfile/dependency pins.
   Do not regenerate the lockfile as part of this lint repair.
3. Run `cargo +1.100.0 fmt --all -- --check`.
4. Run `cargo +1.100.0 build --workspace --all-targets --all-features --locked`
   and `cargo +1.100.0 test --workspace --all-features --locked`, preserving all
   original tests. Also retain the CI workspace-test result.
5. Run `cargo +1.100.0 clippy --workspace --all-targets --all-features --locked
   -- -D warnings` and retain the corresponding Linux CI result for its exact
   command above, with warnings still denied.
6. Inspect `cargo +1.100.0 tree --workspace --locked`, duplicate identities, and
   inverse Arrow dependencies; record resolved Arrow package names, versions
   and checksums from lock/metadata.

This repair assignment is static only: no Cargo, compiler, dependency download,
build, tests or Clippy execution is authorized here. An installed standalone
`rustfmt --check` can check the changed Rust file's formatting/parser acceptance
but cannot
prove type checking, runtime behavior or Rust 1.100.0 lint acceptance. Required
compiled/runtime checks remain unrun until a separate verifier executes them.

Retain actual commands, exit codes, full outputs (with any display-path
redaction explicitly marked), exact artifact hashes and unrun scope in the
task report. A version/tool-driven pre-existing failure is not a passed check;
diagnose it without unrelated source edits.

## Limitations and remaining gaps

The reference `PipeExec::step` still holds its shared execution-state mutex
while invoking callbacks. A DOP/task count is not measured callback parallelism.
The existing scheduler facades do not establish bounded CPU/I/O pools,
completion-driven readiness, fairness, cancellation races or host admission.
The error-propagation rewrite does not repair these gaps.

Arrow arrays and this test do not establish hard-limit memory enforcement,
pre-allocation admission, shared-buffer budget deduplication, or host integration.
No business functions/operators, stage dependency/finalization proof, FFI
ownership tests, performance acceptance or production readiness are claimed.
Compatibility results require independent code review and verification; they
do not grant an owner gate.
