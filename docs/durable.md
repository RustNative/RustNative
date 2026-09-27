# Durable and event-driven execution

`PLAN.md` Milestone 56. `rustnative-durable` runs work that must survive
restarts, and work that arrives as events rather than as requests. It keeps
its records in SQLite, through `rustnative-server`'s data layer.

## Workflows

```rust
struct Fulfil;

#[async_trait::async_trait]
impl Workflow for Fulfil {
    const KIND: &'static str = "fulfil";
    type Input = Order;
    type Output = String;

    async fn run(&self, context: &mut WorkflowContext, order: Order) -> Result<String, WorkflowError> {
        context.transactional_step("charge", |tx| charge(tx, &order))?;  // exactly once
        context.sleep(Duration::from_secs(86_400)).await?;            // a durable timer
        let approved: bool = context.signal("approve").await?;       // a person decides
        context.step("ship", |key| book_courier(&order, key)).await?; // with an idempotency key
        context.compensate("refund", || refund(&order));              // undone if a later step fails
        Ok("shipped".into())
    }
}

let engine = LocalEngine::new(db)?.register(Fulfil);
engine.start::<Fulfil>("order-42", &order)?;
engine.run_until_idle().await;
engine.signal("order-42", "approve", &true)?;
```

- **Replay.** Every step's result is recorded. After a restart the
  workflow runs again from the top, and each recorded step returns its
  recorded result instead of running.
  - `examples/workflow-crash` kills its own process twice: after a step,
    and in the middle of one.
  - The next run completes, and each step has run exactly once.
- **Exactly once.**
  - `transactional_step` commits the step's database effect together with
    its journal entry.
  - `step` covers effects outside the database. It gives the work an
    idempotency key (`execution:position`) to pass to the system it calls,
    so a retry after a crash repeats the same key.
- **Timers and signals.**
  - `sleep` records its wake time, so after a restart the workflow waits
    only for the time that is left.
  - `signal` waits for `LocalEngine::signal`. An approval is a signal that
    carries `true` or `false`.
  - A waiting workflow does no work until it is due.
- **Compensation.** If the workflow fails, each `compensate` registered
  for a completed step runs, newest first. Each compensation is journaled.
- **Versioning.** `version("change", oldest, newest)` gives a new
  execution `newest`. An execution that was already past that point when
  the change shipped gets `oldest`. The answer is recorded, so it never
  changes during an execution.

### Determinism

Replay works only if the workflow makes the same decisions each time it
runs.

- Time, randomness, and I/O are reachable only through the context
  (`now`, `random`, and `step`). The context exposes no services.
- If a replay asks for a different step than the one recorded at that
  position, the workflow fails with `WorkflowError::Divergence`, naming
  both steps. It does not continue on a history that is wrong.
- The compiler cannot stop a workflow from calling the standard library's
  clock or random source directly. Put this `clippy.toml` beside the
  workflow module so such calls fail lint:

```toml
disallowed-methods = [
  { path = "std::time::SystemTime::now", reason = "use WorkflowContext::now" },
  { path = "std::time::Instant::now", reason = "use WorkflowContext::now" },
]
disallowed-types = [
  { path = "std::collections::hash_map::RandomState", reason = "use WorkflowContext::random" },
]
```

### Engines

- `WorkflowEngine` is the engine contract: start, signal, status, and run.
- `LocalEngine` implements it on SQLite.
- A hosted engine's adapter implements the same contract. It is owed
  until a deployment target hosts one.

## Event handlers

```rust
let runner = EventRunner::new(db, Arc::new(handler), /* batch */ 10, /* attempts */ 3)?;
runner.publish(&EventEnvelope::new("evt-1", "shop", "order.placed", data))?;
runner.run_until_idle().await?;
```

- **Envelope.** An `EventEnvelope` carries an id, source, kind, time,
  data, and attempt number, close to CloudEvents.
- **Partial failure.** A handler receives a batch and returns the events
  that failed (`BatchResult`). The rest are done, and only the failed ones
  are retried.
- **Retries.** Retries back off exponentially. After its last attempt, an
  event goes to the dead-letter table with its error.
- **Deduplication.** An id published twice is processed once.
- **Task scope.** Each invocation runs in a task scope that is cancelled
  when the invocation ends.
- **Serverless platforms.** `events::sqs_batch(handler)` runs a handler as
  a function's entry point: an SQS batch in, `batchItemFailures` out, so
  only the failed records are redelivered (Web milestone K,
  `a_function_batch_reports_its_partial_failures`). The handler contract
  does not change. Deduplication there is the queue's, or a key the
  handler checks against its data service.

## Actors

```rust
let documents = LocalActorSystem::<Document>::new(db, Duration::from_secs(60))?;
documents.ask("doc-1", Edit::Append(who, words)).await?;
```

- **One message at a time.** Each actor id has one instance, and it
  handles one message at a time. A read-modify-write inside `handle`
  cannot lose an update.
- **Durable storage.** An actor's storage (`context.storage()`) is
  durable. An idle actor is evicted. Its next message starts a new
  instance, which finds its storage as the last instance left it.
- **Alarms.** An alarm (`set_alarm`) is recorded, so it fires through
  `fire_alarms` even after a restart.
- **Two places.** `LocalActorSystem` is the single-process implementation,
  for development and single-node deployments. `edge::serve::<A>` runs the
  same actor on an edge host, an instance per request with its storage in
  the host's key-value store (Web milestone K, `C46`;
  `docs/web/deploy.md`). `examples/edge-actors` runs one collaborative
  document under both. Storage is a `Backend` (SQLite, or the edge store),
  and the crate builds for `wasm32-wasip1` without its `local` feature.

## Supervision

`supervise(policy, make)` runs a long-lived worker, such as a job
processor, an event runner, or a device loop. If the worker fails or
panics, it is restarted after the delay its `SupervisionPolicy` sets. The
function returns the worker's history.

## Operations across the boundary

`Operations` on the server starts work that reports progress. The server
serves the work's state at `GET /_ops/:id` and cancels it at
`POST /_ops/:id/cancel`. A client follows and cancels it over the core
`HttpService` (`follow`, `cancel`). The states are the same as in
`rustnative_data::OperationState`: running with progress, succeeded,
failed, and cancelled.
