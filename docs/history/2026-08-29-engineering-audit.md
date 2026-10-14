# RustNative — In-Depth Engineering Audit

**Audit date:** 2026-08-29  
**Scope:** `RustNative.zip` as supplied in this conversation  
**Audit mode:** Read-only / no source rewrite performed  
**Target bar:** Production-grade Rust framework, not merely “compiles and passes the current tests”

---

## 1. Executive summary

RustNative has a **good architectural direction and a surprisingly substantial amount of real implementation**, especially for a project at this stage. The code demonstrates deliberate attention to ownership, component lifetimes, native-object ownership, task cancellation, tree reconciliation, layout separation, and FFI safety comments.

However, it is **not yet at the “top notch / production framework” bar** requested for this audit.

The most important reason is that several architectural choices currently make correctness depend on assumptions that are either too fragile, insufficiently enforced by types, or not covered by native integration tests. The largest issues are:

1. **Node/component identity is globally hash-based instead of structurally scoped.** Reusing an ordinary key such as `"root"` in two composed components can produce a duplicate global `NodeId` and trigger an assertion. More fundamentally, FNV-1a `u64` hashing cannot guarantee identity uniqueness.
2. **The Windows backend contains a very large unsafe/reentrancy boundary around raw `*mut Runtime`, `*mut Application`, `*mut WindowRegistry`, and `GWLP_USERDATA`.** The comments are strong, but the safety model is complex enough that it needs stronger structural encapsulation and dedicated native tests.
3. **The Windows file-dialog implementation uses `spawn_blocking` + per-call COM initialization as if the blocking pool supplied dedicated STA threads. It does not.** A Tokio blocking worker is reusable and may have an incompatible COM apartment already. The code also uses APIs Microsoft now recommends replacing with `IFileDialog`.
4. **The task-scope implementation retains every `TaskHandle` forever for the life of a component and therefore can grow without bound.** Cancellation also has a race/semantic gap between “cancel requested” and “result must never be delivered.”
5. **The framework core owns a process-global Tokio runtime with exactly two worker threads.** That is a strong global architectural commitment and can unnecessarily couple unrelated framework instances/applications.
6. **Effects use a 64-bit hash as dependency equality.** Hash collision means a dependency change can theoretically be missed; the API does not actually express equality semantics.
7. **Tree snapshots repeatedly scan the entire `HashMap` to reconstruct children.** Several tree operations therefore become O(n²) or worse for large UI trees.
8. **Layout invalidation is overly coarse:** every `TreeOp::Update` invalidates layout, including changes that cannot affect geometry.
9. **The Windows menu implementation has a real command-ID exhaustion bug:** `u16` IDs saturate at 65535 and subsequent menu items reuse the same command ID.
10. **Native menu handles and some failure paths lack complete RAII ownership.** A production FFI layer should make native resource ownership structurally impossible to leak.
11. **Accessibility is only partially realized.** The portable semantic model exists, but the Windows backend largely relies on native control defaults and does not implement the custom semantic bridge promised by the architecture. Some focusability transitions are not actually synchronized to native styles.
12. **The repository's engineering controls are incomplete:** the README documents files that are absent (`CI`, `rustfmt.toml`, `SECURITY.md`, `CONTRIBUTING.md`), there is no visible CI workflow, and `cargo-audit` is incorrectly listed as a rustup component.
13. **The current source is monolithic and several central types are multi-responsibility objects.** `rustnative-core/src/lib.rs` and `rustnative-windows/src/lib.rs` contain thousands of lines spanning many independent responsibilities. This is both a Single Responsibility / cohesion problem and a major maintainability and reviewability problem for a framework with unsafe/native boundaries. The strongest fix is not arbitrary file splitting, but deliberate responsibility boundaries with narrow interfaces.
14. **The current automated test suite is heavily biased toward core logic.** There is only one Windows-specific test in the backend, and no automated native Windows event-loop/resource/reentrancy suite.

The project should therefore be considered:

> **Architecturally promising, implementation-heavy, but not yet production-ready.**

The good news is that the underlying design can be improved without throwing away the entire framework. A rewrite of everything would be wasteful. The correct approach is a **targeted architectural hardening pass**, beginning with identity, scheduling/task ownership, native resource RAII, Windows threading, and test infrastructure.

---

## 2. Audit methodology

This audit intentionally follows the additional requirement:

> **Do not settle on the first approach. For important design decisions, compare alternatives and choose the approach that gives the strongest long-term correctness, safety, maintainability, and performance characteristics.**

The same rule is applied to responsibility decomposition: the audit does not equate Single Responsibility with “one type per file” or “one function per concern.” A good boundary is one where a component has one coherent reason to change, preserves its own invariants, and exposes a small interface to neighboring responsibilities. Where two responsibilities are inherently coupled, they should remain together behind a stable abstraction rather than being split mechanically.

The repository was inspected as a whole, including:

- workspace structure and Cargo manifests;
- lockfile and dependency declarations;
- Rust edition/MSRV/toolchain configuration;
- lint configuration;
- cargo-deny configuration;
- documentation and development plan;
- all Rust source files;
- unit and integration tests;
- Windows FFI boundaries;
- task scheduling and cancellation;
- component identity and reconciliation;
- layout and tree representation;
- theme/style realization;
- native window lifecycle;
- menus, dialogs, clipboard, notifications;
- error handling and panic boundaries;
- resource ownership;
- public API surface;
- Single Responsibility / cohesion / coupling across modules, types, and major methods;
- performance characteristics that can be established statically.

The archive itself was also integrity-checked successfully with `unzip -t`.

### Validation limitation

This audit environment does **not contain `cargo`, `rustc`, `rustfmt`, or Clippy**, so I could not independently execute:

```text
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo doc --workspace --no-deps
cargo deny check
cargo audit
```

The repository's `BUILD_STATUS.md` reports earlier Windows-side verification, but those results are treated as **historical evidence, not as a substitute for this audit's independent execution**.

The Windows backend additionally requires actual Windows execution for meaningful validation of message-loop, COM, HWND, GDI, menu, dialog, and reentrancy behavior.

---

## 3. Baseline inventory

### Workspace

The workspace contains three members:

- `rustnative-core`
- `rustnative-windows`
- `hello-label`

The root workspace uses:

- Cargo resolver 3;
- Rust edition 2024;
- declared MSRV 1.85;
- MIT OR Apache-2.0 licensing;
- workspace lints;
- `windows-sys` behind the Windows target dependency boundary.

### Source size

Approximate Rust source size inspected:

- `rustnative-core/src/lib.rs`: very large monolithic module;
- `rustnative-windows/src/lib.rs`: very large monolithic native backend;
- example application;
- core unit tests;
- lifecycle integration tests.

Static counts from the supplied archive:

- **41** `#[test]` functions in `rustnative-core/src/lib.rs`;
- **4** integration tests in `rustnative-core/tests/component_lifecycle.rs`;
- **1** Windows-specific test in `rustnative-windows/src/lib.rs`;
- approximately **9,438 Rust source lines** across the implementation/example/test files.

The test count is already meaningful, but the distribution is not sufficient for a native framework.

---

# 4. Findings

Severity definitions:

- **P0 — Critical:** correctness/safety/release blocker; resolve before production use.
- **P1 — High:** serious architectural, reliability, or maintainability issue; resolve before calling the framework production-grade.
- **P2 — Medium:** important quality/performance/design debt; should be resolved during hardening.
- **P3 — Low:** polish, ergonomics, or future-proofing.

---

## P0-01 — Global NodeId identity is architecturally unsafe

**Severity:** P0  
**Area:** component model / reconciliation / identity

### Evidence

`NodeId::from_key` hashes an arbitrary string into a single `u64` using FNV-1a (`rustnative-core/src/lib.rs`, around lines 26–38).

More importantly, component scoping explicitly leaves node IDs unchanged:

- `scope_component_node_ids` around lines 3710–3745;
- the code comments explicitly say a node's developer key remains its global ID;
- `rebuild_node_owners` then asserts if two component subtrees contain the same ID (around lines 1758–1769).

### Why this is a serious design flaw

A component framework needs **local identity inside a component** and a separate **global/native identity**.

The current design conflates those two concepts.

For example:

