# Change detection

`PLAN.md` Milestone 52 (`C03-1`). Every UI framework must answer one
question: how does it know something changed? This page states the
framework's answer, lists what triggers a render and what does not, and
compares the answer with the four others in use. If you arrive from one of
those frameworks, this page tells you which of your expectations no longer
hold.

## The strategy: message-driven invalidation of the owning component

A component's state changes only in its own `update`, in answer to an
event or a message. The framework therefore knows exactly which component
may have changed, and renders that one.

- **The rest of the tree.** Everything else is skipped unless one of the
  inputs it read changed:
  - its props, compared by equality;
  - an environment value it read;
  - a preference it read;
  - a container size class it read.
- **The rules in full.** `docs/invalidation.md` has the exact table. It is
  tested by `crates/rustnative-core/tests/invalidation.rs` against the
  render log, so a render nobody asked for fails a test.
- **What this adds up to.** It is setter-triggered invalidation where the
  setter is the message. Ownership adds something that JavaScript
  frameworks cannot offer: the set of things that can change a
  component's state is known statically. Over-rendering is a testable
  property, not a performance anecdote.

### What triggers a render

- **An event or a message** delivered to the component: the component.
- **Changed props:** the component whose props changed (not equal to the
  last ones).
- **Changed context:** a component that read an environment key,
  preference, or container size class that changed.
- **A new theme:** every component.
- **A store the component subscribed to** (Milestone 47): the subscribers
  of the slice that changed.

### What does not

- **Mutation from outside.** Mutating shared state from outside the
  component, such as an `Rc<RefCell<…>>` written elsewhere and read during
  render. The framework cannot see it, so route the change through a
  message or a store.
- **Equal props.** A parent that renders and passes equal props does not
  render the child. The child's previous output is reused, and the
  backend's diff for it is empty.
- **Timers and completed tasks.** A timer firing or a task completing does
  nothing unless it delivers a message.

## Arriving from another strategy

| You know | How it detects change | What to expect here |
|---|---|---|
| **Dirty checking**: compare every watched value on a trigger | Scans all watchers | Nothing is scanned. Change arrives as a message, and only its owner renders. |
| **Asynchronous interception**: any async completion triggers a check | Patched async entry points | A finished task does nothing by itself. It must deliver a message (`TaskScope` results arrive as messages). |
| **Explicit notification**: observable properties raise events | Subscriptions | There are no subscriptions to leak. A shared store (Milestone 47) is the one place subscriptions exist, and a component's scope owns them. |
| **Setter-triggered re-render**: a state setter schedules the component | The setter | This is the closest. The "setter" is `update` handling a message, and a render is batched per event, not per assignment. |
| **Read tracking** (signals): reading during render subscribes | Tracked reads | Reads are tracked only for the environment, preferences, and container sizes. Component state is not tracked, because only its owner can change it. |

## Why this one

- **Predictable cost.** The work done for a change is bounded by the
  components that own or read it. Measured budgets hold it
  (`docs/responsiveness.md`).
- **Testable.** Invalidation is recorded (`RenderCause`), so a test can
  say exactly what must render.
- **No hidden subscriptions.** The owner is the only writer, so there is
  nothing to unsubscribe and nothing to leak.

The rejected alternative, asynchronous interception, is recorded with its
reasons in `docs/ecosystem-analysis/concepts-delivery.md`.
