# Bootstrapping a new crate

Do this before writing a single annotation. The goal is a crate where
`run_flux.py --full` prints `RESULT=SAFE` on the *unannotated* source — that is
your baseline, and without it you cannot tell your errors from pre-existing ones.

## Contents

- [Prerequisites](#prerequisites)
- [1. Wire the crate up](#1-wire-the-crate-up)
- [2. Get a green baseline](#2-get-a-green-baseline)
- [3. Generate the work list](#3-generate-the-work-list)
- [4. Per-target loop](#4-per-target-loop)
- [Keeping flux and the libs in step](#keeping-flux-and-the-libs-in-step)

## Prerequisites

On PATH: `flux`, `cargo-flux`, the `liquid-fixpoint` binary, and Z3 4.15+.
Flux pins a nightly in its own `rust-toolchain`, and the libs must be built from
the same checkout as the installed driver (see the last section).

## 1. Wire the crate up

**`Cargo.toml`** — three dependencies and the opt-in:

```toml
[dependencies]
flux-rs    = { path = "/path/to/flux/lib/flux-rs" }
flux-core  = { path = "/path/to/flux/lib/flux-core" }
flux-alloc = { path = "/path/to/flux/lib/flux-alloc" }

[package.metadata.flux]
enabled = true
cache = true          # drop this while debugging; stale cache looks like magic

# `cfg(flux)` is set by `cargo flux`, not cargo, so declare it or every
# ordinary build warns.
[lints.rust]
unexpected_cfgs = { level = "warn", check-cfg = ['cfg(flux)'] }
```

Prefer **path** deps over `git` deps. A git dep pins a revision that drifts out
of step with your locally installed driver, and the failure is baffling — e.g.
`invalid extern spec for trait: generic parameters don't match` from inside
`flux-core` itself. If the manifest must keep git deps, override them locally in
an untracked `.cargo/config.toml` instead of editing the manifest:

```toml
[patch."https://github.com/flux-rs/flux"]
flux-rs    = { path = "/path/to/flux/lib/flux-rs" }
flux-core  = { path = "/path/to/flux/lib/flux-core" }
flux-alloc = { path = "/path/to/flux/lib/flux-alloc" }
flux-attrs = { path = "/path/to/flux/lib/flux-attrs" }
```

**`src/lib.rs`** — crate-root attributes, all behind `cfg_attr(flux, ..)` so
ordinary builds are untouched:

```rust
// Needed if you will write extern specs for `Vec`'s allocator-generic impls.
#![cfg_attr(flux, feature(allocator_api))]

// Lets flux check field accesses through raw pointers, e.g. `(*ptr).ref_cnt`.
// With the default flux does not report an error for these — it crashes.
// Written as the raw `flux::` tool attribute because the `flux_rs::opts` proc
// macro cannot apply to a crate root (rustc rejects inner macro attributes
// over file modules).
#![cfg_attr(flux, flux::opts(allow_raw_deref = "ok"))]
```

**Crate-root `defs!`** — if the crate does any bit-packing, every `property`
must live here (in a submodule they parse and are silently ignored). Put shared
refinement functions here too, or in a dedicated `src/flux_specs.rs`, which is
also the natural home for extern specs and any trusted modelling helpers.

## 2. Get a green baseline

```bash
python3 <harness>/run_flux.py --full          # expect RESULT=SAFE
```

If it is not SAFE, fix that **before** annotating anything:

- `RESULT=CRASH` (an ICE) — do not hand-patch. Run
  `python3 <harness>/flux_thaw.py --crate-dir .`, which mechanically finds the
  crashing functions, adds `#[flux_rs::trusted(reason = "ICE")]` (plus
  `trusted_impl` on the enclosing impl when needed), and keeps only the edits
  that actually improve the crash signature. Then re-run.
- `RESULT=ERROR` from inside `flux-core`/`flux-alloc` — a lib/driver version
  skew. See the last section.
- `RESULT=TIMEOUT` — note which functions, and read the timeout guidance in
  SKILL.md.

Commit the baseline so every later step is revertible.

## 3. Generate the work list

```bash
python3 <harness>/run_flux.py --make-targets     # -> log/call_graph.json, log/type_graph.json
python3 <harness>/make_targets.py .              # -> log/targets.json, log/type_targets.json
```

`call_graph.json` / `type_graph.json` hold `{uid, path, span, deps}` per item,
already in dependency order — which is what makes bottom-up work mechanical.
`make_targets.py` just adds a `file:line:col` `target` field for convenience.

## 4. Per-target loop

**Phase 1, types** — walk `log/type_targets.json` in order:

```bash
sed -n '<start>,<end>p' <file>                       # read the definition + doc comments
# add #[refined_by(..)] / #[invariant(..)]
python3 <harness>/run_flux.py --check-type=<uid>
<harness>/checkpoint.sh "DONE: type <uid> <path>"    # on a clean pass
```

Pick the index set from the type's *representation invariant* (`len <= cap`,
`ptr` in bounds), not from whatever one function needs. An invariant is
re-asserted at every use, so a `%`-heavy one multiplies solver work crate-wide.

**Phase 2, functions** — walk `log/targets.json` in order:

```bash
python3 <harness>/run_flux.py --check-uid=<uid>
```

Before writing a spec: list the callees, read each one's existing spec, and work
out which callee's output reaches the error site. `--check-uid` prints the
callee specs it used under `=== CONTEXT ===`, which is usually enough to see
that the gap is a weak callee postcondition rather than a missing precondition
here. Then apply the strategy list in SKILL.md.

Checkpoint each clean pass. When you cannot get one, log it (see SKILL.md) and
move to the next uid rather than stalling.

## Keeping flux and the libs in step

The installed driver (`~/.cargo/bin/flux`, from `cargo xtask install` in the flux
checkout) and `flux-core`/`flux-alloc` must come from the same revision. After
rebuilding or switching flux:

```bash
rm -rf target/flux target/FLUXCACHE      # stale metadata looks like impossible errors
python3 <harness>/run_flux.py --full     # re-establish the baseline
```

Expect the baseline to move when flux changes — a new release can start checking
things it previously ignored, which shows up as errors in code you did not
touch. Re-baseline first, then attribute.

Flux's debug `target/` grows to tens of GB; `target/debug/incremental` alone is
usually several GB and is pure cache, so it is the safe thing to clear when the
disk fills.