```text
Parent
├── ChildA
│   └── Button("submit")
└── ChildB
    └── Button("submit")
```

Using the same natural key inside two reusable components should be completely normal. In the current implementation, both can resolve to the same `NodeId`, and the framework can panic during owner reconstruction.

That means reusable components cannot safely use ordinary local keys unless every application author manually namespaces every key globally.

That is contrary to the purpose of component composition.

### Deeper problem: hash identity is not uniqueness

Even within one component, FNV-1a is a hash, not an identity mechanism. Two different strings can collide.

The framework cannot recover from that collision because it discards the original key after hashing.

### Better approaches considered

#### Approach A — Tell users to globally namespace keys

Example:

```text
settings.submit
profile.submit
```

**Rejected.** This leaks framework-global identity into every component and makes reusable components fragile.

#### Approach B — Keep FNV but combine it with ComponentId

For example:

```text
GlobalNodeId = hash(ComponentId, local_key)
```

**Better, but still not ideal.** Hash collisions remain possible, and a component-local key is still being converted into a probabilistic global identity.

#### Approach C — Structural opaque identity

Use something conceptually like:

```text
NodeIdentity {
    owner_component: ComponentId,
    local_key: KeyId,
}
```

where `KeyId` is interned/generated by the component runtime rather than being treated as a global hash.

The backend then gets a separate opaque `NativeNodeId` or stable realization ID.

**Recommended.** This cleanly separates:

- developer-visible local key;
- component ownership;
- framework identity;
- native object identity.

### Recommended end state

Do not make `NodeId` the public global namespace.

Use a two-level identity model:

```text
ComponentId
    +
LocalKey
    ↓
FrameworkNodeId
    ↓
NativeObjectId
```

A public event selector can still be convenient, but it should resolve through the owning component rather than requiring globally unique strings.

This change should happen **before adding more component-library functionality**, because everything above reconciliation depends on identity being correct.

---

## P0-02 — Raw-pointer Windows runtime architecture is too difficult to prove safe

**Severity:** P0  
**Area:** unsafe Rust / Win32 callbacks / lifetime management

### Evidence

`Runtime` contains:

```text
*mut Application
*mut WindowRegistry
```

and Win32 `GWLP_USERDATA` stores:

```text
*mut Runtime
```

The backend repeatedly converts these back into mutable references inside `WNDPROC` callbacks.

Examples are concentrated around:

- `Runtime` definition around lines 1689–1702;
- `WindowRegistry` around lines 1775–1785;
- `create_window_once` around lines 1833–1960;
- `window_proc_impl` around lines 2530 onward;
- `container_proc_impl` around lines 2834 onward.

### Positive assessment

This is **not careless unsafe code**.

The code has unusually detailed `SAFETY:` explanations and explicitly reasons about:

- boxed runtime stability;
- message-loop exclusivity;
- deferred window destruction;
- `WM_NCCREATE` initialization;
- `GWLP_USERDATA` invariants;
- callback panic boundaries.

That is good engineering.

### Why it is still a P0

The issue is not that a particular dereference is obviously wrong from inspection. The issue is that **too many invariants are global, informal, and reentrancy-dependent**.

The framework is relying on statements like:

> the runtime remains alive because `WindowRegistry` intentionally never removes it during the loop

and:

> the event loop has exclusive access

while callbacks can synchronously re-enter framework/application code.

That creates a very large proof surface.

Native callback code should be designed so that incorrect lifetime usage is difficult or impossible to express, rather than relying on hundreds of local safety comments to maintain one global invariant.

### Better approaches considered

#### Approach A — Keep current raw pointers and add more comments

**Rejected.** Documentation improves reviewability but does not reduce the unsafe surface.

#### Approach B — Wrap every raw pointer in `NonNull` and centralize access

**Good intermediate step.** This makes nullability and intent explicit, but still leaves the fundamental lifetime model.

#### Approach C — Native handle objects own callback state through explicit lifetime guards

**Recommended.** Build a small internal native layer where:

```text
Win32 callback
    ↓
NativeWindowContext::from_hwnd()
    ↓
validated runtime context
    ↓
safe framework operation
```

and keep all raw pointer manipulation in a very small module.

The callback should not know how `Application` is stored or how `WindowRegistry` is allocated.

### Required tests

This area needs Windows integration tests for:

- window creation reentrancy;
- window creation during another window's creation;
- self-close during event dispatch;
- parent closes while modal child exists;
- child closes itself;
- window creation failure after `WM_NCCREATE`;
- component panic during `WM_*` callback;
- renderer failure during synchronous creation messages;
- message delivery after native object removal.

Until those exist, the unsafe model remains too high-risk for a production framework.

---

## P0-03 — File-dialog COM threading assumption is incorrect

**Severity:** P0  
**Area:** Windows services / COM / threading

### Evidence

`WindowsFileDialogs::show` calls `run_blocking`, which uses:

```text
tokio::task::spawn_blocking
```

The comment describes the worker as a “freshly spawned, dedicated blocking-task thread.” That is not what Tokio's blocking pool guarantees.

Then `show_file_dialog` calls:

```text
CoInitializeEx(NULL, COINIT_APARTMENTTHREADED)
```

and treats any non-negative result as successful initialization.

### Why this matters

Tokio blocking threads are pool workers. They can be reused.

Microsoft documents that calling `CoInitializeEx` with an incompatible apartment model returns `RPC_E_CHANGED_MODE`. A thread can therefore already be initialized in a different model.

The current code then has this behavior:

```text
CoInitializeEx fails with incompatible apartment
        ↓
com_initialized = false
        ↓
show_file_dialog still proceeds
```

That is not a valid guarantee that the required COM apartment exists.

Microsoft also states that `SHBrowseForFolder` requires COM initialization and that `BIF_NEWDIALOGSTYLE` needs an STA model.

### Better approach

There are two independent design improvements.

#### 1. Do not use a generic blocking pool as an STA abstraction

Use a **dedicated Windows dialog thread** with a real message loop and a known COM apartment model, or marshal dialog requests onto the application's UI thread.

This is the stronger long-term architecture.

#### 2. Replace deprecated/legacy common-dialog APIs

Microsoft recommends `IFileDialog` for modern Windows versions instead of `GetOpenFileName` / `GetSaveFileName`, and recommends `IFileDialog` with `FOS_PICKFOLDERS` instead of `SHBrowseForFolder`.

The existing code explicitly chose the older APIs for perceived stability. That decision should be revisited against current Windows guidance.

### Recommendation

Create a Windows service host with explicit STA ownership:

```text
Framework service request
        ↓
Windows service dispatcher
        ↓
Dedicated STA/UI thread
        ↓
IFileDialog / Shell APIs
        ↓
Result marshalled back to framework scheduler
```

This is substantially stronger than trying to turn `spawn_blocking` into an STA abstraction.

Microsoft references:

- `CoInitializeEx` threading/apartment rules;
- `SHBrowseForFolder` COM requirements;
- `IFileDialog` recommendation over legacy common dialogs.

---

## P1-04 — Notification implementation has questionable Win32 identity/lifetime semantics

**Severity:** P1  
**Area:** Windows notifications

### Evidence

The notification implementation constructs `NOTIFYICONDATAW` with:

```text
hWnd = NULL
uID = 1
```

then calls:

```text
Shell_NotifyIconW(NIM_ADD, ...)
Shell_NotifyIconW(NIM_DELETE, ...)
```

immediately.

### Problems

Microsoft documents that the notification icon is identified using `hWnd + uID` or a GUID. The standard pattern uses a real window handle.

Also, deleting the icon is documented as causing associated balloon notifications to hide. Deleting the icon immediately after requesting the notification therefore makes “fire-and-forget notification” behavior questionable.

### Better approach

Do not emulate a notification API through a temporary tray icon unless that behavior is explicitly required.

Use a proper Windows notification implementation:

- Windows toast/App SDK/WinRT notification infrastructure where supported;
- explicit app identity and notification lifecycle;
- a fallback path only where required by the supported OS range.

If a tray-icon implementation remains necessary, create a real hidden message window and maintain the icon until the notification lifecycle is complete.

---

## P1-05 — TaskScope retains every TaskHandle forever

**Severity:** P1  
**Area:** scheduler / memory / lifecycle

