# Typestate as a design rule (`C21`, Milestone 39)

A framework API whose states permit different operations encodes those
states in types, so an operation invalid in the current state does not
compile. Applied so far:

| API | States | Where |
|---|---|---|
| Escape-hatch handles | `NativeHandle<Unchecked>` → `validate` → `NativeHandle<Live>`; only `Live` exposes the raw value | `rustnative_core::handle` |
| Capability-gated services | `Services::scoped(grants)` yields `Granted<S>` only for granted services | `rustnative_core::grant` |
| Thread affinity | `UiThread` is `!Send`; APIs that need the UI thread take `&UiThread` | `rustnative_core::affinity` |
| Validated form values | raw input → `Changeset` → `Validated<T>` (Milestone 47) | `rustnative-data` |

When to apply it: when misuse is a correctness bug rather than a style
issue, and when the states are few and named. When not to: when the state
depends on runtime data the caller cannot know statically — then a `Result`
is the honest type.
