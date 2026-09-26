# State, resilience, and data

`PLAN.md` Milestone 47. The worked example is `examples/data-demo`, a task
board over an in-process server. Its tests run it on the headless backend;
`rustnative-windows`'s `native::data_integration` runs the same machinery on
native controls.

The pieces every component can use are in `rustnative-core`: shared state,
error boundaries, supervision, and streams. The data layer is in
`rustnative-data`.

## Shared state

A `Store<T>` holds a value that two distant components both need. It is
provided to a subtree, not made global, and read by *slice*:

```rust
// The provider, once for its lifetime:
let cart = context.provide_scoped_with(|| Store::new("cart", Cart::default()));

// Any descendant:
let cart = context.scoped::<Store<Cart>>().unwrap();
let count = context.select(&cart, |cart| cart.items.len());
```

- `select` records what it read. After an update, the selector runs again,
  and the component re-renders only when its slice is unequal to what it
  saw (`C08`). An update that leaves every slice equal renders nothing.
- Updates apply in the order they are made. An update made from inside
  another is queued and applied right after it.
- `Derived<I, O>` caches a value computed from an input and recomputes it
  only when the input changes by value. Use it inside a selector.
- `Store::inspectable` stores are listed by `rustnative inspect stores`,
  with their values.
- `History<T>` (in `rustnative-data`) gives undo and redo. It can fold a run
  of changes (typing, say) into one step. Bind it to `CommandId::UNDO` and
  `CommandId::REDO`.

`provide_scoped` is also how a service with a narrower lifetime than the
application is scoped (`C16`):

- **Application** scope: `Services`.
- **Window** scope: provided by the window's root.
- **Destination** scope: provided by a navigation entry's screen.
- **Request** scope: arrives with the server (Milestone 49).

A scoped value is dropped when its provider unmounts.

## Error boundaries and supervision

```rust
context.boundary::<Feed>("feed", props, SupervisionPolicy::RestartWithBackoff {
    initial: Duration::from_millis(200),
    max: Duration::from_secs(5),
    attempts: 3,
}, |failure| Node::column("failed", [
    Node::label("why", failure.message.clone()),
    Node::button("retry", "Try again"),
]))
```

A boundary contains a panic anywhere in its subtree: while rendering, while
handling an event, or while receiving a message. Containing it does four
things:

1. The subtree is removed. Its tasks are cancelled and its effects cleaned
   up, as on unmount, so nothing the panic left inconsistent keeps running.
2. The fallback is shown in its place.
3. The failure is reported, in `ComponentTree::take_failures` and
   `Application::take_failures`, and in the inspection trace.
4. The policy decides what happens next:
   - **Restart with backoff** rebuilds the subtree from new state after a
     delay that doubles each time.
   - **Isolate** waits for a person to press the fallback's `retry` node.
   - **Escalate** passes the failure to the enclosing boundary.

A panic with no boundary above it still reaches the application's
`PanicPolicy`.

The same policies supervise tasks (`TaskScope::spawn_supervised`). A
failure never cancels its siblings.

## Streams and off-thread preparation

- `context.collect(stream, map)` delivers each item of a stream as a
  message, for as long as the component is mounted (`C12`).
  - Framework streams are hot.
  - Collection pulls as fast as the stream yields.
  - A producer that must not run ahead of the UI sends through a bounded
    channel.
- `context.prepare(|| build_index(rows))` computes off the UI thread and
  delivers the result, moved, as one message. The component applies it in
  one update (`C09-2`).

## Queries

```rust
let client = context.scoped::<QueryClient>().unwrap();
match client.use_query(context, Query::new(["todos"], fetch_todos).empty_when(Vec::is_empty)) {
    QueryState::Loading => …,
    QueryState::Empty => …,
    QueryState::Failure(error) => …,
    QueryState::Success(todos) | QueryState::Refreshing(todos) => …,
}
```

- **Exhaustive states** (`C29`). A failure with an earlier value keeps
  showing that value.
- **Keys** are hierarchical paths. `invalidate("todos")` marks everything
  under `todos/` stale and refetches what is observed.
- **Deduplication.** One request per key is in flight, however many
  components ask.
- **Two lifetimes** (`C30`). `stale_time` is how long a result is fresh.
  `retain_time` is how long it is kept after its last observer leaves;
  then it is collected.
- **Stale-while-revalidate.** A stale result shows as `Refreshing` while it
  is fetched again. The `Revalidate` policy declares when: on mount, on
  focus (`window_focused`), on reconnect (`set_online(true)`), and on an
  interval.
- **Retries** use exponential backoff with seeded jitter, so the delays are
  deterministic under a `ManualExecutor`.
- **Cancellation.** A fetch nobody observes any more is cancelled.
- **Structural sharing.** An equal result keeps the cached value's
  identity, so no component re-renders.
- **Pagination.** `Query::infinite` plus `fetch_next_page`.
- **Prefetch.** `QueryClient::prefetch`.
- **Batching.** A `BatchLoader` gathers the keys that components ask for
  while the UI thread is busy, and loads them in one request. Each
  component receives only its own item.