### Evidence

`TaskScopeInner` contains:

```text
RefCell<Vec<TaskHandle>>
```

`TaskScope::spawn` pushes every handle into the vector.

Nothing removes completed handles.

### Impact

A long-lived component that performs repeated asynchronous operations accumulates:

- `TaskHandle`;
- Tokio `AbortHandle`;
- an `Arc<AtomicBool>`;

for every task it ever created.

This is an unbounded memory-retention problem.

### Better approach

Use an owned task registry keyed by `TaskId`, and remove entries when the task finishes or is cancelled.

A stronger design is:

```text
TaskScope
  ├── live task registry
  ├── cancellation token
  └── completion lifecycle
```

Completion should atomically transition the task from live → completed and remove it from the registry.

### Additional issue

`TaskHandle::cancel()` sets a flag and calls `AbortHandle::abort()`, but cancellation does not establish a strict “result can never be delivered” guarantee in a race with completion.

For deterministic structured concurrency, the scheduler should validate task generation/identity before enqueueing or delivering a result.

---

## P1-06 — Task cancellation documentation overstates preemption

**Severity:** P1  
**Area:** async correctness/documentation

The comment around `TaskHandle::cancel()` says the underlying Tokio task is aborted “pre-emptively at its next await point” and contrasts this with a future that never yields.

The wording still risks implying stronger cancellation than actually exists.

An async future executing CPU-bound code without yielding cannot be forcibly interrupted by Tokio cancellation.

### Recommendation

Document cancellation precisely:

```text
Cancellation requests task abortion. A task that is currently executing a
future cannot be synchronously preempted by the framework. Cancellation becomes
effective when the future reaches a cancellation point/yields according to the
executor's semantics.
```

For framework-level guarantees, separately guarantee that **completed/cancelled task results are not committed to dead components or superseded task generations**.

---

## P1-07 — Process-global two-thread Tokio runtime is too strong a core-level commitment

**Severity:** P1  
**Area:** architecture / scheduling

### Evidence

`rustnative-core::runtime()` creates a process-global `OnceLock<Runtime>` with exactly two worker threads.

### Problems

This means:

- two independent `Application` instances share the same executor;
- one application's CPU-heavy async workload can affect another;
- tests share global runtime state;
- the framework cannot naturally let the host application provide its executor;
- runtime lifetime is process-global instead of application-scoped;
- framework behavior is coupled to Tokio even though scheduling is conceptually an abstraction of the framework.

### Better approaches considered

#### A — Keep the global runtime

Simple, but not appropriate for a framework intended to be embedded.

#### B — One Tokio runtime per Application

Better isolation, but still forces Tokio as a framework architectural dependency.

#### C — Scheduler abstraction + host/runtime integration

Recommended.

The core should define something like:

```text
TaskExecutor / SchedulerBackend
```

and the default implementation can use Tokio.

Then:

- desktop application can use Tokio;
- tests can use deterministic/manual scheduling;
- another host can integrate an existing executor;
- future Web backend can map scheduling to browser semantics.

This is the strongest long-term architecture.

---

## P1-08 — Effect dependency tracking uses hash equality instead of semantic equality

**Severity:** P1  
**Area:** effects / correctness

### Evidence

`ComponentContext::effect` hashes `dependencies` with `DefaultHasher` and stores only the resulting `u64`.

### Problem

The API says “dependencies unchanged,” but the implementation actually says:

```text
hash(dependencies) unchanged
```

Those are not equivalent.

A hash collision can cause an effect to fail to restart when its dependencies changed.

This is unlikely with random application values but is unnecessary correctness risk in framework internals.

### Better approaches

- Require a dependency type with semantic equality and store an erased equality object.
- Introduce a dedicated `EffectDependencies` abstraction.
- Generate dependency versions explicitly from the application/component model.

The strongest option is an erased dependency value with equality semantics, because it directly models the API contract.

If hashing is retained for performance, use it only as a fast path followed by actual equality.

---

## P1-09 — Unknown targeted events are routed to the root component

**Severity:** P1  
**Area:** event routing / correctness

### Evidence

`ComponentTree::dispatch` does:

```text
known target → owning component
unknown target → ComponentId::ROOT
```

This happens around lines 1427–1436.

### Why this is dangerous

A targeted event whose node has disappeared should normally be rejected as stale.

Routing it to the root means a malformed or stale event can trigger unrelated root application logic.

### Recommended behavior

Distinguish:

```text
Targeted event + target exists     → route
Targeted event + target missing    → ignore/reject
Targetless event                   → root/window owner
```

This also makes stale native callbacks much safer during reconciliation.

---

## P1-10 — Duplicate identity failures rely on panics instead of structured errors

**Severity:** P1  
**Area:** API/error handling

Examples include assertions in:

- effect key registration;
- node owner rebuilding;
- component lookup assumptions;
- component tree rendering.

The framework's own `TreeSnapshot::from_node` has a structured `TreeError`, which is good, but the higher-level component system still uses `assert!` / `expect()` for invalid application states.

### Recommendation

Introduce a structured render/reconciliation error model.

For example:

```text
RenderError
├── DuplicateNodeIdentity
├── InvalidComponentComposition
├── DuplicateEffectKey
├── InvalidChildLifecycle
└── InternalInvariantViolation
```

Internal impossible states may still use `debug_assert!`, but user-triggerable invalid composition should return an error rather than panic.

---

## P1-11 — Tree representation causes avoidable O(n²) work

**Severity:** P1  
**Area:** performance / architecture

### Evidence

`TreeSnapshot` stores nodes in a `HashMap<NodeId, TreeNode>`.

Functions such as:

- `ordered_nodes`;
- `collect_ordered_nodes`;
- `ordered_children`;
- `depth`;

reconstruct hierarchy by repeatedly scanning the entire node map.

For example, `ordered_children` filters all snapshot nodes for each parent.

`depth` walks parent chains repeatedly.

### Impact

A tree with thousands of nodes can turn reconciliation/layout operations into quadratic work.

This is particularly relevant because the roadmap explicitly plans virtualized lists and large data sets.

### Better approach

Store structural indexes alongside nodes:

```text
TreeSnapshot
├── nodes: NodeMap
├── children: Parent → ordered ChildIds
├── parent: NodeId → ParentId
└── depth/cache if required
```

Or use an arena/tree representation where sibling order is intrinsic.

The important point is: **do not rebuild the tree topology by scanning the entire map.**

---

## P1-12 — Layout invalidation is far too coarse

**Severity:** P1  
**Area:** performance / rendering pipeline

`TreeDiff::invalidates_layout()` returns true for every `Update`.

But an update can be purely visual, accessibility-related, or otherwise non-geometric.

For example:

```text
foreground color changed
```

should not necessarily force a complete layout pass.

### Better approach

Classify changes:

```text
LayoutAffecting
├── size mode
├── constraints
├── margin/padding
├── text content
├── typography
├── children/order
└── overflow

PaintOnly
├── colors
├── border color
├── interaction state
└── visual-only properties

AccessibilityOnly
└── semantic metadata
```

Then propagate the minimum required invalidation.

This becomes increasingly important once animations and virtualized lists are added.

---

## P1-13 — Menu command IDs can overflow and alias

**Severity:** P1  
**Area:** Windows menus / correctness

### Evidence

`append_menu_item` uses a `u16` command ID and performs:

```text
next_command_id.saturating_add(1)
```

When the counter reaches `u16::MAX`, it stays there.

Subsequent menu items therefore reuse the same ID and overwrite the `HashMap<u16, NodeId>` entry.

### Recommended fix

Fail explicitly before exhaustion.

Better yet, use a dedicated command allocator with a clear platform range and return:

```text
MenuError::CommandIdExhausted
```

Do not silently alias commands.

Also ensure recursive submenu construction rolls back native resources on failure.

---

## P1-14 — Native menu resources lack complete RAII ownership

**Severity:** P1  
**Area:** Windows resource management

`build_native_menu` creates `HMENU` handles and recursively creates submenus.

On intermediate failure, the code returns an error without a clear ownership guard that recursively destroys all created menu handles.

The final `BuiltMenu` is attached to the window, but there is no explicit `DestroyMenu` ownership path visible in the backend.

