// Exercises the forms the skill claims work.
use flux_rs::attrs::*;

defs! {
    fn valid_buf(p: ptr, len: int) -> bool { p.addr >= p.base && p.size >= 0 && len <= p.size }
    fn min_int(a: int, b: int) -> int { if a < b { a } else { b } }
    fn default_size<T>(me: T) -> int;
    property MaskLow[&](x, y) { 0 <= x => ([&](x, 1) == x % 2 && [&](x, 31) == x % 32) }
}

#[refined_by(n: int)]
pub struct S {
    #[field(usize[n])]
    n: usize,
}

// statics: index must be stated, and is checked
#[spec(S[1])]
static ONE: S = S { n: 1 };

#[spec(fn() -> usize[1])]
pub fn read_static() -> usize { ONE.n }

// bool indexed by a predicate; constrained arg; existential result
#[spec(fn(&S[@s]) -> bool[s.n == 0])]
pub fn is_empty(s: &S) -> bool { s.n == 0 }

#[spec(fn(n: usize{n > 0}) -> usize{v: v < n})]
pub fn dec(n: usize) -> usize { n - 1 }

// tuple results with an existential output binder related by `ensures`
#[spec(fn(&S[@s]) -> (usize[s.n], usize[#cap]) ensures s.n <= cap)]
pub fn len_cap(s: &S) -> (usize, usize) { (s.n, s.n) }

// &strg + ensures
#[spec(fn(s: &strg S[@old], k: usize{k <= old.n}) ensures s: S[old.n - k])]
pub fn shrink(s: &mut S, k: usize) { s.n -= k; }

// existential &mut accessor
#[spec(fn(s: &mut S) -> {k. &mut usize[k]})]
pub fn field_mut(s: &mut S) -> &mut usize { &mut s.n }

// pure `ensures` assumption helper
#[trusted(reason = "demo")]
#[spec(fn(&S[@s]) ensures s.n <= 100)]
pub fn assume_small(s: &S) { let _ = s; }

// field indexed by a referenced struct's index + an invariant relating them
#[refined_by(strict: bool)]
pub struct Vt {
    #[field(bool[strict])]
    strict: bool,
}

#[refined_by(n: int, m: int, vt: Vt)]
#[invariant(vt.strict => n == m)]
pub struct Handle {
    #[field(usize[n])]
    n: usize,
    #[field(usize[m])]
    m: usize,
    #[field(&Vt[vt])]
    vt: &'static Vt,
}

#[spec(fn(&Handle[@h]) -> usize{v: h.vt.strict => v == h.m})]
pub fn via_invariant(h: &Handle) -> usize { h.n }

// property in use: `&` lowered to `%`
#[spec(fn(x: usize) -> usize{v: v <= 1})]
pub fn low_bit(x: usize) -> usize { x & 1 }

// refined fn-pointer parameter mentioning an earlier param
#[spec(fn(off: usize, f: fn(usize[@q]) -> usize{v: v <= q}) -> usize{v: v <= off}
       requires off >= 0)]
pub fn apply(off: usize, f: fn(usize) -> usize) -> usize { let r = f(off); r }

// refined fn-pointer field, with `requires` (accepted here, unlike a param)
#[refined_by()]
pub struct Table {
    #[field(fn(&S{v: v.n > 0}) -> S)]
    dup: fn(&S) -> S,
}

#[spec(fn(&S[@s]) -> S[s] requires s.n > 0)]
fn dup_impl(s: &S) -> S { S { n: s.n } }

static TABLE: Table = Table { dup: dup_impl };

#[spec(fn(&S[@s]) -> S requires s.n > 0)]
pub fn via_table(s: &S) -> S { (TABLE.dup)(s) }

// ===== statics vs consts, and unsafe fn pointer fields =====

#[refined_by(flag: bool)]
pub struct GTable {
    // The guard goes on the last argument that can see the earlier binders.
    // `unsafe fn` works the same way where the flux build supports it -- see
    // syntax_tour_unsafe_fnptr.rs.
    #[field(fn(&S[@a], usize{l: flag => l == a.n}) -> usize[a.n])]
    get: fn(&S, usize) -> usize,
    // Ghost discriminator: must be a real field, since every `refined_by`
    // parameter needs a field index position.
    #[field(bool[flag])]
    flag: bool,
}

#[spec(fn(&S[@a], usize[@l]) -> usize[a.n] requires l == a.n)]
fn get_impl(a: &S, l: usize) -> usize {
    let _ = l;
    a.n
}

// A `static`'s index: CHECKED against the initializer.
#[spec(GTable[true])]
static GUARDED: GTable = GTable { get: get_impl, flag: true };

// A `const`'s index: ASSUMED. Needed only when a `const fn` reads it.
#[flux::constant(GTable { flag: true })]
const GUARDED_C: GTable = GTable { get: get_impl, flag: true };

#[spec(fn(&S[@a], usize[@l]) -> usize[a.n] requires l == a.n)]
pub fn via_static(a: &S, l: usize) -> usize {
    (GUARDED.get)(a, l)
}

#[spec(fn(&S[@a], usize[@l]) -> usize[a.n] requires l == a.n)]
pub fn via_const(a: &S, l: usize) -> usize {
    (GUARDED_C.get)(a, l)
}

// ===== a fn-pointer argument that is both bound and constrained =====
// `&{T[@q] | pred}` is the form that gives a signature-scoped binder, a
// constraint, AND keeps `q` usable in the return type. Guarding the constraint
// lets call sites with different capabilities share one contract.

#[spec(fn(&S[@q]) -> S[q])]
fn id_lax(s: &S) -> S {
    S { n: s.n }
}

#[spec(fn(&{S[@q] | q.n > 0}) -> S{v: v.n == q.n && v.n > 0})]
fn id_pos(s: &S) -> S {
    S { n: s.n }
}

#[spec(fn(s: &S[@a], strict: bool[@t], f: fn(&{S[@q] | t => q.n > 0}) -> S{v: v.n == q.n})
       -> S{v: v.n == a.n}
       requires t => a.n > 0)]
fn apply_guarded(s: &S, strict: bool, f: fn(&S) -> S) -> S {
    let _ = strict;
    f(s)
}

#[spec(fn(&S[@a]) -> S{v: v.n == a.n})]
pub fn via_lax(s: &S) -> S {
    apply_guarded(s, false, id_lax)
}

#[spec(fn(&S[@a]) -> S{v: v.n == a.n} requires a.n > 0)]
pub fn via_pos(s: &S) -> S {
    apply_guarded(s, true, id_pos)
}
