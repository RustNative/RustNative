# The invalidation contract

Exactly which components render for each kind of change. Every other
component is skipped and its previous output reused — spliced into its
parent's output, so a parent does not render because a child did. The
contract is held by `crates/rustnative-core/tests/invalidation.rs` against
`ComponentTree::last_render_log`, which records each render with its
`RenderCause`.

| Change | Renders | Cause |
|---|---|---|
| An event delivered to component *C* | *C* | `Event` |
| A message (callback or task result) delivered to *C* | *C* | `Message` |
| *C*'s parent renders and passes props unequal to *C*'s current ones | the parent (for its own reason) and *C* | `Props` |
| *C*'s parent renders and passes equal props | the parent only; *C* is reused | — |
| An environment value changes (window root, or provided by an ancestor) | exactly the components that read that key last render | `Environment(key)` |
| A descendant publishes a different preference value | the ancestors that read that preference | `Preference(key)` |
| A container a component read the size classes of crosses a class boundary | that component | `Environment("rustnative.container-size")` |
| The theme changes | every component | `Theme` |
| `ComponentTree::render()` is called explicitly | every component | `Forced` |
| A component is created | it | `Initial` |

Guarantees that follow:

- **batching** (`C09`): all state changes caused by one event or message
  are visible in the same render pass; no component renders twice in one
  pass unless a provided value changed during it (a bounded second pass);
- **reuse of native objects**: a skipped component's output is the same
  value as before, so the backend's diff for it is empty.

What this asks of components: render must be a function of the
component's own state, its props, and what it reads from the context
(environment, preferences, container sizes). State shared by other means —
an `Rc<RefCell<…>>` read during render — is invisible to invalidation;
route changes to it through a message, or through the shared-store
contract of Milestone 47.
