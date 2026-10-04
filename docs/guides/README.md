# Guides

Task-oriented guides, each with an example in `examples/` that builds and
is tested on every change (`PLAN.md` Milestone 52). Where a guide shows a
tree, it shows it in both syntaxes, and where it styles one, in both
spellings. `crates/rustnative-conformance/tests/doc_parity.rs` holds that.

| Task | Guide | Runnable example |
|---|---|---|
| Build a first application | `docs/guides/first-app.md` | `rustnative new`, then `examples/hello-label` |
| Lay out and style a screen | README ("Layout", "Style"), `docs/tokens.md`, `docs/styling.md` | `examples/gallery` |
| Use the component library | `docs/components.md` | `examples/gallery` |
| Keep state, forms, and lists | `docs/data.md` | `examples/data-demo`, `examples/filter-demo` |
| Translate it | `docs/i18n.md` | `examples/i18n-demo` |
| Stay responsive under load | `docs/responsiveness.md` | `examples/bench-app` |
| Inspect and debug it | `docs/inspection.md`, `docs/developer-loop.md` | `examples/reference-app` |
| Write a server | `docs/server.md` | `examples/server-demo`, `examples/server-client` |
| Sync and collaborate | `docs/sync.md` | `examples/collab-notes`, `examples/live-counter`, `examples/device-desired` |
| Run durable work | `docs/durable.md` | `examples/workflow-crash` |
| Deploy and update | `docs/deploy.md` | `crates/rustnative-server/tests/deploy.rs` |
| Observe and secure it | `docs/observability.md`, `docs/security/` | `crates/rustnative-windows/tests/observability.rs` |
| Tray, jump list, secure storage, flags | `docs/surfaces.md` | `examples/product-services` |
| Adopt it inside an existing application | `docs/interop/` | `examples/adoption-*` |
| Host web or media content | `docs/guides/host-content-controls.md` | `examples/gallery` |
| Draw your own content | `docs/guides/hybrid-rendering.md` | `examples/hello-label` |
| Write a capability package | `docs/packages.md` | `examples/package-battery` |
| Add accounts, admin, or a store | `docs/packages.md` ("Feature kits") | `examples/kits` |
| Upgrade across a breaking change | `docs/policy/stability.md` | `crates/rustnative/tests/codemod-corpus/` |

## The API reference

`cargo doc --workspace --no-deps` builds the reference from the source.
The verification gate builds it with warnings denied, so every public item
is documented. See `docs/api/README.md` for how the reference and the
machine-readable description (`docs/api/framework.json`) are produced.
