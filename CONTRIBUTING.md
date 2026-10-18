# Contributing

Every contribution is held to the engineering standards in
[`STANDARDS.md`](STANDARDS.md): what each standard requires, why, and what
enforces it, with the checklist reviewers verify against (its section 33).
This page lists the commands to run and the framework's design rules.

## Development checks

Run the following before opening a change:

```powershell
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo doc --workspace --no-deps
cargo deny check
cargo audit
```

Run them with the newest stable toolchain as well as the one installed
(`cargo +<newest> clippy …`): CI uses the newest stable, and a new release's
lints reach CI first. The MSRV is checked with `cargo +1.85 check
--workspace --all-targets`.

Each backend is verified on its own host:

- `rustnative-windows`: a real Windows session (its `native` module is
  `#[cfg(windows)]`). Native callback, HWND, COM, menu, dialog, and GDI
  changes add or update an executable integration test where practical.
  Visual goldens are kept per machine class (`native::capture`); a missing
  one fails with its capture written beside it, for review.
- `rustnative-linux`: a Linux session, or Xvfb with a private session bus —
  `tools/linux-session.sh xvfb cargo test -p rustnative-linux -p adoption-gtk`
  (WSL works) — on Ubuntu, Fedora, and Arch, as CI runs it.
- `rustnative-android`: `cargo clippy -p rustnative-android --target
  aarch64-linux-android --features device-tests`, then the device suite on a
  phone or emulator: `tools/android-device-test.sh`.
- `rustnative-web` and `rustnative-server`: the browser tests need Microsoft
  Edge (or Chromium) installed; they run in headless mode.

`cargo test --workspace` on a Linux machine without a display excludes the
GTK backend's own tests (`--exclude rustnative-linux`); run them through
`tools/linux-session.sh` instead.

## Design rules

- Preserve the core/platform dependency boundary. A platform is named in one
  place, `rustnative-host`; applications, examples, and templates ask for
  capabilities (`ComponentContext::capabilities`) and services, never for a
  platform. A capability realized only through a backend's own type is a
  missing portable contract: add it to `rustnative_core::services` first.
- Share code between backends only through a platform-group crate
  (`docs/conformance/platform-groups.md`); a backend never depends on another
  backend. Never tell two targets apart by `cfg` when they share a compiler
  target — iOS and iPadOS share `target_os = "ios"` and one universal binary,
  so the choice between them is made once at launch and everything downstream
  asks the backend it was handed (`PLAN.md` Milestone 70).
- Keep the two authoring syntaxes equal (`PLAN.md` 2.9). A new node kind or
  `with_*` modifier lands with its builder form, its markup element or
  attribute, and an equivalence case asserting that both produce equal `Node`
  values, with the markup compiled from a `.rsx` file and through `rsx!`.
  Markup may only emit builder calls — no node kind, runtime type, or
  behaviour may exist on one side and not the other.
- Keep the two style spellings equal (`PLAN.md` 2.14). A new style property
  lands with its typed form, its declaration name, its utility spelling or a
  documented note that it has none, an equivalence case asserting both spellings
  resolve to the same value, and each backend's answer for it — realized,
  approximated, or unavailable, given separately for a native control and for
  a box the framework owns. A style a backend cannot realize is a
  diagnostic, never a silent no-op.
- Do not narrow the style vocabulary to what every host shares, and do not
  widen it past what the headless backend can record. A property lands only
  with its headless realization, and properties land by family
  (`docs/styling.md`). A backend may decorate a box the framework owns through
  its host's drawing and composition services; it never captures, redraws, or
  owner-draws a native control to satisfy a style (`PLAN.md` 2.2).
- A style that depends on another node is a typed relationship the core
  resolves by a bounded lookup (`PLAN.md` 2.14, fifth rule), never a selector or
  a matching step. A new variant that would need one is refused, and host
  passthrough stays confined to one host and outside the equivalence suite.
- Keep the markup grammar identical in `.rsx` files and in `rsx!`. The
  `.rsx` compiler only wraps markup in `rsx!`; it never parses or lowers
  markup on its own, and never accepts anything the macro would reject.
- Write documentation in both syntaxes, labelled and side by side, with the
  markup as it appears in a `.rsx` file. Runnable doc examples use `rsx!` for
  the markup side, because `rustdoc` compiles plain Rust. Neither syntax is the
  default, so neither appears alone. The same holds for the style spellings
  wherever an example styles anything.
- Keep unsafe code contained and document the invariant that makes it safe.
- Use component-local keys; platform node IDs are runtime-scoped and opaque.
- Keep async work owned by a task scope and never mutate component state from
  an executor thread.
- Add tests for behavior changes, especially lifecycle and reentrancy cases.