### Recommendation

Introduce:

```text
OwnedMenu
OwnedPopupMenu
```

with `Drop` calling `DestroyMenu` where ownership has not been transferred.

When attached to a window, the ownership state should be explicit.

This is exactly the sort of resource where RAII should replace comments.

---

## P1-15 — Windows file-dialog APIs are legacy relative to the stated quality bar

**Severity:** P1  
**Area:** Windows API choice

The project intentionally chose:

- `GetOpenFileNameW`;
- `GetSaveFileNameW`;
- `SHBrowseForFolderW`.

Microsoft currently recommends the Common Item Dialog (`IFileDialog`) over the legacy Open/Save common dialogs and recommends `IFileDialog` with `FOS_PICKFOLDERS` over `SHBrowseForFolder` on modern Windows.

### Recommendation

Use COM RAII and the modern shell dialog interfaces.

This also creates a better place to implement:

- long-path support;
- modern shell navigation;
- richer filters;
- native async/task integration;
- proper parent-window ownership;
- cancellation semantics.

---

## P1-16 — Accessibility model is not actually fully realized

**Severity:** P1  
**Area:** accessibility

The framework has a good portable semantic model:

- role;
- name;
- description;
- focusability.

The roadmap explicitly says a full custom Windows UI Automation provider is future work, so this is not a claim that milestone 11 itself is invalid.

However, the production standard should be clearer: **portable semantic data existing in Rust is not the same thing as platform accessibility being correct.**

The Windows backend currently relies heavily on the default semantics of native controls.

Additionally, `sync_accessibility_state` adds `WS_TABSTOP` for some focusable buttons but does not symmetrically remove native focus styles when focusability changes. Text inputs are created with `WS_TABSTOP` regardless of the portable focusability flag.

### Recommendation

Define an explicit accessibility adapter boundary now, even if the full UI Automation provider remains future work.

Then test:

- role mapping;
- name mapping;
- disabled state;
- focusability;
- state changes;
- screen-reader-visible labels;
- custom semantic nodes once introduced.

---

## P1-17 — No native Windows integration-test harness

**Severity:** P1  
**Area:** testing / release engineering

The Windows crate contains only one test, and it checks capability flags.

That does not test the dangerous part of the backend.

The most failure-prone code is:

- HWND creation;
- synchronous Win32 callbacks;
- `GWLP_USERDATA`;
- reentrancy;
- menus;
- modal windows;
- destruction ordering;
- GDI lifetime;
- COM;
- clipboard ownership;
- file dialogs;
- scheduler wakeups.

### Recommendation

Add a Windows-only integration test executable that can run under a real Windows CI worker.

At minimum:

```text
native_window_lifecycle
native_child_reconciliation
native_reorder
native_text_input
native_focus_traversal
native_dynamic_window_creation
native_dynamic_window_close
native_modal_window_lifecycle
native_menu_dispatch
native_gdi_resource_lifecycle
native_task_wakeup
native_component_panic_boundary
```

Where GUI CI is difficult, use a dedicated Windows runner with deterministic scripted interactions rather than pretending compile-time tests cover native behavior.

---

## P1-18 — The scheduler has no deterministic virtual-time/test backend

**Severity:** P1  
**Area:** testing / async architecture

The framework scheduler is tied to Tokio's runtime and `tokio::time::sleep`.

This makes deterministic testing of time-based component behavior harder than necessary.

### Better approach

The scheduler abstraction recommended in P1-07 should allow:

```text
ProductionScheduler
DeterministicTestScheduler
ManualScheduler
```

The test scheduler should support:

- advance time;
- run pending tasks;
- cancel tasks;
- inspect task ownership;
- assert no task survives a scope;
- deterministically deliver completions.

This would dramatically strengthen lifecycle/effect testing.

---

## P1-19 — `DefaultIntrinsicMeasurer` has integer-overflow hazards

**Severity:** P1  
**Area:** layout correctness

The default intrinsic measurement performs arithmetic such as:

```text
characters as i32
characters * 8
lines * 32
```

without checked/saturating arithmetic.

Likewise, layout gap calculations multiply an `i32` gap by a child count cast to `i32`.

A pathological or adversarially large input can overflow in debug builds or wrap in release builds.

### Recommendation

Use bounded/saturating arithmetic throughout layout.

Better still, establish explicit layout limits at the framework boundary and validate them once.

A UI framework should have a defined maximum logical coordinate/size domain rather than allowing arithmetic overflow to determine behavior.

---

## P1-20 — `WindowId`, `ComponentId`, and task IDs wrap silently

**Severity:** P1  
**Area:** identity / correctness

The code deliberately uses wrapping increments for:

- component render generations;
- window IDs;
- task IDs.

Wraparound is unrealistic for normal operation, but it is still the wrong semantic behavior for identity allocators.

An identity allocator should either:

- return an exhaustion error;
- reserve/reuse released IDs safely;
- use a generation-indexed slotmap-style identity.

Do not silently reuse an old identity after `u64` wrap.

---

## P1-21 — Single Responsibility is not sufficiently enforced at the architectural boundaries

**Severity:** P1  
**Area:** architecture / cohesion / coupling / maintainability

### Finding

The project has a clear conceptual architecture, but the implementation does not consistently preserve **single, coherent reasons to change** at the module and type level. The strongest evidence is not simply file length; it is that several objects simultaneously own policy, state, orchestration, platform realization, and error/lifecycle decisions.

This matters more for a framework than for a small application. A framework's internal responsibilities become long-lived contracts: when rendering, scheduling, reconciliation, native realization, accessibility, and input routing share the same object, a change in one subsystem increases the chance of regression in the others.

Rust's module privacy system is particularly useful here because private modules and restricted visibility can make responsibility boundaries enforceable rather than merely documented. Rust's API guidance likewise emphasizes APIs that are coherent and easy to understand, while Microsoft's Rust guidance recommends balanced modules and clear subsystem boundaries.

### Core: `rustnative-core/src/lib.rs`

The core crate is approximately 4,500+ lines in a single source module and combines multiple independently evolving domains:

- component identity and lifecycle;
- component tree ownership/reconciliation;
- effect registration and cleanup;
- task scheduling and task cancellation;
- application/window orchestration;
- service registration and service implementations;
- theme/style definitions;
- layout primitives and layout execution;
- declarative node construction;
- tree snapshots and diffing;
- platform abstraction;
- tests.

The most important responsibility hotspots are (source locations are approximate and refer to the supplied archive):

- `ComponentTree` — `rustnative-core/src/lib.rs:1344–1850`;
- `Application` — `rustnative-core/src/lib.rs:1925–2184`;
- `Renderer` — `rustnative-windows/src/lib.rs:974–1677`;
- `Runtime` — `rustnative-windows/src/lib.rs:1689–1774`;
- `WindowRegistry` — `rustnative-windows/src/lib.rs:1775–1966`;
- Win32 callback/message handling — `rustnative-windows/src/lib.rs:2466–2906`.

#### `ComponentTree`

`ComponentTree` currently owns substantially more than one responsibility. Its methods cover:

- rendering components;
- maintaining component entries;
- component generation tracking;
- effect declaration/commit/cleanup;
- child component creation/pruning/removal;
- node ownership reconstruction;
- event dispatch;
- message queue draining;
- task pumping;
- exposing the rendered view.

These are related, but they are not one reason to change. In particular, **component lifecycle**, **tree reconciliation**, **effect lifecycle**, and **event routing** have different invariants and test strategies.

A failure in effect cleanup should not require understanding node ownership reconstruction; a change to event routing should not require modifying component rendering machinery.

#### `Application`

`Application` combines:

- multiple-window ownership;
- window ID allocation;
- window command queuing;
- component-tree orchestration;
- service/theme access;
- application-level event routing;
- render orchestration;
- scheduler access.

This is a legitimate application coordinator, so it should remain a higher-level object. The problem is that it currently knows too much about the internals of each subsystem rather than delegating to dedicated managers.

The correct target is **not** to eliminate `Application`; it is to make it an orchestration facade over narrower objects such as a window manager, component host/tree, command queue, and scheduler interface.

#### Service definitions and service implementations

