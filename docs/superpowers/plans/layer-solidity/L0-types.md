# L0 — `rustnative-types` (deep read, iteration 1)

Files read in full: `Cargo.toml`, `src/lib.rs` (28), `src/color.rs` (48), `src/geometry.rs` (224), `src/scalar.rs` (51). Tests: one unit test (`edge_insets_sums_saturate_instead_of_overflowing`) and one doctest.

## Findings (verified against the source)

| ID | Severity | Finding | Evidence |
|---|---|---|---|
| L0-F1 | P1 | `Scalar` claims to be "a finite `f32`" but `Scalar::new` passes `±∞` through. Any consumer relying on finiteness (division, interpolation, layout rounding) can produce `NaN` downstream — and `NaN` is exactly what the type exists to keep out of `Event`. | `src/scalar.rs:3` (doc) vs `:23` (`if value.is_nan() \|\| value == 0.0 { 0.0 } else { value }`) |
| L0-F2 | P3 | Module doc and crate description say "totally ordered scalars", but `Scalar` implements only `PartialEq`/`Eq`/`Hash` — total *equality*, no `PartialOrd`/`Ord`. Misleads callers into expecting `Ord` (e.g. for `BTreeMap` keys or sorting samples). | `src/scalar.rs:1`, `Cargo.toml` description, `src/lib.rs:5` |
| L0-F3 | P3 | `From<u32> for Color` silently discards the top byte. `Color::from(0x336699CC)` written by someone expecting `0xRRGGBBAA` yields `rgb(0x66,0x99,0xCC)` — a wrong colour, not an error. | `src/color.rs:40-47` |
| L0-F4 | P2 (design) | `Size` is `u32` but `Rect`/`Point`/`SizeMode::Fixed`/`EdgeInsets` are `i32`: negative widths, negative fixed sizes, and negative insets are representable and the crate documents "no invariants". The invariant therefore lives in every consumer (layout engine, five backends, canvas). Whether each consumer clamps is an L3/L6 question — the indicator belongs here: *the set of representable-but-meaningless values is listed, and each consumer's handling is tested*. | `src/geometry.rs:16-52, 57-65, 116-127` |
| L0-F5 | P3 | `serde` field names are the public field names (`start`/`end`, `red`…); renaming a field silently changes every persisted/wire format that embeds these types (core enables `serde`). No fixture pins the serialized form. | `#[cfg_attr(feature = "serde", derive(...))]` on every type |
| L0-F6 | info | `Scalar` has no `serde` impl — good: a derived `Deserialize` would bypass `Scalar::new` (deserializing `NaN`/`-0.0` raw and breaking `Eq`). Must stay this way, or use `#[serde(from = "f32")]`. Worth a compile-time guard (a test that `Scalar: !Deserialize` or that any future impl goes through `new`). | `src/scalar.rs:11` |
| L0-F7 | info (positive) | `#![no_std]` with no `extern crate alloc`: allocation is structurally impossible — L0.6 is already E5. `#![deny(missing_docs)]` already on. | `src/lib.rs:19-20` |

## Indicators added or sharpened by the deep read

- **L0.1a** *Constructor postconditions are exhaustive*: for every one of the 2³² `f32` bit patterns, `Scalar::new(x).get()` is finite and not `-0.0` (exhaustive test, ~10 s release). Catches L0-F1.
- **L0.1b** *Doc ⇔ trait consistency*: a doctest asserting the claimed properties (`Eq + Hash`, and `Ord` only if implemented) — catches L0-F2's class (documentation claiming a trait the type lacks).
- **L0.4a** *Representable-but-meaningless values inventory*: `Rect{width<0}`, `SizeMode::Fixed(<0)`, `EdgeInsets{<0}`, `Size{u32 > i32::MAX}` (does not fit a `Rect`). Each listed in the crate docs with the documented meaning (clamp to 0? error?), and each consumer has a test (owned by L3 layout and L6 backends).
- **L0.5a** *Serialized-form fixtures*: one JSON fixture per type (with the `serde` feature) that must round-trip byte-exactly.
- **L0.7** *No invariant-bypassing deserialization*: types with invariants never derive `Deserialize` directly.

## Q-loop for L0 (iteration 1)

- Q2 aspects re-checked: correctness (F1, F2), robustness (F3, F4), compatibility (F5), soundness (no `unsafe`; `#![forbid(unsafe_code)]` still missing — add), concurrency n/a, resources n/a (no alloc), perf n/a.
- New question surfaced for the loop: **"what values can be constructed that the layer's documentation calls meaningless, and who handles them?"** — applied to every later layer (added to the method as Q1e).