## Mutations and the offline queue

```rust
client.mutate(
    Mutation::new("add", &task)
        .optimistic::<Pages<Task, u32>>(["tasks"], move |pages| pages.items.push(task))
        .invalidates("tasks"),
);
```

- A mutation is data: a kind and a JSON payload. The code that sends each
  kind is registered once with `register_mutation`.
- Optimistic updates apply at once. They are rolled back when the server
  rejects the mutation.
- **Offline**, or when sending finds the server unreachable, a mutation
  joins the queue.
  - `with_offline_queue` keeps the queue in the state store, so it survives
    a restart.
  - It is sent in order on reconnect, and at the next start.
  - An entry leaves the queue only once it is sent.
- **Conflicts** follow the client's `ConflictPolicy`:
  - `ServerWins`: roll back.
  - `ClientWins`: send again with `force`.
  - `Merge(fn)`: send the merged payload with `force`.

## Forms

```rust
const EMAIL: Field<String> = Field::text("email");
const AGE: Field<i64> = Field::integer("age");
let schema = Schema::new()
    .field(EMAIL, [Rule::Required, Rule::Email])
    .field(AGE, [Rule::Range(13, 130)])
    .constraint("users_email_key", EMAIL, "is already registered");
```

- A `Changeset` holds raw input as typed. It is bound two ways: `set` from
  the field's change event, and `raw` for the text shown.
- It tracks what is dirty.
- `validate` returns typed output (`Valid::get(AGE)` is an `i64`) or
  per-field errors. Each error has a stable code for translation.
- The server checks raw input with the same `Schema::check`. The errors and
  messages are the same.
- A storage constraint violation maps back to its field.
- `Form` adds the submission lifecycle: `Idle`, `Submitting`, `Succeeded`,
  and `Failed`.

## Persisted state across versions

`Migrations::new().step(2, up, down)` moves a value up or down one
version.

- `VersionedStore` wraps the state store. It writes an envelope carrying
  the version, and migrates on load. Milestone 30's persisted component
  state therefore survives an update that changes its shape.
- A value written before versioning counts as version 1.
- A value from a newer version is not read.
- `dry_run` reports what a migration would do without writing anything.

## Navigation state

The navigation stack is typed state. Its saved state is each destination's
route: the subset a destination declares worth restoring (`C14`).

`NavigationStack::saved_state(SAVED_STATE_BUDGET)` refuses anything over
64 KiB and names the largest entry. An oversized destination is therefore
found in development, not dropped by the host.

## Local tables

A `LocalTable<T>` is kept in memory, and in the state store if asked.

- Its `live(context, filter, order)` query re-renders its component only
  when the result changes. Use it as a virtual list's source.
- The **repository pattern** (`C31`): the network writes to the table, and
  the UI reads from it.
- A `PagingSource` loads the pages a list's visible range needs and does
  not have. It fills gaps and never refetches what it holds.

## HTTP

`HttpClient` wraps any `HttpService` with interceptors:

- `BearerAuth` refreshes the token once on a 401;
- `Retry` retries idempotent requests on a transport error or a 5xx;
- `Logging` records each request;
- `ResponseCache` revalidates with the `ETag` and answers a 304 from cache.

`Endpoint::get("todos/{id}")` declares a typed call.

On Windows, `WinHttp` is the host service, and it enforces
`CertificatePins`. For a pinned host:

1. The request's headers are sent, which completes the TLS handshake.
2. The server's certificate digest is checked.
3. Only then is the body sent.

Plain HTTP and redirects are refused for a pinned host.

## Long-running work

- `Operation<P, R>` covers a long-running operation (`C87`). It has a goal,
  progress reports, cancellation, a result, and a store for components to
  select. Starting a new goal pre-empts the running one.
- `StateMachine<S>` is a state machine on an enum (`C10`).
  - Entering a state can start work in its `StateScope`, and leaving the
    state cancels that work.
  - `to_mermaid()` draws the machine.
- `BackgroundWork` runs a job once its `Constraints` hold: the network,
  external power, or a deadline, whichever comes first.
  - On Windows, `WindowsConditions` answers with `GetSystemPowerStatus`
    and `WinINet`.
  - A desktop application has no scheduler outside its process, so jobs
    run while it runs.

## Images

`ImageLoader::use_image(context, url, Some((w, h)))` is an image query,
keyed by URL and size, so it is deduplicated, retained, and cancelled like
any query.

- Fetched bytes are also kept in a disk cache.
- On Windows, `WicDecoder` decodes and downscales with the Windows Imaging
  Component. `PortableDecoder` reads PPM for tests.
- Memory is bounded by retention time rather than bytes.

## Owed

- A hidden subtree's stream collection pauses (Milestone 54). A query's
  interval revalidation still runs while its only observers are hidden.
- Request-scoped services and server-side validation with the shared
  schema arrive with Milestone 49.
- The progress component that shows an `Operation` arrives with
  Milestone 48.
- Mobile hosts' own background schedulers arrive with the other backends.