The same file contains both service traits/abstractions and concrete in-memory implementations such as `MemoryStorage` and `MemoryClipboard`. Those implementations are useful for testing, but they should not make the service abstraction module responsible for its test/default implementations.

A better separation is:

```text
services/
    mod.rs          public service contracts + registry
    error.rs        service errors
    memory.rs       deterministic in-memory implementations
```

#### Layout

The layout subsystem is comparatively cohesive, but its public data model, style model, intrinsic measurement, invalidation, and engine implementation all live in the same monolithic module. This is less severe than `ComponentTree`, but it still makes the layout contract harder to evolve independently.

The recommended boundary is:

```text
layout/
    geometry.rs
    constraints.rs
    style.rs
    measure.rs
    engine.rs
    invalidation.rs
```

This is a **feature-oriented split**, not a file-size-driven split.

#### Node/component definitions

`Node`, `Label`, `Button`, `TextInput`, `Column`, `Row`, `TreeNode`, `TreeSnapshot`, and `TreeDiff` are all different layers of the rendering model but currently occupy the same source module.

The distinction should become explicit:

```text
Declarative node API
        ↓
Resolved/render node
        ↓
Snapshot
        ↓
Diff
        ↓
Platform realization
```

This reduces accidental coupling between the public declarative API and internal reconciliation structures.

### Windows backend: `rustnative-windows/src/lib.rs`

The Windows backend has an even stronger SRP problem because one file combines public platform services and a large native runtime.

The major responsibility clusters are:

- clipboard service;
- system URL launching;
- notifications;
- file dialogs and COM initialization;
- Windows error translation;
- platform capability discovery;
- native object registry;
- intrinsic measurement;
- GDI/style resources;
- renderer;
- layout realization;
- accessibility synchronization;
- scrolling;
- control creation;
- runtime orchestration;
- window registry;
- menu construction;
- keyboard/focus handling;
- Win32 class registration;
- WNDPROC exception boundaries;
- message translation.

The `Renderer` is the clearest "god object". It currently handles:

```text
TreeDiff operations
+ native HWND creation
+ native HWND destruction
+ native control updates
+ layout
+ positioning
+ scrolling
+ visual style realization
+ GDI resource ownership
+ accessibility state synchronization
+ interaction state synchronization
+ text measurement
```

Those responsibilities have different change drivers and different failure modes. For example:

- changing font caching should not modify accessibility code;
- changing layout should not modify HWND creation;
- changing scroll behavior should not modify GDI ownership;
- changing accessibility semantics should not require understanding native menu construction.

`Runtime` is a second coordinator hotspot. It currently coordinates rendering, relayout, event dispatch, task pumping, and window synchronization. This is acceptable for a top-level façade, but the individual operations should be delegated to dedicated subsystem objects.

`window_proc_impl` is also necessarily broad because Win32 delivers many message classes through one callback. **That is not itself a violation of SRP.** The correct decomposition is to keep the callback as a thin dispatcher and route message categories to focused handlers. Splitting the WNDPROC into dozens of unrelated callback functions without a coherent dispatch model would be worse.

### Approaches considered

#### Approach A — One type/function per responsibility, split aggressively

Example:

```text
Renderer::create_button
Renderer::apply_style
Renderer::sync_accessibility
```

becoming many tiny objects solely because the names differ.

**Rejected.** This creates indirection without improving cohesion. It also makes state ownership harder to understand.

#### Approach B — Split only by file size

For example, cut `lib.rs` every 500 lines.

**Rejected.** File size is not a responsibility boundary. This would produce arbitrary modules and likely increase coupling.

#### Approach C — Feature/bounded-context modules with focused internal types

Group code by stable domain boundaries, then split each domain where a type has a different reason to change.

Example:

```text
component/
    runtime.rs
    lifecycle.rs
    events.rs
    effects.rs

reconcile/
    snapshot.rs
    diff.rs
    identity.rs

layout/
    constraints.rs
    measure.rs
    engine.rs

platform/
    services.rs
    window.rs
    scheduler.rs
```

**Recommended.** This gives the best balance between discoverability, cohesion, privacy, and low coupling.

#### Approach D — Separate crates for every subsystem

For example, separate crates for layout, scheduler, component runtime, tree diff, services, and every Windows subsystem.

**Rejected for the current size.** Crate boundaries are stronger dependency boundaries than modules and introduce build/API/versioning overhead. Some future subsystems may deserve their own crate, but the present code should first achieve healthy module boundaries inside the existing crates.

### Recommended responsibility model

The strongest target is a **hybrid feature-oriented architecture with explicit coordinators**:

```text
rustnative-core
│
├── identity/             owns identity allocation + identity semantics
├── component/            owns component lifecycle/state
│   ├── runtime           orchestration only
│   ├── lifecycle         mount/update/unmount
│   ├── effects           effect declaration/commit/cleanup
│   └── events            event targeting/routing
├── reconcile/            owns declarative → resolved tree transformation
│   ├── snapshot
│   ├── diff
│   └── ownership
├── node/                 owns public declarative node model
├── layout/               owns geometry/layout only
├── style/                owns style/theme resolution
├── scheduler/            owns scheduling contract/task lifecycle
├── services/             owns service contracts/registry
├── window/               owns window-domain state/commands
└── application.rs        thin top-level orchestration facade

rustnative-windows
│
├── services/
│   ├── clipboard.rs
│   ├── dialogs.rs
│   ├── notifications.rs
│   └── system.rs
├── ffi/                  all raw Win32 declarations + unsafe adapters
├── resources/            HWND/HMENU/GDI/PIDL RAII wrappers
├── window/
│   ├── registry.rs
│   ├── lifecycle.rs
│   └── message_loop.rs
├── rendering/
│   ├── realization.rs
│   ├── controls.rs
│   ├── styling.rs
│   ├── accessibility.rs
│   └── scrolling.rs
├── input/
│   ├── keyboard.rs
│   ├── mouse.rs
│   └── focus.rs
├── menus/
│   └── builder.rs
└── platform.rs           thin public adapter
```

The exact names can change; the important part is the dependency direction and ownership.

### Responsibility rules to enforce

1. **One owner per invariant.** If two modules can mutate the same invariant, the boundary is probably wrong.
2. **Coordinators orchestrate; they do not implement every subsystem.** `Application` and `Runtime` should primarily sequence operations.
3. **Rendering should not own application scheduling.** Rendering consumes a resolved tree and produces native realization changes.
4. **Layout should not know about HWNDs.** The core layout engine should remain platform-independent.
5. **Accessibility semantics should not be implemented inside generic control creation.** Control realization may consume an accessibility projection.
6. **Input translation should produce framework events; component routing should decide where those events go.**
7. **Resource ownership belongs to RAII resource types, not to high-level renderer methods.**
8. **FFI safety belongs in a narrow module.** Safe higher-level modules should not repeatedly reconstruct raw-pointer invariants.
9. **Public APIs should be narrower than internal APIs.** Use `pub(crate)` / `pub(super)` aggressively to enforce boundaries.
10. **Tests should live near the responsibility they verify, with integration tests crossing boundaries intentionally.**

### Dependency-direction rule

The most important SRP improvement is not the folder tree; it is the dependency graph.

The intended direction should be approximately:

```text
Public API
   ↓
Application / Component Runtime
   ↓
Reconciliation / Layout / Scheduler / Services
   ↓
Platform abstraction
   ↓
Windows realization / FFI
```

There should be no upward dependency such as:

```text
layout → windows
renderer → component lifecycle internals
service implementation → application internals
FFI → public component API
```

If a subsystem needs information from a higher layer, introduce a narrow data projection or callback/trait rather than importing the higher-level implementation.

### How to know whether a future split is justified

Before creating a new module/type, ask:

1. Does it have a different reason to change?
2. Does it own a distinct invariant?
3. Can it expose a smaller interface than the current code?
4. Can it be tested independently?
5. Would isolating it reduce unsafe code or cross-domain knowledge?
6. Does the split reduce coupling rather than merely move code?

If the answer is mostly “no,” keep the code together.

### Recommended outcome

The goal is **high cohesion and low coupling**, not maximal fragmentation.

A good final architecture should make it possible for an engineer working on:

- layout;
- component lifecycle;
- scheduling;
- Win32 resource management;
- accessibility;
- menus;

