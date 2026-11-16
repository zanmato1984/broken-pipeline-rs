# Callback wait ownership and lifetime

`FutureAwaiter`, `ConditionalAwaiter`, `CoroAwaiter` and
`SingleThreadAwaiter` retain their input `SharedResumer`s. A registered callback
must hold only a weak backedge to its awaiter, not a strong `Arc`: otherwise an
unresumed input and its awaiter keep each other alive forever.

## Ownership and failure contract

- A successful constructor returns the only strong awaiter reference unless the
  caller explicitly clones it. Input resumers remain strongly owned by the
  awaiter for the existing `resumers()` API and protocol.
- Abandoning an unready awaiter drops its strong object and its input references
  without resuming any input. Externally held resumers remain usable.
- Constructor failure after registering callbacks on earlier valid inputs must
  release the partially constructed awaiter and all its strong input references.
  The existing `UnexpectedResumerType` error and validation order are preserved.
- An outstanding callback can retain a `Weak` and its allocation/control block
  until that callback is invoked or its resumer is dropped. Failure to upgrade
  proves strong-object destruction, **not** physical allocation reclamation,
  complete budget refund, admission control, or a full host/adapter solution.

## Resume and concurrent drop contract

Each invocation attempts a temporary strong upgrade before accessing the awaiter.
If the upgrade succeeds, it protects the object through readiness notification,
even if the caller concurrently releases its last external reference. If the
upgrade fails, notification is a no-op: an already destroyed object is never
resurrected. Once an in-flight successful upgrade is released, it cannot keep an
abandoned wait alive. A concurrent test must join callback threads before
asserting that the strong object is gone; external drop alone is not that proof.

The resumer still fires each registered callback once, including immediate
invocation when registration follows resume. Duplicate resumes remain no-ops.
Readiness counts registered inputs, not duplicate `resume()` calls; multiple
entries referring to the same input retain the existing registration semantics.
The constructor, `wait()`, `is_ready()` and `resumers()` signatures and behavior
are unchanged. Empty inputs and zero ready count retain their errors; a positive
ready count greater than the number of inputs remains permitted and may never
become ready. No input is force-resumed, discarded, or detached to break a cycle.

## Bounded regression evidence

The integration tests exercise all four actual public awaiter types: abandoned
never-resumed waits with retained external resumers; late and duplicate resume;
all handles dropped without resume; valid-first/wrong-second partial constructor
failure; ready-before-construction and multiple-ready semantics; and coordinated
bounded drop/resume races. Race threads are joined, with barriers/channels rather
than sleeps as evidence. Existing protocol tests remain intact.

The identical additive tests are compiled against original source commit
`07d9d34dc918242b5ffef1081895ac025819a399` for runtime-red controls, then against
the weak-backedge repair for green checks. A compile failure is not a valid red
control. These checks establish the scoped generic lifetime repair only; they do
not establish physical block release, SQL semantics, production admission, or
owner acceptance.
