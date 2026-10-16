---
name: flux
description: Write and debug Flux refinement-type annotations on Rust crates — `#[flux_rs::spec]`, `refined_by`, `invariant`, `assoc`, `extern_spec`, `defs!`, `property`, `trusted`. Use this whenever a task involves verifying Rust with Flux — adding or fixing specs, reading a refinement type error, deciding where a precondition belongs, modeling raw pointers or bit-packed fields, giving a trait impl an associated refinement, or writing an extern spec for a std type. Also use it when a spec ought to verify but does not, when flux reports SAFE but you suspect nothing was checked, or when you are about to reach for `#[flux_rs::trusted]` — the traps reference usually has the real cause. Trigger on flux, refinement type, liquid types, refined_by, flux_rs, liquid-fixpoint, or a `cargo flux` or `run_flux.py` error.
---

# Flux refinement types

Flux checks Rust against logical predicates discharged by an SMT solver. Your job
is usually to find the **spec that is both true and strong enough for callers** —
not to make errors go away.

## Non-negotiables

**Run flux only through a timeout-bounded harness.** A bare `cargo flux` can spin
for 10+ minutes on a solver blow-up with no output. In the centrifuge repo that is
`python3 ../../harness/run_flux.py` (`--full`, `--check-def=<substr>`,
`--check-uid=N`, `--check-type=N`). If a project has no such wrapper, write one
before you start: run cargo in a fresh process group so a timeout kills the
`flux-driver` children, which otherwise orphan and hold the target-dir lock.

**Never add `trusted` / `ignore` to make progress.** They are the one thing that
silently converts "unverified" into "looks verified". Reach for the traps
reference first (`references/traps.md`) — most apparent limitations are a
resolution or syntax issue with a one-line fix. If an annotation is genuinely
needed, write a `reason = "..."` that you have *tested*, and log it.

**Test every claim before you write it down.** This is the single highest-value
habit. Reasons inherited from a previous pass, or inferred from reading the code,
are wrong at a startling rate — in one crate essentially *every* un-retested
`reason` string turned out to be false or to name the wrong cause. Remove the
annotation, re-run, read the actual error.

**Clear stale metadata after flux itself is rebuilt**: `rm -rf target/flux
target/FLUXCACHE`. Symptoms otherwise are impossible errors in untouched code.

## Order of work

**On a new crate, start at `references/setup.md`** — wiring up `Cargo.toml` and
the crate root, establishing a green baseline on the *unannotated* source, and
generating the dependency-ordered work list. Skipping the baseline is the
classic mistake: without it you cannot tell your errors from pre-existing ones,
and an ICE or a lib/driver version skew will masquerade as a bad spec.

The short version:

```bash
python3 <harness>/run_flux.py --full          # must be RESULT=SAFE before you start
python3 <harness>/run_flux.py --make-targets  # -> log/{call,type}_graph.json
python3 <harness>/make_targets.py .           # -> log/{targets,type_targets}.json
python3 <harness>/run_flux.py --check-type=<uid>   # phase 1, in listed order
python3 <harness>/run_flux.py --check-uid=<uid>    # phase 2, in listed order
```

Then bottom-up, because a spec is only as useful as its callees' specs:

1. **Types first.** `#[refined_by(..)]` + `#[invariant(..)]` on the structs.
   Pick the index set from the *representation invariant* (`len <= cap`,
   `ptr` is in-bounds), not from whatever a single function happens to need.
2. **Callees before callers.** Before writing a spec, list the callees and read
   each one's existing spec. The error is usually that a callee's postcondition
   is weaker than this function needs — fix it there.
3. **One function at a time**, to a clean pass or a logged abort.

## When a spec will not verify

Work through these in order. The first three fix the large majority of cases.

1. **Ask what the producer already knows, not what the local guard can prove.**
   This is the move that unsticks most hard cases. A function receiving a pointer
   cannot re-derive which allocation it came from; the function that *built* it
   can. Push the fact to a precondition and let the caller discharge it.
2. **Prefer a precondition over an axiom.** A `requires` stays visible, is checked
   at every call, and is very often dischargeable by callers you have not looked
   at yet. A `trusted` axiom hides the obligation forever. When you think a fact
   is unprovable, push it to the *outermost* function that needs it and see who
   can supply it — the surprise is usually that someone can.
3. **Check whether one discarded call is a proof.** `let _ = b.remaining();`
   before the code that needs `remaining`'s bounds turns an "axiom" into a
   checked body, because the call's postcondition enters the environment. Same
   idea for any fact a callee already promises.
4. **Bundle co-dependent accessors.** Three separate calls (`vec.len()`,
   `vec.capacity()`, `vec.as_mut_ptr()`) give three unrelated results. One helper
   returning a tuple under a shared binder relates them:
   `-> (usize[n], usize[#cap], *mut{p: cap <= p.size} T) ensures n <= cap`.