to change that subsystem without needing to understand the entire framework.

This is particularly important for the unsafe Windows layer: reducing the number of responsibilities that can reach the FFI boundary is a direct safety improvement, not merely a code-organization preference.

---

# 5. Additional medium-severity findings

## P2-22 — Core implementation is too monolithic

`rustnative-core/src/lib.rs` contains unrelated domains including:

- component lifecycle;
- services;
- scheduler;
- effects;
- application/window management;
- styling;
- layout;
- tree representation;
- diffing;
- menus;
- tests.

`rustnative-windows/src/lib.rs` similarly contains:

- platform services;
- clipboard;
- dialogs;
- notifications;
- errors;
- renderer;
- layout integration;
- native resources;
- window registry;
- message loop;
- menu realization;
- WNDPROC implementation.

### Recommendation

Split into modules before the codebase grows further.

Suggested core structure:

```text
rustnative-core/src/
├── lib.rs
├── identity.rs
├── component/
│   ├── mod.rs
│   ├── context.rs
│   ├── lifecycle.rs
│   └── tree.rs
├── event.rs
├── node.rs
├── tree/
│   ├── snapshot.rs
│   └── diff.rs
├── layout/
│   ├── mod.rs
│   ├── constraints.rs
│   └── measure.rs
├── style/
│   ├── mod.rs
│   └── theme.rs
├── scheduler/
│   ├── mod.rs
│   └── task.rs
├── services.rs
├── application.rs
└── window.rs
```

Windows should similarly split FFI services from renderer and window/message-loop code.

This is not cosmetic. Smaller modules reduce the unsafe proof surface and make code review materially more reliable.

---

## P2-23 — Public API surface is insufficiently documented/enforced

The workspace does not enable `missing_docs` as a hard lint.

Static inspection finds a large number of public items without an immediately preceding rustdoc comment, including public structs, fields, constructors, service accessors, enums, and methods.

For a framework/library, public API documentation is part of correctness.

### Recommendation

Start with:

```toml
[workspace.lints.rust]
missing_docs = "deny"
```

Then explicitly allow/document internal exported types only where there is a strong reason.

Also add examples for major public APIs.

---

## P2-24 — Public data structures expose too much mutable design surface

Many public structures expose all fields directly, for example:

- `HttpRequest`;
- `HttpResponse`;
- `Color`;
- `Typography`;
- `VisualStyle`;
- `Theme`;
- `WindowState`;
- `TreeNode`;
- `TreeDiff`.

That makes future invariants and API evolution harder.

### Recommendation

Prefer private fields plus constructors/builders where invariants matter.

For framework internals, expose read-only accessors and narrowly defined mutation APIs.

This is especially important for identity, layout constraints, and native realization metadata.

---

## P2-25 — `TreeSnapshot` is mutable through overly broad public structures

`TreeNode` is a public data carrier containing fields that are essentially framework-internal realization state.

This allows downstream code to construct states that the renderer assumes were produced by the framework.

### Recommendation

Separate:

```text
DeclarativeNode
ResolvedNode
BackendNode
```

or make snapshot internals private and expose read-only accessors.

---

## P2-26 — `ServiceFuture<T>` is public but appears unused

`ServiceFuture<T>` is defined as a boxed future alias, while the actual service traits use `async_trait`.

This creates API redundancy and confusion.

### Recommendation

Either:

- use the alias as the canonical trait signature;
- or remove it.

Do not maintain two public async abstractions without a compelling reason.

---

## P2-27 — `async-trait` is an architectural choice that should be isolated

The comments correctly explain why `async-trait` was chosen for dyn-compatible services.

However, the service layer should own that compatibility decision rather than exposing it throughout the framework architecture.

### Better design

Define service traits around an explicit boxed future ABI at the framework boundary, or use an object-safe service adapter trait.

That makes the rest of the core independent from the macro's implementation details.

---

## P2-28 — `TreeDiff::Update` compares redundant style representations

`TreeNode` contains both:

- `style_override`;
- `visual_style`.

The diff compares both.

This is workable but makes semantic ownership unclear: which representation is authoritative for which phase?

### Recommendation

Explicitly model:

```text
StyleOverride
ResolvedStyle
InteractionResolvedStyle
```

and make the phase boundary impossible to misunderstand.

---

## P2-29 — Native object lookup is linear by HWND

`NativeObjectRegistry::id_for_hwnd` scans the entire `HashMap`.

For every native event this is potentially O(n).

### Recommendation

Maintain a reverse map:

```text
HWND → NodeId
```

with cleanup tied to native object RAII.

This is particularly important for mouse/focus events.

---

## P2-30 — Container background painting allocates a GDI brush on every erase

`WM_ERASEBKGND` creates a brush, fills, and deletes it on every call.

That is safe enough when successful, but unnecessary churn exists because the renderer already maintains style resources.

### Recommendation

Reuse the owned brush or use a dedicated paint-resource abstraction.

If a message-specific brush lifetime is required, document why the cached brush cannot be reused.

---

## P2-31 — Several Win32 return values are ignored

Examples include operations such as:

- `SetMenu`;
- `ShowWindow`;
- `SetWindowPos`;
- `EnableWindow`;
- `SetFocus`;
- `SendMessageW` in places where the return can matter;
- resource destruction calls.

Not every Win32 return value needs to become an error, but each ignored result should be deliberate.

### Recommendation

Create wrappers that classify Win32 results:

```text
MustSucceed
BestEffort
Informational
IgnoredByContract
```

This makes the decision explicit rather than implicit.

---

## P2-32 — Error type loses rich native context

`WindowsApi` stores only:

```text
operation: &'static str
code: u32
```

That is useful, but production diagnostics would benefit from:

- HRESULT where applicable;
- Win32 error category;
- window ID;
- node ID;
- native handle when safe to report;
- service operation;
- contextual source error.

### Recommendation

Use layered errors with `thiserror` or an equivalent explicit error enum rather than a single flat error representation.

Do not expose raw OS handles in `Display`, but keep them available to debug diagnostics where appropriate.

---

## P2-33 — Panic boundaries are inconsistent across the architecture

Windows catches panics at FFI callback boundaries, which is good.

But core component rendering and reconciliation can still panic in normal Rust execution through assertions and `expect()`.

The production architecture should have a consistent application-level error/panic policy.

### Recommendation

Define:

```text
Component panic
    ↓
error boundary
    ↓
application policy
├── terminate
├── close window
├── isolate component
└── report + continue
```

The policy should be configurable by the host.

---

## P2-34 — No fuzz/property testing for tree and layout invariants

The framework has many algebraic properties that are ideal for property testing:

- diff then apply preserves target tree;
- identity survives rerender;
- duplicate IDs are rejected;
- removal ordering is safe;
- layout never returns negative dimensions;
- constraints are respected;
- scroll ranges are non-negative;
- moving nodes preserves identity.

### Recommendation

Add property-based tests for core reconciliation/layout and fuzz malformed/generated trees.

This is likely to find bugs faster than adding more hand-authored examples alone.

---

## P2-35 — No benchmark suite despite explicit performance goals

The plan explicitly identifies tree diff, layout, native object creation, text measurement, event dispatch, scheduler overhead, scrolling, startup, and binary size as performance targets.

No benchmark harness is present in the supplied project.

### Recommendation

Add benchmarks for:

```text
TreeDiff::between
TreeSnapshot construction
Component render/reconcile
LayoutEngine::layout_result_with
Event dispatch
Task scheduling
Large sibling sets
Deep trees
```

Use representative sizes such as 10, 100, 1k, 10k nodes.

---

## P2-36 — Documentation and repository contents disagree

`README.md` lists:

- `.github/workflows/ci.yml`;
- `rustfmt.toml`;
- `SECURITY.md`;
- `CONTRIBUTING.md`.

Those files are absent from the supplied archive.

This is a release-quality hygiene issue because documentation is presenting repository state that is not actually present.

### Recommendation

Either add the files or update the README.

For a production project, the stronger choice is to add them.

---

## P2-37 — `rust-toolchain.toml` incorrectly treats `cargo-audit` as a rustup component

The toolchain file contains:

```toml
components = ["clippy", "rustfmt", "cargo-audit"]
```

