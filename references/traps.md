# Flux traps: symptom → cause → fix

Read this before concluding that anything is impossible. Everything in the first
section looked like a flux limitation and was not; each cost hours before the
real cause turned up.

## Contents

- [Fixable: looks like a limitation, isn't](#fixable)
- [Silent: no error, nothing checked](#silent)
- [Real walls, with exact error text](#real-walls)
- [Soundness and style](#soundness-and-style)

## Fixable

### `self.method()` in a `&mut self` body proves nothing

**Symptom.** `assert!(len <= self.remaining())` tells flux nothing; a following
`self.limit -= len` reads as a possible underflow. Or an `#[assoc]` you just
defined appears to be ignored.

**Cause.** With a `&mut self` receiver, method resolution autorefs to
`&&mut Self` and binds a blanket impl such as
`impl<T: Buf + ?Sized> Buf for &mut T`, whose `Self` is a *reference* and so
carries the unit sort. Your concrete impl is never selected.

**Fix.** UFCS: `<Self as Buf>::remaining(self)`. This one trap accounted for six
separate "limitations" in a single crate. Suspect it whenever a refinement that
obviously should be in scope is not.

### A trait impl supplies `#[assoc]` but its methods still fail

**Symptom.** The impl has the right `#[flux_rs::assoc(fn remaining(..))]`, and
`remaining()` still will not verify.

**Cause.** The `assoc` definition and the *method specs* are separate
obligations. Inheriting the trait's spec leaves `Self` abstract, so the method's
result is related to the abstract associated refinement, not your definition.

**Fix.** Restate the trait's spec on each method with the concrete `Self`:

```rust
#[flux_rs::spec(fn(&Chain<T, U>[@c]) -> usize[<Chain<T, U> as Buf>::remaining(c)])]
fn remaining(&self) -> usize { .. }
```

Compare a trait impl that already works in the same codebase — the difference is
usually exactly this.

### A `property` has no effect

**Symptom.** Bit-packing predicates behave as if the `property` were absent;
functions over them hang or fail inexplicably.

**Cause.** `property` items only take effect at the **crate root**. Inside a
submodule's `defs!` they parse and are silently ignored. (`defs!` *functions* are
crate-global from anywhere, which makes this inconsistency easy to miss.)

**Fix.** Move every `property` to `src/lib.rs`.

### "parameter cannot be determined"

**Cause.** Every `refined_by` parameter needs a field whose index position fixes
its value. Appearing in an `invariant`, a `requires`, or inside a
function-pointer field type does not count.

**Fix.** Index a field with it, or drop it and express the fact another way.

### Reading a `static` or `const` gives an arbitrary value

**Cause.** Neither one's index is inferred from its initializer.

**Fix.** `#[flux_rs::spec(S[1])] static ONE: S = S { k: 1 };` for a static —
*checked* against the initializer, so sound. For a `const`,
`#[cfg_attr(flux, flux::constant(S { k: 1 }))]` — but that one is **assumed**, so
it is a one-line `trusted` in disguise: a wrong value errors at the readers, not
on the const. `#[flux_rs::trusted]` on either does nothing.

Prefer a `static`. A `const` is forced when a `const fn` reads it, and a static
whose initializer reads a const inherits the const's unknown index.

### A closure fails a refined fn-pointer coercion

**Fix.** Hoist it to a named `fn` item with the spec written out. The identical
body then verifies. Closure→fn-pointer coercions are not checked against refined
fn-pointer types.

### `&mut T[@b]` locks the callee out of mutating

**Cause.** An indexed `&mut` makes the index invariant: the post-state must equal
`b`.

**Fix.** `-> {k. &mut T[k]}` for an accessor the caller must be able to mutate,
or `&strg` + `ensures` when you want to *describe* the post-state.

### Sort position vs type position

A `refined_by` struct used as a **sort** takes no generics (`me: VecDeque` in an
`#[assoc]`); in a **type** position it takes all of them
(`&VecDeque<u8, alloc::alloc::Global>[@d]`). Mixing these up produces confusing
arity errors.

### An extern_spec target is rejected

Generic parameter **names** must match the external definition
(`FromIterator<T>`, not `FromIterator<A>`), and only stable paths resolve —
`core::io` is unstable and rejected, `std::io` is the same `DefId` and works.

## Silent

### `RESULT=SAFE` is not evidence

A `--check-def` filter that matches nothing reports SAFE. Filters containing
spaces or angle brackets (`'Buf for std::io::Cursor'`) match nothing *silently*.
A forgotten `trusted` reports SAFE. An unreachable branch reports SAFE.

**Canary**: inject a statement that must fail and confirm it does.

```rust
let _c = core::hint::black_box(0usize) - 1;   // expect: arithmetic may underflow
```

No error means nothing is being checked. Use module-prefix filters
(`--check-def=bytes_mut::`), and when a filtered run disagrees with a full sweep,
believe the sweep.

### Stale metadata after flux is rebuilt

Impossible errors in untouched code. `rm -rf target/flux target/FLUXCACHE`.

### An unspecced callee is silently unrefined

A function with no `#[flux_rs::spec]` gets an unrefined signature, so its results
carry no information. A postcondition that "cannot be proved" for no apparent
reason is often a `Vec::from_raw_parts` or similar with no spec anywhere. Adding
a *precondition-free* extern spec that records only the length is cheap and
creates no obligations at the call sites.

## Real walls

Confirmed with minimal standalone files. Re-test before trusting — these get
fixed.

| want | error |
|---|---|
| `refined_by` param used only in a fn-pointer field | `parameter `x` cannot be determined` |
| caller-chosen output index, `-> T[@base]` | `illegal binder` |
| `requires` on a fn-pointer **parameter** type | `syntax error` — use `&{T[@q] \| pred}` instead, see annotations.md |
| named params in a fn-pointer type | syntax error |
| `requires` inside `#[field(fn(..))]` | **none — silently ignored** (see below) |
| `#[flux::constant(..)]` on a `const` | works, but **assumed**, not checked |

### `requires` in a fn-pointer field type is silently ignored

The nastiest of these, because there is no error. A `requires` inside a
`#[field(fn(..))]` parses, then does nothing: it neither binds callers nor
discharges the target function's own `requires` at the coercion. Use a
constrained argument type instead — `#[field(fn(&S{v: v.n > 0}) -> S)]` — which
does both.

Also structural: raw pointers are `{base, addr, size}` with **no fat-pointer
slice length**, so a `&mut [u8] -> *mut [u8] -> &mut [u8]` round-trip loses the
length even at the same type. Route through a `from_raw_parts`-style call that
takes the length explicitly. And `ptr as usize`
(`CastKind::PointerExposeProvenance`) discards the index — wrap it in a trusted
shim `fn ptr_as_usize<T>(p: *const T) -> usize[p.addr]`.

## Soundness and style

### Preconditions beat axioms

A `requires` stays visible, is checked at every call, and is often dischargeable
by a caller you have not examined. A `trusted` axiom hides the obligation
forever. When a fact seems unprovable, push the precondition to the *outermost*
function that needs it: that exposes precisely the condition under which the
operation is safe, and the usual surprise is that someone upstream can supply it.

### An "axiom" is often one discarded call

`let _ = b.remaining();` ahead of the code that needs `remaining`'s bounds makes
the body a proof rather than an assumption, because the call's postcondition
enters the environment. Check this before writing any `trusted`.

### Localize an unavoidable assumption

If a fact really must be assumed, put it in one small named helper with a pure
`ensures` and call it where needed, rather than marking a large body `trusted`:

```rust
#[flux_rs::trusted(reason = "<the representation invariant, stated and justified>")]
#[flux_rs::spec(fn(data: *mut[@sp] (), ptr: *const[@p] u8, len: usize)
                ensures valid_buf(p, len) && derived_from(p, sp))]
pub fn assume_contract(data: *mut (), ptr: *const u8, len: usize) { let _ = (data, ptr, len); }
```

One assumption, named and greppable, with every real body still checked.

### Reverting

Revert from copies you made yourself, never `git checkout --` on a file with
other uncommitted work. And note that a *rejected* Bash call may already have
applied its edits — re-read the file rather than assuming the denial undid it.
