# Reconciliation beyond the screen

`PLAN.md` Milestone 55. The framework's core idea is to declare the desired
state and reconcile reality towards it. `rustnative-sync` applies that idea
in three places:

1. Data replicated between devices and a server.
2. A UI tree held on a server.
3. A device fleet.

## Sync

```rust
let server = Arc::new(Mutex::new(
    SyncServer::new().collection::<Note>(ConflictPolicy::LastWriterWins, 1),
));
let mut notes = SyncedCollection::<Note>::new(Arc::new(Clock::new(replica)));
notes.put(&note);              // local first; works offline
notes.sync(&transport).await?; // push what changed, pull what others changed
```

- **Local first.** Reads and writes use the local copy. Writes made
  offline stay pending until a sync succeeds. `snapshot` and `restore`
  persist the copy across restarts.
- **Conflict policy.** The server declares one per collection:
  - `ServerAuthority`: a write based on an outdated copy is refused, and
    the writer receives the server's copy.
  - `LastWriterWins`: every write carries a hybrid logical clock stamp, and
    the later stamp wins.
  - `Merge(fn)`: the two values are combined. For collaborative text,
    merge the CRDTs inside them, as `examples/collab-notes` does.
- **Partial replication.** A `Filter` (a field equal to a value) limits
  what a replica pulls.
- **Server push.** `SyncServer::subscribe` signals every accepted write.
  Over HTTP, `GET /_sync/wait?since=N` long-polls for it.
- **Schema versions.**
  - Records carry the version they were written at.
  - `Record::upgrade` brings an older record up to date, on the server and
    on a newer client.
  - A client older than the collection's minimum is told `NeedsUpgrade`.
- **Transports.**
  - `InMemory`: can be taken offline, for tests.
  - `HttpSync`: over the core `HttpService`, so it works through `WinHttp`.
  - With the `server` feature, `http::server::mount` adds the endpoints to
    a `rustnative-server` application.

## Replicated types

`GCounter`, `PnCounter`, `LwwRegister`, `OrSet` (an add wins over a
concurrent remove), `LwwMap`, and `Rga` (a sequence, for lists and
collaborative text). Property tests (`tests/crdt.rs`) check that every
merge is commutative, associative, and idempotent.

## Server-interactive mode

```rust
// Server
LiveServer::new(CounterApp, Duration::from_secs(30)).serve(listener).await;

// Windows client
let client = LiveClient::connect("ws://127.0.0.1:8095");
Application::new(RemoteView::new(client), window)
```

- **Session.** Each connection is a session. Its `ComponentTree` runs on
  its own thread on the server, and the tree travels as a
  `rustnative_core::wire::WireNode`. The client reconciles each arrival like
  any tree.
- **Events.** Events travel back numbered, and each tree says the last
  event it reflects. An event not yet reflected is sent again after a
  reconnect, and the server applies each number once. Nothing is lost or
  doubled. A text input's typing is echoed locally at once (the optimistic
  hook) until the server acknowledges it.
- **Reconnection.** The client reconnects with its session id. Within the
  grace period the tree is still there, and nothing is lost. After it, the
  client offers its last state snapshot, and the new session starts from
  that. The root component's `inspect` supplies the snapshot.
- **Deployment draining.** `LiveServer::drain(new_url, pause)` sends each
  client its state and the new address. The clients move to the new
  instance without losing state.
- **Render mode per subtree.** `Subtree` has four modes:
  - `Static`: rendered once.
  - `ServerInteractive`.
  - `ClientInteractive`: the component from `ClientModules`.
  - `Auto`: server-interactive until the client registers the component,
    then client-interactive, starting from the server session's state.

  The state transfer is tested (`tests/live.rs`).
- **Protocol.** WebSocket frames of JSON:
  - client to server: `hello` and `event`;
  - server to client: `welcome`, `tree`, and `drain`.
  - The browser client is owed with Web milestone H.

## Channels and presence

`Channel` (publish and subscribe) and `Presence` (join, heartbeat, leave,
and members) are service contracts. `LocalHub` implements both in
process. A member whose heartbeats stop is dropped after the timeout.

## Device desired state

- **Twin.** A `Twin<T>` holds the versioned desired state and the last
  reported state.
- **Agent.** The device's `DeviceAgent` reconciles through the
  application's `Actuate`. The hardware may clamp a value, and the report
  says what the device actually is.
  - Offline, the agent keeps the newest desire. Catching up applies only
    that one, not every desire it missed.
  - A change made on the device itself is settled by `DevicePolicy`
    (`DesiredWins` or `LastWriterWins`).
- **Messaging** (`bus`). Each message has a delivery guarantee
  (`AtMostOnce`, `AtLeastOnce`, or `ExactlyOnce`). The bus also supports
  retained values, a last will, and persistent sessions that queue while
  the device is away.
  - `Broker` holds these semantics.
  - `LocalBus` is an in-process client.
  - `mqtt` carries the same broker over MQTT 3.1.1: a packet codec, a TCP
    broker, and a client with acknowledgement and redelivery.
- **Data models.** `DeviceModel` maps typed state to a standard data
  model's resource paths (LwM2M/IPSO `object/instance/resource`).
- **Commissioning.** Joining a network and provisioning credentials are
  delegated to the existing stacks (Matter, LwM2M bootstrap, cloud device
  provisioning). This framework starts once the device can reach its
  broker.

## Examples

| Example | Shows |
|---|---|
| `examples/collab-notes` | Two devices edit one document offline and converge. `serve` runs the sync server. |
| `examples/live-counter` | A server-held counter that survives a reconnect and a deploy, shown by the Windows client |
| `examples/device-desired` | A device over MQTT that converges on its desired configuration after being offline |