`cargo-audit` is not a standard rustup component like Clippy or rustfmt.

Rustup's documented components include compiler/tooling components such as `clippy`, `rustfmt`, and `miri`; cargo-audit is a Cargo-installed tool.

### Recommendation

Use:

```toml
components = ["clippy", "rustfmt"]
```

and install `cargo-audit` explicitly in CI/tooling, preferably at a pinned version.

---

## P2-38 — Toolchain is “stable” rather than a reproducible compiler version

`rust-toolchain.toml` uses:

```toml
channel = "stable"
```

while Cargo declares `rust-version = "1.85"`.

These serve different purposes, but the current setup gives developers a moving compiler while the declared MSRV is fixed.

### Recommended model

Use CI to test both:

```text
MSRV: 1.85.x
stable: current
```

and optionally pin the development toolchain separately if exact reproducibility is required.

The lockfile should remain committed.

---

## P2-39 — No CI enforcement is present

There is no `.github/workflows/ci.yml` in the archive even though the README documents one.

Therefore there is no repository-local proof that every change must pass:

- formatting;
- compiler warnings;
- Clippy;
- tests;
- documentation;
- dependency audit;
- license checks;
- Windows compilation.

### Recommended CI matrix

At minimum:

```text
Ubuntu
  cargo fmt --check
  cargo check --workspace
  cargo test --workspace
  cargo clippy --workspace --all-targets --all-features -- -D warnings
  cargo doc --workspace --no-deps
  cargo deny check
  cargo audit

Windows
  cargo check --workspace
  cargo test --workspace
  cargo clippy --workspace --all-targets --all-features -- -D warnings
  native integration tests

MSRV
  cargo check --workspace
```

---

## P2-40 — Clippy policy is not yet strict enough for the stated standard

Current lints include:

- `unsafe_op_in_unsafe_fn = deny`;
- `undocumented_unsafe_blocks = deny`;
- a few warnings.

That is good, but insufficient for a framework claiming top-tier quality.

Do not enable every Clippy restriction group blindly. Clippy itself explicitly warns against enabling the entire restriction group.

### Better approach

Start with:

```text
clippy::all
clippy::pedantic
clippy::cargo
```

and then cherry-pick carefully selected restriction lints such as:

- `unwrap_used` where appropriate;
- `expect_used` where appropriate;
- `todo`;
- `unimplemented`;
- selected numeric/cast restrictions.

Every intentional exception should have a reason.

---

# 6. Strong points worth preserving

This project should **not** be rewritten wholesale. Several design decisions are genuinely strong.

## 6.1 Rust 2024 + resolver 3

The project is already aligned with modern Rust edition conventions.

Rust 2024 is the current modern edition line and is appropriate for a new framework.

## 6.2 Platform boundary is conceptually correct

`rustnative-core` does not directly import Win32 APIs.

The Windows backend is isolated behind `rustnative-windows`.

That is exactly the right direction for the stated architecture.

## 6.3 Native controls rather than canvas imitation

The project correctly chooses native Win32 controls rather than drawing an imitation toolkit.

That creates better alignment with platform behavior, accessibility, keyboard interaction, and native semantics.

## 6.4 Component-owned task scopes

The move toward structured task ownership is excellent.

The important invariant:

```text
component lifetime
    owns
async task lifetime
```

is the right abstraction.

The implementation needs to be hardened, not abandoned.

## 6.5 Effects have explicit cleanup

The effect lifecycle is thoughtfully designed:

```text
render
→ commit
→ cleanup previous
→ cancel previous effect tasks
→ start new effect
```

That is a strong foundation.

## 6.6 Native object identity is separate conceptually from Rust component identity

The architecture's intention to maintain stable framework IDs and reuse native objects is correct.

The identity implementation needs restructuring, but the principle should remain.

## 6.7 Native resources are generally treated as owned resources

GDI objects, clipboard locks, PIDLs, HWNDs, and similar resources are often accompanied by explicit cleanup reasoning.

The next step is to turn that reasoning into RAII types wherever possible.

## 6.8 FFI safety comments are unusually detailed

The `SAFETY:` comments are significantly better than typical application-level Win32 Rust.

They should be retained, but the goal should be to reduce the amount of unsafe code requiring such proofs by introducing safe internal wrappers.

## 6.9 The project already distinguishes transient interaction state

The decision not to rerender the whole component tree for scrolling/focus/hover is correct and important for performance.

## 6.10 Deferred dynamic window operations are the right idea

Deferring open/close operations instead of mutating the window registry in the middle of a component render is sound architectural thinking.

The native implementation needs more adversarial testing, but the abstraction itself should stay.

---

# 7. Standards benchmark

The audit benchmarked the code against the following classes of guidance:

### Rust language / edition

Rust 2024 is the appropriate current edition for a new project. The Edition Guide describes Rust 2024 as the latest edition line and explains its migration/compatibility model.

### Clippy

Clippy's normal correctness/suspicious/style/complexity/performance groups are valuable. Clippy explicitly advises **against** enabling the entire restriction group indiscriminately; restriction lints should be selected individually.

### Unsafe Rust

The Rust ecosystem strongly favors keeping unsafe code localized and documenting the invariants that make each block safe. The project is already following that spirit, but its native runtime needs further structural isolation.

### Microsoft Rust guidance

Microsoft's Pragmatic Rust Guidelines emphasize minimizing unsafe, avoiding unsafe as a workaround for type-system restrictions, using compiler/lint enforcement, and using tools such as Miri and cargo-hack where appropriate.

### Windows APIs

Microsoft's current Windows documentation recommends modern Common Item Dialog APIs over the legacy file dialog functions and documents explicit COM apartment requirements for shell APIs.

---

# 8. Recommended target architecture

The best long-term design I see after comparing the alternatives is:

```text
                          Application
                              │
                    ┌─────────┴─────────┐
                    │                   │
              Component Runtime    Platform Services
                    │                   │
          ┌─────────┼─────────┐         │
          │         │         │         │
       Identity   State    Scheduler    │
          │                   │         │
          │             Executor API    │
          │                   │         │
          ▼                   ▼         ▼
      Declarative        Task Scopes  Native Service Host
         Tree                 │         │
          │                   │         │
          ▼                   ▼         ▼
      Tree/Diff          Completion   Platform adapter
          │              ownership        │
          ▼                   │            ▼
        Layout                │        Windows/macOS/
          │                   │        Linux/Web/etc.
          ▼                   │
     Native realization ◄─────┘
```

Key principles:

1. **Each subsystem has one coherent responsibility and owns its invariants.**
2. **Local component identity, global opaque runtime identity.**
3. **No hash is treated as uniqueness without collision handling.**
4. **Scheduler is an abstraction; Tokio is an implementation.**
5. **Task ownership is RAII/registry-based and bounded.**
6. **Native resources are owned by dedicated safe wrapper types.**
7. **All Win32 raw pointers live in one small FFI module.**
8. **Windows services have explicit thread/apartment ownership.**
9. **Tree topology is indexed, not reconstructed by scanning maps.**
10. **Layout invalidation is incremental.**
11. **Application errors and component panics have an explicit policy.**
12. **Native behavior is tested on real Windows.**
13. **CI enforces the standards automatically.**

---

# 9. Recommended remediation order

Do **not** start by polishing individual functions. The order matters.

## Phase 1 — Correctness foundations

1. Redesign Node/Component identity.
2. Make targeted-event routing reject stale IDs.
3. Replace hash-only effect dependency semantics.
4. Fix task-scope retention and cancellation races.
5. Remove silent identity wraparound.
6. Fix menu command allocation.

## Phase 2 — Native safety architecture

7. Introduce RAII wrappers for HWND/HMENU/HFONT/HBRUSH/PIDL and similar resources.
8. Centralize all raw-pointer/GWLP_USERDATA access.
9. Reduce `*mut` pointers to the smallest possible unsafe boundary.
10. Add Windows reentrancy/lifecycle integration tests.

## Phase 3 — Windows service correctness

11. Replace generic blocking-pool COM assumptions.
12. Move dialogs to a proper STA host.
13. Adopt `IFileDialog`.
14. Redesign notifications using a supported modern notification mechanism.
15. Add real parent/owner HWND handling to native dialogs.

