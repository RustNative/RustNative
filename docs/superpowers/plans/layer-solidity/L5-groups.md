# L5 — platform groups: `rustnative-host`, `rustnative-update`, `rustnative-drawlist`, `rustnative-interop` (deep read, iteration 1)

✅ = reproduced by a scratch program (`scratchpad/verify5`, release build); no repository change.

Read in full:
- host: `lib.rs`
- update: `lib.rs`
- drawlist: `lib.rs`, `grid.rs`, `damage.rs`, `focus.rs`, `raster.rs`
- interop: `lib.rs`, `idl.rs`, `runtime.rs`, `generate.rs` (C header, C#, and Rust emitters)
- tests skimmed: `drawlist/tests/cell_quantized_layout.rs`, `examples/adoption-library/tests/hosts.rs`

## `rustnative-host`

| ID | Sev | Finding | Evidence |
|---|---|---|---|
| L5-F1 | P2 (security) | **`AppIdentity.id` is never validated**, though it becomes a directory name. It names the state store folder (`FileStateStore::for_app`), the crash directory, the secure-storage namespace, and the single-instance key. An id such as `../x`, `C:\x`, a name with reserved Windows device names (`con`), or an empty string is accepted. The reverse-domain format is only documented. To confirm in L6: what `for_app` does with it. | `host/src/lib.rs` `AppIdentity::new` |
| L5-F2 | P3 | **`wants_terminal()` treats any argv `--tui` as the switch.** That includes a value after `--`, or a file argument literally named `--tui`. It also reads `RUSTNATIVE_TARGET` in release builds, so the environment changes which backend runs (same pattern as L3-F20 and L3-F44). | `wants_terminal` |
| L5-F3 | P3 | Android `install_crash_capture` discards its result (`let _ =`). Linux capture is panic-only: there is no native-signal or abort capture, while Windows writes minidumps. The capability asymmetry is undocumented in the host contract. | `host` per-target `install_crash_capture` |
| L5-F4 | P3 (coverage) | `the_default_services_cover_every_contract_the_host_offers` asserts only 7 of the 13 to 15 services (share, work, push, commerce, printing, serial, and locale are unchecked). A new `Services` field does not fail it, so it is not a completeness guard. Windows and Linux supply no `share`/`work` even where the OS has them (Windows DataTransferManager, background tasks), and nothing records that as intended. | `host` tests |

## `rustnative-update` (signed update manifest)

| ID | Sev | Finding | Evidence |
|---|---|---|---|
| L5-F5 | **P1 (security)** | **Version parsing accepts garbage silently, which feeds downgrade and freeze decisions.** `compare_versions` maps any non-numeric part to `0` (`part.parse().unwrap_or(0)`). So `v2.0.0` < `1.0.0`, `1.2.x` == `1.2.0`, an over-u64 number == `0`, and a trailing space zeroes the last part. A publisher typo leaves every installation silently `UpToDate`. A malformed `current` (for example a `CARGO_PKG_VERSION` with a `v` prefix) makes every manifest look newer. Leading zeros (`01`) are accepted, though SemVer forbids them. Malformed versions must be `BadManifest`. | `update/src/lib.rs` `compare_versions` |
| L5-F6 | P2 (security) | **The model payload's installed version defaults to `"0"` on any read error** (`map_or_else(\|_\| "0")`). A transient read failure, or a deleted `current` file, makes any signed, unexpired, older model manifest an `Update`: a downgrade path. The same transient-error-becomes-absence pattern appears in L3-F17. | `Installation::check_at` |
| L5-F7 | P2 | **`write_atomically` does not fsync**: it writes `path.new` and renames, with no file or directory `fsync`. After a power loss the new name can point at an empty file on Linux or Android filesystems. The fixed temp name `*.new` also lets two writers collide. `FileStateStore` gets this right (L3-F29); here the same job is done worse. | `write_atomically` |
| L5-F8 | P2 (security, design) | **One pinned key, with no rotation and no revocation.** `Installation` trusts exactly one Ed25519 key. A lost key means installations can never be updated again; a compromised key cannot be retired. There is no key id in the envelope and no sequence number. Freeze attacks are bounded only by `expires` and an honest system clock: a user or malware that sets the clock back makes an expired manifest acceptable again. | `Installation::new`, `check_at` |
| L5-F9 | P3 (security) | **The model `name` from the manifest is joined into a path unvalidated** (`root.join("models").join(name)`). With `../..` or an absolute path it escapes the installation root (`join` with an absolute path replaces the root). It is signed, but this is defense in depth against a publisher-side bug. | `model_directory` |
| L5-F10 | P3 | `bucket()` maps a corrupt `installation-id` to bucket 0, so that installation always gets every rollout first. `pin()` stores the version, but `check` only tests whether the file exists: the pin never expires when the user updates manually. `sign` turns a serialization failure into empty bytes (`unwrap_or_default`). | `bucket`, `pin`, `sign` |
| L5-F11 | info (positive) | The signature is verified over the exact published bytes before parsing, so no re-serialization ambiguity exists. App-id binding and expiry refusal are both present. Pre-release ordering follows SemVer, and the tests cover it. | as cited |

## `rustnative-drawlist` (terminal and embedded rasterization)

| ID | Sev | Finding | Evidence |
|---|---|---|---|
| L5-F12 ✅ | **P1 (responsiveness, TUI)** | **Ellipse and path fills iterate over their whole bounds before clipping.** `fill_ellipse` and `fill_polygons` loop over every unit of the shape's bounding box and test `clip.contains` per unit. `fill_units`, by contrast, intersects with the clip first. Measured on an 80×24 target: an ellipse 8 000 px across takes 6.6 ms, 80 000 px takes 1.27 s, and 400 000 px takes **69 s**, all to write 0 visible cells; `fill_rect` of the same size takes 2 µs. A canvas (chart, map, zoomed drawing) whose data puts one shape far off-screen freezes the terminal UI thread. Path fills are additionally O(units × edges). | `drawlist/src/raster.rs` `fill_ellipse`, `fill_polygons`; verify5 output |
| L5-F13 ✅ | P2 | **`StrokeRect` builds its whole perimeter in a `BTreeMap` before clipping** (`outline`). That costs 39 ms for a 400 000 px rectangle and grows linearly in time and memory with the perimeter, which can be billions of units. `StrokeLine` caps itself at 100 000 steps but does not clip first either. | `outline`, `line` |
| L5-F14 | P3 | **"Rectangles never overlap" is not what the code guarantees.** `GridMapping::edges` widens a sub-unit rectangle to one unit, so two adjacent 3 px boxes at x=0 and x=3 (8 px cells) both map to `[0,1)`. The module doc says they "never overlap", and the test only asserts `left.right() <= right.x + 1`. Harmless once `quantize` has run, but the documented guarantee is false. | `grid.rs` `edges` and its doc; `adjacent_rectangles_stay_adjacent…` |
| L5-F15 | P3 | `GridRect::union` and `intersect` subtract without saturation (`right - x`), so a debug build can overflow on extreme rectangles. `Region::row_spans` allocates one entry per row of each damage rectangle, which is unbounded for an unclipped rectangle. A zero-scale transform still paints one unit ("nothing drawn vanishes"), though scale 0 should paint nothing. | `grid.rs`, `damage.rs`, `raster.rs` `covered` |
| L5-F16 | P2 (a11y/consistency) | **Drawn-host focus is plain preorder with no scope**: no modal or overlay focus trap (Tab leaves an open dialog), no visual or RTL ordering, and when the focused node disappears focus jumps to the first node instead of a neighbour. `hit_test` does not exclude disabled nodes, which compounds L3-F12. To compare with native backends in L6. | `focus.rs` |
| L5-F17 | info (positive) | Damage coalescing is bounded (`MAX_RECTS` = 16) and loses nothing (tested). Grid rounding has a single, documented rule. Alpha and premultiplied images are handled. Non-affine transforms are reported (`approximated_transform`) rather than silently mis-drawn. | as cited |

## `rustnative-interop` (library-only mode: C and C# bindings)

| ID | Sev | Finding | Evidence |
|---|---|---|---|
| L5-F18 | P2 | **`[thread = any]` is documented as "serialized by the handle table" but returns `BUSY` under contention.** `HandleTable::with` takes the value out of its slot; a concurrent call from a second thread finds `None` and fails with `status::BUSY` instead of waiting. A C# host calling from two threads gets intermittent `InteropException(5)`. | `idl.rs` module doc; `runtime.rs` `HandleTable::with` |
| L5-F19 | P2 | **Identifiers are not checked against target-language keywords.** `is_ident` accepts any ASCII identifier, so a parameter named `ref`, `out`, `object`, `string`, `event`, `base`, `lock` (C#), `type`, `match`, `loop`, `move` (Rust), or `int`, `char`, `default` (C) produces bindings that fail to compile, or fail only in the language the author did not build. Pascal-casing collisions (`foo_bar` and `fooBar` → `FooBar`), service names differing only in case (one `…_INSTANCES` static), `Foo`+`bar_baz` vs `Foo_bar`+`baz` (one C symbol), and a method named `dispose` (collides with `IDisposable.Dispose`) are likewise accepted by the IDL and fail later. Duplicate event names are not rejected. | `idl.rs` `is_ident`, `service`; `generate.rs` `symbol`, `pascal` |
| L5-F20 | P3 | **Event handlers cannot read the instance.** A callback runs while the instance is checked out, so a handler calling a getter on the same instance gets `BUSY`; this is undocumented. A panic in a method puts the possibly half-mutated instance back in its slot, with no poisoning. The C# class has no finalizer or `SafeHandle`, so a forgotten `Dispose` leaks the native instance and its delegates forever. There is no way to unregister a callback. | `runtime.rs` `Restore`; `generate.rs` `csharp_class` |
| L5-F21 | info (positive) | Every FFI entry runs inside `catch_unwind` and returns a status. Handles are 64-bit and never reused (no ABA). Thread affinity is checked on every owner-only call. Strings have a clear ownership rule, and NUL bytes are sanitised. A C program and a C# program are compiled against the generated output in `adoption-library/tests/hosts.rs`. `panic = "abort"` would defeat `guard`; the profile must be checked in L11. | as cited |

## Indicators added (L5)

- **L5.1 Identity validation** (F1): `AppIdentity::new` returns `Result`; an id must match `^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+$` (≤ 255 bytes, no reserved device names). A property test feeds arbitrary strings and asserts that every accepted id maps to a path inside the per-user root.
- **L5.2 Strict version grammar** (F5): `UpdateManifest::open` rejects any version that is not SemVer 2.0.0 (an exact grammar, no leading zeros, numbers ≤ u64). The current version is validated at `Installation::new`. The tests include a corpus of malformed versions.
- **L5.3 Update state durability** (F6, F7): every update record is written via temp → fsync → rename → dir fsync. Any read error on update state is an error, never a default value. A test injects a read error and asserts no `Update` decision follows.
- **L5.4 Key lifecycle** (F8): the envelope carries a key id; an installation trusts a key set with a rotation manifest signed by the old key; a monotonically increasing manifest sequence number is stored and older sequences are refused, which defeats replay and clock-rollback freezes.
- **L5.5 Rasterization cost is O(visible)** (F12, F13): every raster command intersects with the clip before iterating. A test with shapes 10⁹ px across, at every command kind, completes in under 1 ms on an 80×24 target.
- **L5.6 Drawn-host focus parity** (F16): `FocusModel` honours focus scopes (modal or overlay) and keeps focus near a removed node. A cross-backend test compares Tab order on the TUI against Windows and GTK for the same tree.
- **L5.7 IDL rejects what any target cannot compile** (F19): a reserved-word table per target language, and a generator round-trip test that compiles every generated binding for an IDL fuzz corpus (C with `cc`, Rust with `rustc`, C# with `csc` where available).
- **L5.8 Contention semantics** (F18): `[thread = any]` calls block (with a bounded wait) rather than return `BUSY`; a stress test runs 8 threads × 10 000 calls with zero `BUSY` results.

## Iteration 4 (2026-10-21)

A fresh Q1–Q10 pass over L5. The scratch crate `verify14/l4-l5` is outside the repository. Re-read with nothing new beyond L5-F5–F10, F18, and F20: all of `update/src/lib.rs` (envelope, expiry, model compatibility, bucket and pin, `write_atomically`), the `interop` IDL parser, `HandleTable`/`OwnerTable`, and C# UTF-8 marshalling.

| ID | Sev | Finding | Evidence |
|---|---|---|---|
| L5-F22 ✅ | P2 (correctness) | **Any rectangle wider than 2³¹ grid units is not drawn in release builds and panics in debug builds.** `State::covered` computes `floor_unit(x1 - 0.5) + 1`; `floor_unit` clamps to `i32::MAX`, so the `+ 1` overflows. In release it wraps to `i32::MIN`, and the degenerate fallback picks a one-unit rectangle at `i32::MAX`, off screen. `FillRect`, `FillRoundedRect`, `StrokeRect`, `PushClip`, and `HitRegion` all use it, so a full-width rule drawn as `fill_rect(0, 0, f32::MAX, h)` draws nothing, and a "no clip" `push_clip(huge)` hides everything inside it. `f32::MAX` and `inf` are common "unbounded" idioms, and `Scalar` admits `inf` (L0-F1). A different site from L5-F15. Reproduced: `fill_rect x=0 width=1e12: 0 visible cells filled (expect 10)`, `16x16 fill inside a 1e12 clip: 0 cells (expect 2)`; debug: `attempt to add with overflow` at `raster.rs:261`. | `drawlist/src/raster.rs:255-276, 300-307, 366-385` |
| L5-F23 ✅ | P2 (availability, U3) | **Every generated app panics at start when any command-line argument is not valid UTF-8.** `wants_terminal()` iterates `std::env::args()`, which panics on a non-Unicode argument, and the project template calls it first in `run()`. On Linux, opening a file with a Latin-1 name through "Open with" crashes the app before a window appears. Reproduced with the same expression and `caf\xE9.txt`: `called Result::unwrap() on an Err value: "caf\xE9.txt"`. | `host/src/lib.rs:220`; `rustnative/src/project/templates.rs:81, 236` |
| L5-F24 ✅ | P3 (U26) | **Interop doc comments go into generated code unescaped.** The C header writes each doc line as `/* {line} */`, so a doc containing `*/` (a glob such as `src/**/*.rs`) ends the comment and the rest becomes C code; C# puts docs into `<summary>` without XML escaping, so `&` and `<` produce malformed doc XML (CS1570). Reproduced: `cc -fsyntax-only` fails with `expected identifier or '(' before '.' token`. | `interop/src/generate.rs:77-81, 199-203` |

### Indicators added (iteration 4)

- **L5.9 Coordinates saturate end to end** (F22): every float-to-grid conversion and every `+ 1` on a converted value uses saturating arithmetic, and a raster test draws rectangles and clips at `f32::MAX` and `inf` and asserts the visible cells are filled.
