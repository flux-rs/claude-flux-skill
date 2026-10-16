# Flux annotation catalog

Every form below is compile-checked: `examples/syntax_tour.rs` exercises all of
them and verifies clean. Run it against a flux checkout with
`cargo xtask run <path>/examples/syntax_tour.rs` to confirm the forms still hold
on your version. Attribute paths are shown as
`flux_rs::` (the `flux-rs` crate); in flux's own test suite they are `flux::`.
Inside `#[flux_rs::refined_by(..)]`-annotated items, `#[field(..)]` is written
bare — `refined_by` is an attribute macro that consumes it, so
`#[flux_rs::field(..)]` is an error ("cannot find `field` in `flux_rs`").

## Contents

- [Function specs](#function-specs)
- [Indices, existentials, binders](#indices-existentials-binders)
- [Mutation: `&mut` vs `&strg`](#mutation-mut-vs-strg)
- [Structs: refined_by / invariant / field](#structs)
- [Statics and consts](#statics-and-consts)
- [defs!: refinement functions](#defs-refinement-functions)
- [defs!: property (primitive-op axioms)](#defs-property)
- [Traits: assoc refinements](#traits-assoc-refinements)
- [extern_spec](#extern_spec)
- [Escape hatches: trusted / opaque / opts](#escape-hatches)
- [Refined function pointers](#refined-function-pointers)

## Function specs

```rust
#[flux_rs::spec(fn(x: usize, y: usize) -> usize[x + y] requires x + y <= usize::MAX)]
#[flux_rs::spec(fn(&Bytes[@b]) -> usize[b.len])]
#[flux_rs::spec(fn(n: usize{n > 0}) -> usize{v: v < n})]          // constrained arg / result
#[flux_rs::spec(fn(&[u8][@n], i: usize) -> u8 requires i < n)]
#[flux_rs::spec(fn(&Vec<T, A>[@n]) -> bool[n == 0])]              // bool indexed by a predicate
#[flux_rs::spec(fn(a: usize, b: usize) -> (usize[a], usize[b]))]  // tuples
#[flux_rs::spec(fn(&T[@x]) ensures 0 <= f(x) && f(x) <= usize::MAX)]  // pure `ensures`, no mutation
```

`requires` / `ensures` go after the return type. `ensures` without `&strg` just
publishes facts about the inputs — the pattern for a deliberate assumption
helper.

Associated-type size/alignment are available as refinement functions:
`T::size_of()`, `T::align_of()`, `<T as Sized>::size_of()`.

A trait method's spec can name the trait's associated refinements:
`-> usize[<Chain<T, U> as Buf>::remaining(c)]`.

## Indices, existentials, binders

| form | meaning |
|---|---|
| `T[e]` | index is exactly `e` |
| `T[@x]` | bind the index to `x`, usable later in the signature |
| `T{v: p}` | existential: some index `v` satisfying `p` |
| `T{v: p}` in arg position | a precondition on that argument |
| `usize[#cap]` | existential *output* binder, usable in a later `ensures` |
| `{k. &mut T[k]}` | existential over a mutable reference (see below) |

```rust
// `#cap` is chosen by the callee and related to the other results.
#[flux_rs::spec(fn(vec: &mut Vec<T>[@n]) -> (usize[n], usize[#cap], *mut T)
                ensures n <= cap)]
```

## Mutation: `&mut` vs `&strg`

`&mut T[@b]` makes the index **invariant** — the post-state must still be `b`.
That is right for in-place edits that preserve the refinement and wrong for
anything that changes it.

```rust
// Index may change; `ensures` describes the post-state. `self:` names the location.
#[flux_rs::spec(fn(self: &strg BytesMut[@b], count: usize{count <= b.cap})
                ensures self: BytesMut{v: v.cap == b.cap - count
                                       && v.nn.addr == b.nn.addr + count})]

// A `&mut` accessor the caller must be able to mutate: existential, not `[@b]`.
// With `&mut T[@b]` the caller could not advance the inner buffer at all.
#[flux_rs::spec(fn(self: &mut Chain<T, U>) -> {k. &mut T[k]})]
```

## Structs

```rust
#[flux_rs::refined_by(ptr: ptr, len: int, dbase: int)]
#[flux_rs::invariant(crate::flux_specs::valid_buf(ptr, len))]
pub struct Bytes {
    #[field(*const [ptr] u8)]
    ptr: *const u8,
    #[field(usize[len])]
    len: usize,
    #[field(AtomicPtr<()>[dbase])]
    data: AtomicPtr<()>,
    vtable: &'static Vtable,            // unannotated fields are fine
}
```

Every `refined_by` parameter must be **determined** by some field's index
position, or flux rejects the item with "parameter `x` cannot be determined".
A parameter appearing only in an `invariant`, a `requires`, or inside a
function-pointer field type does not count.

A field may be indexed by *another* struct's index, and an invariant may relate
the two — the way to express "this handle's representation is identified by the
table it points at":

```rust
#[flux_rs::refined_by(n: int, m: int, vt: Vt)]
#[flux_rs::invariant(vt.strict => n == m)]
pub struct Handle {
    #[field(usize[n])] n: usize,
    #[field(usize[m])] m: usize,
    #[field(&Vt[vt])] vt: &'static Vt,
}
```

Note the two positions for generics: a struct used as a **sort** takes none
(`vt: Vt`, `me: VecDeque`), while the same struct in a **type** position takes
all of them (`&VecDeque<u8, alloc::alloc::Global>[@d]`).

## Statics and consts

Neither one's index is inferred from its initializer — read a field of a bare
`static ONE: S = S { k: 1 };` and you get an arbitrary value. Both can be
pinned, but by different attributes with **different trust**, which is the thing
to get right:

```rust
// static: CHECKED against the initializer. A wrong index is rejected on the
// `static` itself, so this is sound, not an assumption.
#[flux_rs::spec(S[1])]
static ONE: S = S { k: 1 };

// const: ASSUMED. A wrong value produces no error here -- only at the readers,
// which then get facts that do not hold. Treat it as a one-line `trusted`.
#[cfg_attr(flux, flux::constant(S { k: 1 }))]
const K: S = S { k: 1 };
```

Use a `static` when you have the choice. A `const` is forced when a `const fn`
reads it, since a const fn cannot refer to a static — and note that a `static`
whose initializer *reads* a const inherits the const's unknown index, so a
"pinned twin" needs its own duplicated literal.

Other forms that do not work, so you do not go looking: `#[flux_rs::spec(T[..])]`
on a `const` is a syntax error (the proc macro only accepts `fn` signatures);
a detached `#[flux::specs { .. }]` block accepts `static` but rejects `const`;
and `#[flux_rs::trusted]` on either has no effect at all.

## defs!: refinement functions

Crate-global from anywhere, one block per module is fine.

```rust
flux_rs::defs! {
    fn valid_buf(p: ptr, len: int) -> bool {
        p.addr >= p.base && p.size >= 0 && len <= p.size
    }
    fn min_int(a: int, b: int) -> int { if a < b { a } else { b } }

    // Uninterpreted (no body): a name the solver knows nothing about. Useful as
    // the default for a trait associated refinement.
    fn default_buf_remaining<T>(me: T) -> int;
}
```

Sorts: `int`, `bool`, `ptr`, `NonNull`, and any `refined_by` struct. Field access
is `p.addr`, `r.start`. Available operators include `+ - * / %`, comparisons,
`&&`, `||`, `=>` (implication), `if .. { } else { }`.

## defs!: property

`<<`, `>>`, `&`, `|`, `^` are **uninterpreted** in flux. A `property` supplies
axioms for them, instantiated per application site, which is how bit-packed
fields become linear arithmetic with no bit-vector theory. State the guard that
makes each one true — these are assumed, not proved.

```rust
flux_rs::defs! {
    property ShlPow2[<<](x, y) {
        (0 <= x && x <= 576460752303423487) => [<<](x, 5) == 32 * x
    }
    property MaskLow[&](x, y) {
        0 <= x => ([&](x, 1) == x % 2 && [&](x, 31) == x % 32)
    }
    // `|` over disjoint bit ranges is addition — the step that turns a whole
    // packing into arithmetic.
    property OrDisjoint[|](x, y) {
        (x % 32 == 0 && 0 <= y && y < 32) => [|](x, y) == x + y
    }
}
```

**A `property` only takes effect at the crate root.** Inside a submodule's
`defs!` it parses and is silently ignored — unlike `defs!` *functions*, which are
global from anywhere. This failure mode looks exactly like a solver blow-up.

## Traits: assoc refinements

Declare on the trait with a default, then define per impl:

```rust
#[flux_rs::assoc(fn remaining(me: Self) -> int { crate::flux_specs::default_buf_remaining(me) })]
pub trait Buf { .. }

#[flux_rs::assoc(fn remaining(me: Take<T>) -> int {
    if <T as Buf>::remaining(me.inner) < me.limit { <T as Buf>::remaining(me.inner) } else { me.limit }
})]
impl<T: Buf> Buf for Take<T> {
    // The impl must ALSO restate the trait's spec with the concrete `Self`.
    // Supplying only `#[assoc]` is not enough — see references/traps.md.
    #[flux_rs::spec(fn(&Take<T>[@t]) -> usize[<Take<T> as Buf>::remaining(t)])]
    fn remaining(&self) -> usize { .. }
}
```

Associated refinements also drive indexing/slicing contracts
(`fn in_bounds(..) -> bool`, `fn output_pred(..) -> bool`).

## extern_spec

Specs for types and functions you do not own. The signature must match the
external item **exactly**, including generic parameter *names*.

```rust
// free function
#[flux_rs::extern_spec(core::ptr)]
#[flux_rs::spec(fn(*const[@src] T, *mut[@dst] T, count: usize)
                requires count * T::size_of() <= src.size
                      && count * T::size_of() <= dst.size)]
unsafe fn copy<T>(src: *const T, dst: *mut T, count: usize);

// inherent impl (note: no body, `;` instead)
#[flux_rs::extern_spec(core::ptr)]
impl<T> *mut T {
    #[flux_rs::spec(fn(me: *mut[@p] T) -> *mut[p] U)]
    fn cast<U>(self) -> *mut U;
}

// give an external type a refinement, then spec its methods
#[flux_rs::extern_spec(alloc::collections)]
#[flux_rs::refined_by(len: int)]
struct VecDeque<T, A: core::alloc::Allocator>;

#[flux_rs::extern_spec(alloc::collections)]
impl<T, A: core::alloc::Allocator> VecDeque<T, A> {
    #[flux_rs::spec(fn(&VecDeque<T, A>[@n]) -> usize[n])]
    fn len(&self) -> usize;
}

// trait
#[flux_rs::extern_spec]
#[flux_rs::assoc(fn with_size(self: Self, n: int) -> bool { true })]
trait FromIterator<T> {}
```

Only **stable** paths work as targets: `core::io` is unstable and rejected, while
`std::io` resolves to the same `DefId` and works.

## Escape hatches

```rust
// Body skipped, signature trusted. Always give a tested reason.
#[flux_rs::trusted(reason = "models `ptr as usize`, which flux lowers to an unrefined usize")]

// Same, for a whole impl block.
#[flux_rs::trusted_impl]

// Overflow checking relaxed for this function only.
#[flux_rs::opts(check_overflow = "lazy")]

// Asserts the body cannot panic.
#[flux_rs::no_panic]

// Fields invisible to flux; the type is manipulated only through specced
// methods. Prefer `refined_by` + `invariant`; `opaque` gives up the body.
#[flux_rs::opaque]
#[flux_rs::refined_by(val: int)]
pub struct Ghost;
```

`trusted` applies to functions and impls. It has **no effect on a `static`** — to
constrain a static, give it a `spec` (above).

## Refined function pointers

A fn-pointer *parameter* or *field* can carry a refined type, and a coercion is
checked against it.

```rust
// parameter: the result is related to the argument, and may mention earlier params
#[flux_rs::spec(fn(off: usize, f: fn(usize[@q]) -> usize{v: v <= q}) -> usize{v: v <= off})]

// field: a coercion into it is checked, and callers get the contract
#[flux_rs::refined_by()]
pub struct Vtable {
    #[field(fn(&Handle[@h]) -> Handle[h])]
    clone: fn(&Handle) -> Handle,
}
```

**Express a precondition as a constrained argument type, never as `requires`.**
A `requires` clause inside a `#[field(fn(..))]` parses and is then *silently
ignored* in both directions — it does not bind callers and it does not discharge
the target's own `requires` at the coercion. A constrained argument type does
both:

```rust
#[field(fn(&S{v: v.n > 0}) -> S)]       // callers must prove `n > 0`;
dup: fn(&S) -> S,                        // and `dup_impl`'s `requires n > 0` is met
```

Other constraints worth knowing before designing around these:

- Parameters in a fn-pointer type cannot be **named** (`fn(*mut[@q] ())`, not
  `fn(s: *mut[@q] ())`).
- `requires` on a fn-pointer type in a **parameter** position is a parse error.
  To give an argument a precondition *and* keep its index usable in the return
  type, wrap the whole type: **`&{P[@q] | pred}`** — brace around the type,
  binder in the index, constraint after the bar, reference outside. This is the
  form to reach for; it gives a signature-scoped binder, a constraint, and `q`
  visible in the result all at once.

  ```rust
  // by reference: brace inside the `&`
  f: fn(&{P[@q] | q.addr == q.base}) -> P{b: b.base == q.base && b.addr == b.base}

  // by value (a raw pointer, an integer): the brace wraps the whole type
  f: fn({*mut[@q] () | q.addr == q.base}) -> *mut{b: b.base == q.base} u8
  ```

  Guard the constraint (`o => q.addr == q.base`, with `o` another parameter) when
  several call sites must share one contract and only some can supply the
  precondition.

  The near-miss spellings all fail, and none of the errors points at the brace:
  `&P{q: ..}` and `{q. &P[q] | ..}` parse but the binder does not reach the
  return type (`cannot find value q in this scope`); `&P[@q]{..}` and
  `&P[@q]{q: ..}` are syntax errors.
- A constraint on argument *N* may mention `@` binders from arguments *1..N-1*,
  which is the other way to place a precondition — put a carrier argument after
  the one you need to constrain. Recognise it in existing specs; prefer
  `&{T[@q] | ..}` in new ones.
- `unsafe fn` fields and parameters **can** be refined, on builds that have the
  fn-pointer subtyping work — write `unsafe fn(..)` in the `#[field(..)]`,
  matching the field structurally. On older builds it is a syntax error, which
  is a useful drift detector: see `examples/syntax_tour_unsafe_fnptr.rs`.
- A **closure** coerced to a fn pointer is not checked against the refined type.
  Hoist it to a named `fn` item and it verifies unchanged.
- An **unrefined** fn-pointer field is not neutral: flux still discharges the
  target's `requires` at the coercion, for arbitrary arguments. So a function
  installed in an unrefined table may not carry a precondition at all.