## Phase 4 — Core architecture/performance

16. Split the core and Windows monoliths using the responsibility/cohesion model above; make `Application`, `Runtime`, and other coordinators thin.
17. Index tree topology.
18. Improve diff invalidation classification.
19. Add reverse HWND lookup.
20. Add deterministic scheduler/test executor.
21. Add benchmarks.

## Phase 5 — Library quality

22. Enforce `missing_docs`.
23. Reduce public field exposure.
24. Remove unused/redundant public APIs.
25. Build structured error types.
26. Add examples to major public APIs.

## Phase 6 — Engineering controls

27. Add CI.
28. Fix rust-toolchain configuration.
29. Add MSRV + stable testing.
30. Run cargo-deny/audit in CI.
31. Add Windows CI.
32. Add security/contributing/release documentation.

## Phase 7 — Advanced verification

33. Property tests for reconciliation/layout.
34. Fuzz malformed trees and identity cases.
35. Native resource leak testing.
36. Long-running task stress tests.
37. Large-tree performance tests.
38. Accessibility integration tests.

---

# 10. What should NOT be rewritten

The following ideas should be preserved unless later evidence proves them wrong:

- native controls as the primary Windows realization;
- platform-independent framework core;
- declarative tree as the source of truth;
- keyed component reuse;
- typed props/messages;
- child-to-parent callbacks;
- component-owned async scopes;
- explicit effect cleanup;
- native transient interaction state;
- separate layout phase;
- native window ownership in the platform backend;
- deferred dynamic window operations;
- platform capability discovery;
- native escape hatch.

The implementation needs hardening, but these architectural principles are sound.

---

# 11. Release gate

I would **not** label the current archive production-ready yet.

The minimum release gate should be:

### Correctness

- [ ] Identity redesigned so reusable components may reuse local keys safely.
- [ ] No hash-only uniqueness assumptions.
- [ ] Stale targeted events rejected.
- [ ] Task completion cannot resurrect stale work.
- [ ] Task registry is bounded.
- [ ] Identity allocators cannot silently wrap.
- [ ] Menu IDs cannot alias.

### Unsafe/native

- [ ] Raw pointer usage isolated to a small FFI module.
- [ ] Native handles have RAII ownership.
- [ ] All callback lifetime invariants covered by tests.
- [ ] Reentrancy cases tested on real Windows.
- [ ] COM apartment ownership is explicit.

### Windows

- [ ] Modern file dialog implementation.
- [ ] Correct notification implementation.
- [ ] Menu resource cleanup verified.
- [ ] Native resource leak tests.
- [ ] Dynamic window lifecycle tests.
- [ ] Modal ownership tests.

### Architecture / responsibility

- [ ] Each major subsystem has a single coherent responsibility and owns its invariants.
- [ ] `Application` and native `Runtime` are orchestration facades rather than subsystem implementations.
- [ ] Component lifecycle, reconciliation, effects, event routing, layout, and scheduling have explicit boundaries.
- [ ] Renderer responsibilities are split between realization, controls, styling, accessibility, scrolling, and input as appropriate.
- [ ] Win32 FFI and native resource ownership are isolated behind narrow interfaces.
- [ ] Module privacy (`pub(crate)`, `pub(super)`, private fields) enforces the intended boundaries.

### Core

- [ ] Scheduler is injectable.
- [ ] Deterministic scheduler exists for tests.
- [ ] Tree topology indexed.
- [ ] Incremental layout invalidation exists.
- [ ] Overflow-safe layout arithmetic.

### API

- [ ] Public API documented.
- [ ] Public invariants enforced through types where practical.
- [ ] Structured framework errors.
- [ ] Panic policy documented.

### Tooling

- [ ] CI exists and is authoritative.
- [ ] `cargo fmt --check` enforced.
- [ ] `cargo clippy ... -D warnings` enforced.
- [ ] `cargo test --workspace` enforced.
- [ ] `cargo doc --workspace --no-deps` enforced.
- [ ] `cargo deny check` enforced.
- [ ] `cargo audit` enforced.
- [ ] MSRV tested.
- [ ] Stable tested.
- [ ] Windows tested on a real Windows runner.

---

# 12. Final assessment

## Overall rating: **6.5 / 10 for production-framework readiness**

This is **not** a rating of how much code exists. There is a lot of implementation here, and much of it is thoughtful.

The rating reflects the gap between:

```text
“substantial working framework prototype”
```

and:

```text
“framework I would trust as a production foundation for many applications.”
```

The project is strongest in:

- architectural intent;
- Rust ownership concepts;
- component lifecycle design;
- separation of platform and core concerns;
- deliberate native realization;
- explicit FFI safety reasoning.

It is weakest in:

- responsibility/cohesion boundaries and monolithic implementation structure;
- identity guarantees;
- native threading/COM correctness;
- structured async lifecycle rigor;
- native integration testing;
- resource RAII completeness;
- performance scalability of tree representation;
- repository/CI discipline;
- modularity of the implementation.

### Most important conclusion

**Do not rewrite the whole project.**

A wholesale rewrite would throw away a lot of good work and would probably reproduce the same architectural decisions in a less-tested codebase.

Instead, perform a **deep hardening rewrite of the framework's foundations**:

```text
Identity
   ↓
Task ownership / scheduler
   ↓
Native resource ownership
   ↓
Windows callback boundary
   ↓
Tree representation
   ↓
Testing / CI
   ↓
API hardening
```

After those foundations are corrected, the majority of the existing component, layout, theme, service, and window abstractions can be retained and improved incrementally.

That is the approach I consider materially better than either extreme of “just patch the current code” or “throw everything away and start over.”

---

# 13. Reference standards consulted

- Rust 2024 Edition Guide — `https://doc.rust-lang.org/edition-guide/rust-2024/`
- Rust Language Reference — `https://doc.rust-lang.org/reference/`
- Rust Reference — Visibility and Privacy — `https://doc.rust-lang.org/reference/visibility-and-privacy.html`
- The Rust Programming Language — Packages, Crates, and Modules — `https://doc.rust-lang.org/book/ch07-00-managing-growing-projects-with-packages-crates-and-modules.html`
- Rust API Guidelines — `https://rust-lang.github.io/api-guidelines/`
- Clippy documentation — `https://doc.rust-lang.org/clippy/`
- Rustup components — `https://rust-lang.github.io/rustup/concepts/components.html`
- Rustup toolchain files — `https://rust-lang.github.io/rustup/overrides.html`
- Cargo lockfile guidance — `https://doc.rust-lang.org/cargo/guide/cargo-toml-vs-cargo-lock.html`
- Microsoft Pragmatic Rust Guidelines — `https://microsoft.github.io/rust-guidelines/`
- Microsoft Pragmatic Rust Guidelines — Balanced Modules — `https://microsoft.github.io/rust-guidelines/guidelines/libs/ux/index.html`
- Microsoft `CoInitializeEx` documentation — `https://learn.microsoft.com/en-us/windows/win32/api/combaseapi/nf-combaseapi-coinitializeex`
- Microsoft COM library guidance — `https://learn.microsoft.com/en-us/windows/win32/com/the-com-library`
- Microsoft `SHBrowseForFolder` documentation — `https://learn.microsoft.com/en-us/windows/win32/api/shlobj_core/nf-shlobj_core-shbrowseforfoldera`
- Microsoft `Shell_NotifyIcon` documentation — `https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shell_notifyicona`
- Microsoft notification-area guidance — `https://learn.microsoft.com/en-us/windows/win32/shell/taskbar`
- Microsoft `NOTIFYICONDATA` documentation — `https://learn.microsoft.com/en-us/windows/win32/api/shellapi/ns-shellapi-notifyicondataw`
- Microsoft `GetOpenFileName` documentation — `https://learn.microsoft.com/en-us/windows/win32/api/commdlg/nf-commdlg-getopenfilenamea`

---

## Audit conclusion

**Current state:** strong prototype / early framework foundation.  
**Production state:** not yet.  
**Recommended strategy:** targeted foundational rewrite + native hardening + real Windows CI, not a total rewrite.  
**Highest priority:** identity, unsafe/native lifetime model, COM/dialog threading, task ownership, responsibility-boundary hardening, and native integration testing.
