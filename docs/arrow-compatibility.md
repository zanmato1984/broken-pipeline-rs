# Arrow 60.1.0 compatibility

## Goal and scope

Keep the real Broken Pipeline workspace at base
`62b816dbbb755af38776eb3988457fa3c3d87766` buildable with one coherent
arrow-rs **60.1.0** dependency identity. This is a generic dependency and
regression-test prerequisite, not an execution redesign or an application
integration. Both workspace crates remain enabled. Existing protocol tests and
all protocol, executor and scheduler implementation files remain unchanged.

This document precedes the manifest, lockfile and regression-test edits.

## Dependency and API contract

- Pin `arrow-array = "=60.1.0"` and `arrow-schema = "=60.1.0"` in the
  core crate, and `arrow-schema = "=60.1.0"` in the optional scheduler crate.
  The scheduler's public Arrow error must be the same Rust type as the core
  binding's error.
- Generate and retain the actual workspace `Cargo.lock`, including registry
  checksums. Locked verification must not resolve a second Arrow major version.
  The base snapshot has no lockfile; this adds one rather than asserting an
  existing resolution was preserved.
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
not SQL computation.

## Alternatives considered

Keeping Arrow 58 would leave consumers on incompatible Rust RecordBatch/error
identities. Allowing mixed 58/60 dependencies or changing only the core crate
does not establish interoperability. Broad semver requirements permit silent
dependency drift; exact pins plus a real lockfile make the tested identity
reviewable. Deleting the optional scheduler, replacing PipeExec, using a
fixture-only executor, or adding application semantics would evade the bounded
compatibility question and are excluded.

If Arrow 60 needs protocol or scheduler source changes beyond dependency
requirements, stop at the exact compiler evidence and report the blocker;
do not hide it behind a substitute implementation.

## Verification

Run on a patched copy of the exact source snapshot, with the original
toolchain declaration (`stable`, rustfmt and clippy):

1. Verify every snapshot file's bytes/hash against the source manifest, then
   verify that all original tests and implementation files are unchanged.
2. Record toolchain versions; generate the lockfile from the exact requirements.
3. Run `cargo fmt --all -- --check`.
4. Run `cargo test --workspace --locked`, preserving all original tests.
5. Run `cargo clippy --workspace --all-targets --locked -- -D warnings`.
6. Inspect `cargo tree --workspace --locked`, duplicate identities, and inverse
   Arrow dependencies; record resolved Arrow package names, versions and
   checksums from lock/metadata.

Retain actual commands, exit codes, full outputs (with any display-path
redaction explicitly marked), exact artifact hashes and unrun scope in the
task report. A version/tool-driven pre-existing failure is not a passed check;
diagnose it without unrelated source edits.

## Limitations and remaining gaps

The reference `PipeExec::step` still holds its shared execution-state mutex
while invoking callbacks. A DOP/task count is not measured callback parallelism.
The existing scheduler facades do not establish bounded CPU/I/O pools,
completion-driven readiness, fairness, cancellation races or host admission.
No source change in this prerequisite repairs these gaps.

Arrow arrays and this test do not establish hard-limit memory enforcement,
pre-allocation admission, shared-buffer budget deduplication, or host integration.
No business functions/operators, stage dependency/finalization proof, FFI
ownership tests, performance acceptance or production readiness are claimed.
Compatibility results require independent code review and verification; they
do not grant an owner gate.
