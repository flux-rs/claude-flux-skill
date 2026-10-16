// `unsafe fn` function-pointer fields. Split out from syntax_tour.rs because
// support for them is newer than the rest: it landed on flux's `fnptr-subtyping`
// work and is not in every build. If this file fails with
//
//     error[E0999]: syntax error
//       | #[field(unsafe fn(..))]
//       |         ^^^^^^ unexpected token
//
// the flux you ran it with predates that support -- and if the project you are
// annotating verifies anyway, your installed driver and your flux checkout have
// drifted apart. Check both (`ls -l ~/.cargo/bin/flux` against the checkout's
// HEAD) before concluding anything about the project.
//
// Run this with the SAME driver the project uses, not whichever one
// `cargo xtask run` happens to build.
use flux_rs::attrs::*;

#[refined_by(n: int)]
pub struct S {
    #[field(usize[n])]
    n: usize,
}

#[refined_by(flag: bool)]
pub struct Vtable {
    #[field(unsafe fn(&S[@a], usize{l: flag => l == a.n}) -> usize[a.n])]
    get: unsafe fn(&S, usize) -> usize,
    #[field(bool[flag])]
    flag: bool,
}

#[spec(fn(&S[@a], usize[@l]) -> usize[a.n] requires l == a.n)]
unsafe fn get_impl(a: &S, l: usize) -> usize {
    let _ = l;
    a.n
}

#[spec(Vtable[true])]
static GUARDED: Vtable = Vtable { get: get_impl, flag: true };

#[spec(fn(&S[@a], usize[@l]) -> usize[a.n] requires l == a.n)]
pub unsafe fn via_unsafe_field(a: &S, l: usize) -> usize {
    (GUARDED.get)(a, l)
}
