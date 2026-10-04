# Contributing

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

Changes to `rustnative-windows` also need verification on a real Windows
runner. Native callback, HWND, COM, menu, dialog, and GDI changes should add
or update an executable integration test where practical.

## Design rules

- Preserve the core/platform dependency boundary.
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