5. **Strengthen an upstream `refined_by` / `invariant`**, or add an intermediate
   callee spec to bridge the gap.
6. Only then consider that it may be a real limitation — and confirm it with a
   minimal standalone file before you believe yourself.

## An assumption is never contradicted by its own use site

A `trusted` helper that publishes a fact is invisible to every check that
depends on it — the sweep stays green whether the fact is true or not. So the
usual signals do not apply, and these are worth a second look even when
everything passes:

- **A fact asserted of a type that has several representations.** If the value's
  behaviour depends on which variant / vtable / tag it carries, an unguarded
  fact is almost certainly too strong for one of them. Replace it with a
  discriminator (`references/modeling.md`) — the exercise of naming *when* the
  fact holds is what exposes the over-strong version.
- **A fact about runtime-mutable state.** "The word points into the buffer" can
  be true at construction and false after a promotion. The guard needs the state
  too, not just the representation.
- **A precondition that is false rather than merely unproven.** Ask whether
  there is any reachable call where it fails. If so, the helper is unsound, not
  pragmatic.

When a fact must be assumed, prefer the smallest, most auditable form: a named
one-line helper with a pure `ensures` over a `trusted` on a large body, and a
checked annotation over an assumed one where both exist (see
`references/annotations.md` on statics vs consts — `spec` on a `static` is
checked, `flux::constant` on a `const` is not).

## Verify that you are actually checking something

Flux reporting `RESULT=SAFE` is not evidence that your target was checked. A
`--check-def` filter that matches nothing reports SAFE. A `trusted` you forgot
reports SAFE. An unreachable branch reports SAFE.

**Canary test**: inject a statement that must fail, and confirm it does.

```rust
let _c = core::hint::black_box(0usize) - 1;   // expect: arithmetic operation may underflow
```

No error means the body is not being checked — find out why before continuing.
Use this whenever a tricky function starts passing, and whenever a filter-based
run disagrees with a full sweep. Prefer module-prefix filters
(`--check-def=bytes_mut::`); filters containing spaces or angle brackets
(`'Buf for std::io::Cursor'`) match nothing silently.

## Solver timeouts

A timeout is fixpoint/Z3 failing to converge, not a spec bug. Do **not** weaken
the spec or hunt for a missing precondition. Two cheap self-inflicted causes to
undo first, both of which keep the function checked:

- a new `qualifier`, especially 3- or 4-argument (predicate abstraction is
  combinatorial in arity) — narrow it or drop it
- a refinement function applied inside an `#[flux_rs::assoc(..)]` body

Otherwise mark `#[flux_rs::trusted(reason = "solver timeout")]` and **keep the
`#[flux_rs::spec]`** so callers still use it; only the body is skipped. After a
*full sweep* timeout, re-run candidates individually — a sweep gives no
per-function attribution, so trust only the ones that time out on their own.

## Recording what you could not do

Keep a log (e.g. `log/ABORT.md`) with a category→count table and a
category→functions list. Key entries by file and function name, not line
numbers — those go stale on every edit. For each entry record the uid/span, the
verbatim flux error, and which of the strategies above you tried and why each
failed. Re-test the whole log periodically: limitations get fixed, and a stale
"flux cannot do X" is worse than no note at all.

## References

Read the one you need; they are independent.

- **`references/setup.md`** — bootstrapping a crate: dependencies, crate-root
  attributes, baseline, work-list generation, the per-target loop, and keeping
  the flux driver and libs in step. Read this first on any crate you have not
  worked on before.
- **`references/annotations.md`** — the full annotation catalog with working
  syntax: `spec`, `refined_by`, `invariant`, `field`, `assoc`, `extern_spec`,
  `defs!`, `property`, `trusted`, `opts`, statics, existentials, `&strg`,
  refined function pointers. Start here when you need the right syntax.
- **`references/traps.md`** — symptom → cause → fix for the failures that look
  like flux limitations but are not. Read this *before* concluding anything is
  impossible. Includes the `&mut self` blanket-impl trap, crate-root-only
  `property`, trait impls needing restated specs, sort-vs-type generics, and the
  known hard walls with their exact error text.
- **`references/modeling.md`** — how to model raw pointers (`{base, addr, size}`),
  bit-packed fields via `property` axioms, allocation-end reasoning, and trait
  associated refinements with uninterpreted defaults.
- **`examples/syntax_tour.rs`** — every form in the catalog in one file that
  verifies clean, plus `examples/syntax_tour_unsafe_fnptr.rs` for the newer
  `unsafe fn` pointer fields. Run them against the **same driver the project
  uses**, not whichever one `cargo xtask run` builds, and do it before trusting
  the docs: flux moves, and a form that was a wall last month may not be one
  now. If the tour and the project disagree, suspect driver/checkout drift
  before suspecting the docs — `ls -l ~/.cargo/bin/flux` against the checkout's
  HEAD, since `cargo flux` uses the installed binary while `cargo xtask run`
  builds the current tree.
