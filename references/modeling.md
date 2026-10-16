# Modeling patterns

How to express the three things that come up constantly in low-level Rust: raw
pointers, bit-packed words, and trait-generic quantities.

## Contents

- [Raw pointers](#raw-pointers)
- [Allocation-end reasoning](#allocation-end-reasoning)
- [Bit-packed fields](#bit-packed-fields)
- [Trait-generic quantities](#trait-generic-quantities)
- [Relating results that are fetched separately](#relating-results-that-are-fetched-separately)

## Raw pointers

Flux's `ptr` sort has three fields: `base` (the allocation's start), `addr` (the
current address), `size` (bytes remaining from `addr`). There is **no**
provenance, no alignment-from-low-bits, and no fat-pointer length.

Name the recurring predicates in `defs!` rather than inlining them — they end up
in dozens of signatures:

```rust
flux_rs::defs! {
    // `p` is inside its allocation with room for `len` more bytes: what
    // `slice::from_raw_parts(p, len)` and `p.add(len)` need.
    fn valid_buf(p: ptr, len: int) -> bool {
        p.addr >= p.base && p.size >= 0 && len <= p.size
    }

    // `q` was derived from `p` by pointer arithmetic: same allocation, both in
    // bounds. This is what `q.offset_from(p)` requires.
    fn derived_from(q: ptr, p: ptr) -> bool {
        q.base == p.base && q.addr >= p.addr && q.addr >= q.base
            && q.size >= 0 && p.addr >= p.base && p.size >= 0
    }

    fn at_base(base: int, addr: int, size_of: int, align_of: int) -> bool { .. }
}
```

Then an invariant ties them to the struct, and functions take them as
preconditions:

```rust
#[flux_rs::refined_by(ptr: ptr, len: int)]
#[flux_rs::invariant(valid_buf(ptr, len))]
pub struct Bytes { #[field(*const [ptr] u8)] ptr: *const u8, .. }

#[flux_rs::spec(fn(buf: *mut[@b] u8, offset: *const[@o] u8, len: usize)
                requires derived_from(o, b) && valid_buf(o, len))]
unsafe fn free_boxed_slice(buf: *mut u8, offset: *const u8, len: usize) { .. }
```

`cast` is not modeled by default; a precondition-free extern spec recovers it:

```rust
#[flux_rs::extern_spec(core::ptr)]
impl<T> *mut T {
    #[flux_rs::spec(fn(me: *mut[@p] T) -> *mut[p] U)]
    fn cast<U>(self) -> *mut U;
}
```

## Allocation-end reasoning

`addr + size` is the allocation's end, and `add`/`sub` preserve it. That makes it
the right currency for "these two pointers are in the same allocation with a
known relationship" — stronger and more usable than `base` equality alone.

A masking operation only lowers the address and never changes the allocation, so
its trusted spec can honestly say all three things:

```rust
#[flux_rs::trusted(reason = "provenance is preserved across the address mask")]
#[flux_rs::spec(fn(ptr: *mut[@p] u8) -> *mut{q: q.base == p.base && q.addr == q.base
                                              && q.size >= 0 && q.addr <= p.addr
                                              && q.addr + q.size == p.addr + p.size} u8)]
fn untag_vec_base(ptr: *mut u8) -> *mut u8 { .. }
```

With `q.addr + q.size == p.addr + p.size` in hand, `len <= p.size` transfers to
`len <= q.size` by arithmetic. Without it you are stuck needing a fresh
precondition at every step.

When two callers need **incompatible** facts about the same argument, state the
weaker fact conditionally so both satisfy one contract:

```rust
// Caller A can prove `q.base <= q.addr`; caller B cannot. Guarding the
// consequent lets B's trivial implementation satisfy it vacuously, while A still
// gets the fact it needs.
(q.base <= q.addr => b.base <= b.addr)
```

## Bit-packed fields

`<<`, `>>`, `&`, `|`, `^` are uninterpreted. Supply `property` axioms at the
crate root (see `annotations.md`), choosing constants that match the real shifts
and masks, and guards that make each axiom true:

- `x << k == x * 2^k` under an overflow bound
- `x >> k == x / 2^k` for `0 <= x`
- `x & (2^k - 1) == x % 2^k`
- a **middle** field needs a compound form: `x & 28 == 4 * ((x / 4) % 8)`
- `(x | y) == x + y` when the ranges are disjoint — the step that turns the whole
  packing into arithmetic

Then the packing predicate is linear arithmetic and no bit-vector theory is
needed. Prefer parity-of-`base` guards over parity-of-`addr` guards where you
have the choice: `base` is fixed at construction, so a producer can establish it,
whereas `addr` moves and the fact has to be re-derived everywhere.

Keep div/mod out of struct invariants where you can — an invariant is re-asserted
at every use, so a `%`-heavy one multiplies solver work across the whole crate.

## Trait-generic quantities

A quantity defined per impl (a buffer's remaining bytes, a collection's length)
is an **associated refinement**. Give the trait an uninterpreted default so impls
that do not define one still typecheck:

```rust
flux_rs::defs! { fn default_buf_remaining<T>(me: T) -> int; }

#[flux_rs::assoc(fn remaining(me: Self) -> int { default_buf_remaining(me) })]
pub trait Buf { .. }
```

Each impl then defines it, and **each method restates the trait spec with the
concrete `Self`** (the step that is easy to miss — see `traps.md`).

An uninterpreted default means the solver knows *nothing* about it, including
that it is non-negative. Where you need bounds, obtain them from a call rather
than an axiom:

```rust
#[flux_rs::opts(check_overflow = "lazy")]
#[flux_rs::spec(fn(b: &T[@x]) ensures 0 <= <T as Buf>::remaining(x)
                                   && <T as Buf>::remaining(x) <= usize::MAX)]
pub fn assume_remaining_bounds<T: Buf>(b: &T) { let _ = b.remaining(); }
```

The discarded `b.remaining()` is the proof: its postcondition is a `usize`, which
is bounded by construction. Not trusted.

When a definition must mirror saturating arithmetic, match the library spec
exactly — `usize::saturating_add` is specced `-> $T[clamp(num + rhs, 0, $T::MAX)]`,
so the associated refinement has to be `clamp(a + b, 0, usize::MAX)`. Writing the
sum plainly, or with a hand-rolled `min`, will not line up with the body's value.

## One type, several representations: the ghost discriminator

A type that is one Rust type but several *representations* — a hand-rolled
vtable, a tagged union, an enum-shaped struct — has facts that hold for some
representations and not others. Asserting them of every value is unsound;
dropping them loses the contract. Guard them by a discriminator on whatever
value identifies the representation.

```rust
#[flux_rs::refined_by(data_is_buf: bool)]
pub(crate) struct Vtable {
    #[field(unsafe fn(&AtomicPtr<()>[@c], *const[@p] u8,
                      usize{l: valid_buf(p, l)
                               && (data_is_buf => c.base == p.base)}) -> Bytes)]
    pub clone: unsafe fn(&AtomicPtr<()>, *const u8, usize) -> Bytes,

    // Ghost: nothing reads it. It has to be a real field anyway -- see below.
    #[field(bool[data_is_buf])]
    pub data_is_buf: bool,
}

#[flux_rs::refined_by(ptr: ptr, len: int, dbase: int, vt: Vtable)]
#[flux_rs::invariant(vt.data_is_buf => dbase == ptr.base)]
pub struct Bytes {
    #[field(&Vtable[vt])]
    vtable: &'static Vtable,
    // ...
}
```

Each table is then pinned to its flags (`#[flux_rs::spec(Vtable[true])]` on a
`static`, `#[flux::constant(Vtable { .. })]` on a `const`), the holder's
invariant turns the flag into the fact, and the dispatch site discharges the
guarded precondition with no assumption at all.

Four things to know before reaching for this:

- **The discriminator must be a real field.** Every `refined_by` parameter needs
  a field index position; a zero-sized stand-in cannot provide one. An `opaque`
  ZST is rejected in a `const` initializer and a non-opaque one has no field of
  its own, hitting "cannot be determined" one level down. The cost is a byte or
  two in however many singletons exist, which is usually nothing.
- **Guard conditions belong on the argument that can see the others.** A
  constrained argument type may mention `@` binders from *earlier* arguments but
  not later ones, so the guard goes on the last argument it needs
  (`usize{l: .. && (flag => c.base == p.base)}` rather than on `p`).
- **State the guard at the weakest condition that is actually true.** If the
  fact also depends on runtime state — a tag bit, a promotion having happened —
  the guard needs that too (`flag && sp.addr % 2 == 1 => ..`). Getting this
  wrong is how an assumption ends up false, and nothing will tell you.
- **Writing the guard down is itself a check.** The exercise of naming when a
  fact holds is what catches an over-strong assumption. In the crate this came
  from, an `assume_*` helper had asserted the unguarded version for a whole pass
  — the sweep was green throughout, because an assumption is never contradicted
  by the thing it is assumed for. If you have `trusted` helpers publishing facts
  about a multi-representation type, try to replace them with a discriminator
  even when they appear to work.

## Relating separately fetched results

Independent calls give independent, unrelated results. `vec.len()`,
`vec.capacity()` and `vec.as_mut_ptr()` leave nothing connecting the capacity to
the pointer's byte extent, which is exactly what a buffer invariant needs. Bundle
them under one binder:

```rust
#[flux_rs::trusted(reason = "flux-alloc refines Vec by length only, so the relation
                             between cap and the pointer's extent has to be stated")]
#[flux_rs::spec(fn(vec: &mut Vec<T>[@n]) -> (usize[n], usize[#cap],
                   *mut{p: p.base == p.addr && cap <= p.size} T)
                ensures n <= cap && cap * T::size_of() <= 9223372036854775807)]
pub fn flux_vec_len_cap_ptr<T>(vec: &mut Vec<T>) -> (usize, usize, *mut T) {
    (vec.len(), vec.capacity(), vec.as_mut_ptr())
}
```

One trusted helper replaces an unprovable invariant at every construction site.
This is the general shape: when the gap is "these facts are each true but
unrelated", the fix is a shared binder, not a stronger precondition.
