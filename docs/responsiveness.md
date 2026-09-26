# Responsiveness under load

`PLAN.md` Milestone 54. The reference application is
`examples/filter-demo`: 200 000 rows filtered as the person types.

- Its headless tests check keystroke handling against the input-latency
  budget, and check that a hidden screen does no periodic work.
- `rustnative bench` measures the same keystrokes on the real Windows edit
  control (`filter_input_latency_ms`, `filter_input_latency_max_ms`,
  `filter_results_ms` in `budgets/windows.toml`).
- `rustnative-windows`'s `native::responsiveness_integration` shows a hidden
  screen's task stopping. It also shows an idle window receiving no messages
  at all for two seconds.

## Priorities

A message carries a `Priority`, attached where it is sent:

```rust
callback.send(message);                                  // Normal
callback.send_with(Priority::Deferrable, results_ready); // after everything more urgent
```

- `Immediate` and `Normal` messages are delivered in the event that sends
  them.
- `Deferrable` messages wait. The host delivers them with
  `ComponentTree::pump_deferred`, and only while no input is queued: on
  Windows it checks `GetQueueStatus(QS_INPUT)`, and after handling the
  input it asks for another pump.
- Each pump delivers as many as fit the render budget (`set_render_budget`,
  4 ms by default), then renders once. A slice therefore commits whole, and
  a frame never shows half of one.

## Deferred values

Most expensive work in an interactive screen is computing what to show, not
rendering it. `ComponentContext::deferred` moves that work off the UI thread
and makes its lateness visible:

```rust
let matches = context.deferred("matches", self.query.clone(), move |query: String| {
    Arc::new(filter(&rows, &query))
});
match (&matches.current, matches.pending) {
    (None, _) => "Filtering…".to_owned(),
    (Some(found), true) => format!("{} rows match — updating…", found.len()),
    (Some(found), false) => format!("{} rows match", found.len()),
}
```

- When the input changes (by value), the computation for the old input is
  cancelled, and one for the new input starts when the UI thread is next
  free. A computation superseded before then never runs.
- Until the new result arrives, `current` is the previous result and
  `pending` is `true`.
- The keystroke's own render never waits. In the reference application it
  costs under a millisecond, whatever the size of the data.

## Pure components

A `PureComponent` renders from `&Props` alone. It has no state, so a render
that mutates does not compile (`C03`).

- It is composed with `context.pure::<Row>(key, props)`.
- It is skipped whenever its props equal the previous render's.

Stateful components keep `render(&mut self, …)`. The framework treats them
as not interruptible.

## Suspension

A component whose output is hidden has its task scope, and its effects'
scopes, suspended until it is shown again. The component keeps its state.
Hidden means:

- a node marked `hidden`, such as the screens under the top one of a
  `NavigationStack`, or non-selected tabs;
- a minimized window (`WindowStateChanged` to `Minimized`, which the
  application turns into `ComponentTree::set_backgrounded`).

Each task states what suspension means for it:

| Rule | While hidden |
|---|---|
| `SuspendRule::Complete` (`spawn`) | runs on; its result is delivered |
| `SuspendRule::Cancel` | cancelled |
| `SuspendRule::Defer` | not polled at all; continues on resume |

A periodic task spawned with `Defer` does no work while its screen is
hidden. Stream collection (`collect`) pauses the same way.

`ComponentTree::suspended_components` lists what is suspended.

## Skipping

A child whose props equal the previous render's is skipped, and the render
log says why each component rendered.

A props type that is not equal even to its own clone can never be skipped.
Debug builds detect this, and `ComponentTree::unskippable_components` names
such components.

## Frame pacing

The Windows backend does no periodic work of its own. With nothing to
animate, no input, and no pending work, the window receives no messages;
the native test counts them over two seconds of idle.

## Found and fixed

- **A hidden child that re-rendered alone reappeared.** The parent's
  `hidden` flag, and a list item's index, were lost when the child's fresh
  output was spliced into the parent's reused output. The splice now keeps
  what the parent set.
- **Slow keystrokes after each filter result.** A keystroke that landed
  right after a filter result waited for about 30 visible rows to be
  destroyed and recreated, because each row was keyed by its data. The
  rows are now keyed by position, so they are re-texted instead. The worst
  keystroke still waits for a result's render to be realized, about 30 ms
  on the reference machine; the median is under a millisecond.

## Owed

The constrained-target executor runs as one task at a declared priority,
and the embedded board's idle current is measured; both arrive with
Milestone 37's embedded backend.
