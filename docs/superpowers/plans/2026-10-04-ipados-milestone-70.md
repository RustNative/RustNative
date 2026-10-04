# Milestone 70 Implementation Plan (the iPadOS backend)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan phase-by-phase. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Status: planned, not started, and not executable on this project's current hardware.** Every phase after Phase 0 needs a Mac with Xcode, and every phase from Phase 2 onward needs an iPad to be verified (`PLAN.md` 2.13). Nothing below is a record of work done; `BUILD_STATUS.md` is where work done is recorded.

**Goal:** Build the iPadOS backend — `PLAN.md` "Milestone 70 — iPadOS backend" — to the definition of a complete backend in `PLAN.md` §8, including every §11 gate that exists when the work starts, and every iPadOS half another milestone records as owed.

**Architecture:** One new backend crate, `rustnative-ipados`, over two platform-group crates it shares with the other Apple targets (`PLAN.md` §8, `docs/conformance/platform-groups.md`):

- **`rustnative-apple`** (macOS, iOS, iPadOS): the Objective-C runtime bindings and the host's retain/release and autorelease-pool convention written down as one ownership module; main-thread affinity as a type (`MainThreadMarker` behind the core's `UiThread`); Core Text measurement behind `IntrinsicMeasurer`; the main run loop as the scheduler's wake source; the portable accessibility model's role and trait mapping to the Apple vocabulary; locale, collation, and number formatting through Foundation; the Keychain; and the Xcode toolchain driver used by `rustnative`.
- **`rustnative-uikit`** (iOS, iPadOS): the realization of node kinds as UIKit views and controls inside a framework container view (`RnLayoutView`) that places children where the portable layout engine says; the scene bridge; `UITextInput` for custom text targets; the touch, gesture-recognizer, and drag bridge; `UIAccessibility` elements, including virtual elements; `CADisplayLink` pacing; and the UIKit services both targets realize identically.
- **`rustnative-ipados`**: everything that makes the iPad a different host — scene sessions as windows the person arranges, `WindowMode` and size classes that change while the application runs, the keyboard as primary input (commands, the main menu, the focus system), the indirect pointer, the precision pen, cross-application drag and drop, the moving software keyboard, popover-anchored services, the sidebar as `UISplitViewController`, external displays — plus its own capability table, style table, unit mapping, conformance column, and budgets.

A backend never depends on another backend: `rustnative-ipados` depends on the group crates and `rustnative-core`, never on `rustnative-ios`. A universal application links both mobile backends and a launcher in `rustnative-uikit` selects one by `UIDevice.userInterfaceIdiom` in `application(_:didFinishLaunchingWithOptions:)`, before any scene connects and any tree is realized. The two targets share `target_os = "ios"`, so the selection is a runtime decision and nothing in either backend or in the style checker may key on `cfg(target_os)` to tell them apart.

**Tech Stack (to be confirmed when work starts):** Rust 2024 (the workspace MSRV at the time, 1.85 today); the `objc2` family — `objc2`, `objc2-foundation`, `objc2-ui-kit`, `objc2-quartz-core`, `objc2-core-text`, `objc2-core-graphics`, `objc2-core-foundation`, `block2`, `dispatch2` (MIT/Apache-2.0) — resolved to versions whose MSRV is at or under the workspace's and whose bindings cover the iPadOS SDK features below; `raw-window-handle` 0.6 for native surfaces (`CAMetalLayer`). Rust targets `aarch64-apple-ios` (devices) and `aarch64-apple-ios-sim` (the simulator on Apple silicon); there is no separate iPadOS target triple. The minimum host version is decided in Phase 0 against the features listed (scene sessions, pointer interaction, the focus system, the keyboard layout guide, geometry requests, pencil hover and squeeze) and the host's installed base at the time, and recorded in `docs/ipados.md`; anything newer than the minimum is answered per host version, never assumed.

**Spec:** `PLAN.md` §2 (2.2–2.5, 2.9, 2.10, 2.13, 2.14), §8 (the backend definition, the Apple paragraph, Milestones 33, 36, and 70), §11 (the gates of Milestones 39–45, 58, 59, 61, 67); `docs/conformance/new-backend-checklist.md`; `docs/conformance/platform-groups.md`; `docs/conformance/permissions.md`; `docs/deploy/update-rules.md`; `docs/styling.md`; `docs/ecosystem-analysis/mobile.md` (`M-OB-*`, `M-TB-*`); the owed lines of `BUILD_STATUS.md` naming Milestone 70, Milestone 36, "the deferred backends", or "device targets".

## Scope decision (2026-10-04)

The user's instruction: *expand the macOS and iOS backends with iPadOS, at the same level as the other two; plan it thoroughly; there is no Apple hardware, so it is planned only; update every plan and doc file; do not touch code.* Therefore:

- **This document is the plan, not the work.** No crate, test, budget file, template, or CLI value exists for iPadOS on the day it is written. The code touchpoints are listed below so the first session that has the hardware does not rediscover them.
- **In scope when work starts:** every Milestone 70 bullet; every item of §8's backend definition; the §11 gates that exist then; the iPadOS halves recorded as owed by Milestones 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 50, 51, 57, 58, 59, 61, and 67; the UIKit group crate (`C65`), which whichever of Milestones 36 and 70 is written second forces — and since the two are planned as one track, the decision is taken now (see Phase 1).
- **Built together with Milestone 36.** iOS and iPadOS share a toolkit, a compiler target, a bundle, and a store record. Building them apart means building the UIKit group twice or moving code between crates after the fact. The recommended order is Phases 0–1 of this plan, then Milestone 36's realization and this plan's Phases 2–9 interleaved, with each target's own column, table, and budgets kept separate throughout.
- **Out of scope, and why:** the macOS backend (Milestone 33) beyond the Apple group crate it shares — it is AppKit, with its own plan when its hardware exists; the host's iPad-to-Mac UIKit port, which is not the macOS backend (`PLAN.md` 2.2, Milestone 70); visionOS and other hosts that run iPad applications in a compatibility layer, which are answered by this backend's capabilities and are not targets; executable over-the-air updates, which the host forbids (`docs/deploy/update-rules.md`).
- **Verification bar (2.13):** a compile check is not a verification, a simulator run is a verification of the simulator, and a remote build is not a verification (Milestone 50). Each phase names what the simulator can prove and what needs the device; the device results are what `BUILD_STATUS.md` records as verified. Anything the hardware available at the time cannot exercise — a particular pencil, an external display, a low-end model — is implemented, capability-answered from the running device, tested at the translation layer with recorded input, and recorded as unverified on hardware. A recorded VoiceOver pass on an iPad is owed to a person, as Narrator's and Orca's are.

## Hardware and accounts needed

| Need | Why | Minimum |
|---|---|---|
| A Mac with Apple silicon and current Xcode | building, signing, the simulator, `xcrun devicectl` | one; also serves Milestone 33 |
| An Apple Developer Program membership | device signing, TestFlight, push, store billing sandbox, App Store Connect | one, shared with 33 and 36 |
| An iPad that runs the host's resizable-window mode with an external display | multiple scenes, live resize, external display, scale changes | one recent M-series iPad |
| A pencil that reports pressure, tilt, and hover (and, if available, roll and squeeze) | the pen phase | one, matched to the iPad above |
| A keyboard with a trackpad | keyboard-first input, pointer, hover, pointer lock | one |
| An external display | external-display scenes and scale changes | one |
| A low-end iPad | the low-end budget profile (`M-OB-4`) | one entry-level model, or recorded as owed |
| An iPhone | the iOS half of the universal bundle and the idiom selection | shared with Milestone 36 |

## Global Constraints

- Everything in the 39–58 plan's and the Linux plan's Global Constraints still holds: the core stays platform-free (no `cfg(target_os)` item in `rustnative-core`); MSRV as the workspace declares it; `#![deny(missing_docs)]`; clippy pedantic clean with every `#[allow]` reasoned and every `unsafe` block carrying `SAFETY:`; `cargo deny check` clean; capabilities advertised only once realized; transient interaction never renders; both syntaxes and both style spellings for anything new.
- The workspace keeps building and testing on Windows and Linux exactly as before. The Apple crates' host dependencies are `[target.'cfg(target_vendor = "apple")'.dependencies]`; on any other OS each crate compiles to its portable half and `IpadOsPlatform::run` returns `Error::UnsupportedHost`, mirroring `WindowsPlatform` on Linux and `LinuxPlatform` on Windows.
- UIKit is touched from the main thread only. The backend asserts it through the core's `UiThread`, built from a `MainThreadMarker`, and every UIKit callback that can arrive while a render holds backend state enqueues and returns — the UIKit form of the Windows backend's "post, never re-enter" rule.
- Every Objective-C object the backend owns is held through the ownership module's retained handle; nothing is released by hand outside it, and every callback that crosses into Rust runs inside an autorelease pool.
- Capability answers that differ by device, host version, window mode, attached accessory, or display are computed from the running device and followed when they change — never from the build, and never from the model name.
- Nothing distinguishes iPad from iPhone through `cfg`. The launcher's idiom check is the only place the distinction is made; everything downstream asks the backend it was handed.

## Verification gate

The workspace gate (`rustnative-tools/gate.sh` on Windows, `gate-linux.sh` in WSL) plus a macOS gate, `rustnative-tools/gate-apple.sh`, run on the Mac:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace                                                    # the portable workspace, on macOS
cargo clippy -p rustnative-ipados --target aarch64-apple-ios --all-targets -- -D warnings
tools/apple-sim.sh ipad cargo test -p rustnative-ipados --target aarch64-apple-ios-sim   # an iPad simulator
tools/apple-device.sh ipad cargo test -p rustnative-ipados --target aarch64-apple-ios    # the attached iPad
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
cargo +<msrv> check --workspace --all-targets
cargo deny check
```

`apple-sim.sh` boots (or reuses) a named iPad simulator through `xcrun simctl`, wraps the test binary in a minimal signed test-host bundle, installs it, launches it with the test arguments, and streams its output and exit status back; `apple-device.sh` does the same on the attached iPad through `xcrun devicectl`. Device-only tests are marked and skipped with a recorded reason on the simulator, never silently passed.

Before any Mac exists, `cargo check -p rustnative-apple -p rustnative-uikit -p rustnative-ipados --target aarch64-apple-ios` from Windows or Linux is expected to type-check the crates (the `objc2` family declares its bindings in Rust and links frameworks only at link time); whether it does is confirmed in Phase 0 and recorded. It is a compile check and nothing more.

## Crates

| Crate | Kind | Change |
|---|---|---|
| `rustnative-apple` | library (group) | new: the Apple group crate (Phase 1) |
| `rustnative-uikit` | library (group) | new: the UIKit group crate (Phase 1), shared with Milestone 36 |
| `rustnative-ipados` | library (backend) | new |
| `rustnative-core` | library | the portable widenings of Phase 0; no platform code |
| `rustnative-style` | library | `IPADOS`, `IPADOS_UNITS`; the `ipados` target name |
| `rustnative-macros` | proc-macro | the `ipados:` target variant; the unavailable-property check by declared target, not by `cfg(target_os)` |
| `rustnative-build` | library | Apple resources: `Info.plist`, entitlements, the scene manifest, asset catalogues, privacy manifest |
| `rustnative` (CLI) | binary | `Platform::Ipados`, `build`/`run`/`package ipados`, Apple `doctor` rows, the template's iPad target |
| `rustnative-windows`, `rustnative-linux`, `rustnative-web`, `rustnative-headless` | library | fill the pen widening where their host reports it; answer `WindowMode::Overlay` |
| `rustnative-conformance` | library | the iPadOS column's compile-fail and equivalence cases |

### Code touchpoints, found while planning (2026-10-04)

So the first session does not rediscover them:

- `crates/rustnative/src/platform.rs` — `Platform` gains `Ipados` (`ipados`, "iPadOS (Milestone 70)"); `planned_milestone` returns 70 until a backend exists; the test that every backend-less platform names its milestone includes it.
- `crates/rustnative/tests/cli.rs` — the platform/milestone table gains `("ipados", "Milestone 70")`.
- `crates/rustnative/src/doctor.rs` — the per-platform readiness rows gain iPadOS beside macOS and iOS.
- `crates/rustnative/src/toolchain/mod.rs` — the toolchain table's Apple row reads "macOS, iOS, iPadOS | Xcode and the Apple SDKs | Milestones 33, 36, 70".
- `crates/rustnative/src/error.rs` — `NoBackend` already carries a milestone number; no change beyond a test case.
- `crates/rustnative-core/src/environment.rs` — `WindowMode` (today `Full` and `Split { fraction }`) gains `Overlay`.
- `crates/rustnative-core/src/input/pointer.rs` — `PointerEvent` carries pressure only; the pen widening of Phase 0 lands here.
- `crates/rustnative-core/src/capability.rs` — `MultipleWindows`, `WindowManagement`, `WindowPlacement`, `ServerSideDecorations`, `GlobalMenuBar`, `Hover`, `Cursors`, `Pen`, `CommandShortcuts`, `DragAndDrop`, `Printing`, and `Surface(_)` already exist and are answered, not added; `PointerLock` is the one candidate addition (Phase 0).
- `crates/rustnative-components/src/widgets.rs` — `NavigationStyle::Sidebar` is what Phase 2 realizes as `UISplitViewController`.
- `budgets/` — no `ipados.toml` yet (none exists for any Apple target); Phase 8 creates it with `SCHEMA.md`'s keys.
- `docs/api/framework.json` and `rustnative describe` — regenerated when the capability and environment widenings land.

---

## Phase 0 — Portable widenings and decisions (no Apple hardware needed)

Everything here is portable, verified on the backends that exist (Windows, Linux, the Web, headless), and can be done before a Mac arrives — which is 2.13's point: hardware changes the order, not the plan.

**Design.**
1. **The target.** `Platform::Ipados` in the CLI, recognized and failing with "Milestone 70" until the backend exists; `ipados` in `[style] targets`; the `ipados:` target variant in both style spellings, with equivalence cases. The macro's unavailable-property check is reworked to select tables by the project's declared targets rather than by `cfg(target_os)`, because `ios` and `ipados` share one compiler target (the Linux plan's per-OS `compile_error!` does not extend to them).
2. **`WindowMode::Overlay`** — a window floating over another application's (the iPad's overlay window; a desktop's always-on-top utility window). Windows and Linux answer it where their window managers produce it, or never; headless can set it; the Web never.
3. **The pen widening of `PointerEvent`**: tilt as altitude and azimuth (with a conversion from the tilt-X/tilt-Y pair Windows and the browser report), barrel roll, hover distance (`None` when touching or unknown), and the coalesced and predicted samples behind each delivered event. Windows fills it from `POINTER_PEN_INFO`, Linux from GDK's axes, the Web from `PointerEvent`'s `altitudeAngle`/`azimuthAngle`/`twist`, `getCoalescedEvents()`, and `getPredictedEvents()`; headless records whatever a test supplies. **Pen actions** (a pencil's double-tap and squeeze, a pen's barrel button) are delivered as commands bound through the command model rather than as a new event family.
4. **`Capability::PointerLock`** — the pointer captured and hidden, delivering relative motion. Answered on Windows (`ClipCursor` plus raw input) and the Web (Pointer Lock API) if they realize it, otherwise no; decided here so iPadOS answers an existing capability.
5. **Anchored presentation.** The portable share and picker requests carry an optional source node; on hosts that present them as popovers it is required, and the headless backend fails a request without one when it is told it is emulating such a host. Windows, Linux, and the Web ignore it.
6. **Per-window lifecycle.** The portable lifecycle distinguishes a window root being backgrounded or disconnected from the process being suspended or terminated, and a restored window root from a fresh one. The headless lifecycle-collision suite (Milestone 45) gains the scene cases: several windows restored after process death, a deep link arriving at a window mid-restoration, and a window discarded by the person while the process is not running.
7. **A headless iPad profile.** `rustnative-headless` gains a profile that emulates what the portable halves of this milestone need — window-mode and size-class changes while running, several window roots, pointer hover, pen samples, a moving keyboard inset, anchored-presentation enforcement — so Phases 2–7's portable logic has tests that run on every machine.
8. **Decisions recorded.** The minimum host version; the `objc2` family versions and their MSRV; whether `cargo check --target aarch64-apple-ios` works from Windows and Linux (and if it does, a CI job that runs it on every push); the UIKit group decision (Phase 1) in `platform-groups.md`.

**Files:** `crates/rustnative/src/{platform,doctor}.rs`, `crates/rustnative/src/toolchain/mod.rs`, `crates/rustnative/tests/cli.rs`, `crates/rustnative-core/src/{environment,capability,lifecycle}.rs`, `crates/rustnative-core/src/input/pointer.rs`, `crates/rustnative-style/src/…`, `crates/rustnative-macros/src/…`, `crates/rustnative-headless/src/…`, each shipped backend's pointer translation, `docs/api/framework.json`, `docs/styling.md`.

**Acceptance tests:** the workspace gates pass on Windows and Linux; `rustnative build ipados` fails naming Milestone 70; each shipped backend's capability test names `PointerLock`; a pen sample with tilt round-trips through Windows' and Linux' translation layers with recorded input; the `ipados:` variant resolves in both spellings and is absent from every other backend's resolution; the headless iPad profile's scene-collision cases pass.

---

## Phase 1 — The Apple and UIKit group crates

**Design.**
- **`rustnative-apple`**: the ownership module (retained handles, autorelease pools at every entry from the host, weak references for delegates, the convention asserted in tests); `UiThread` from `MainThreadMarker`; an `Executor` and wake source on the main run loop (a `CFRunLoopSource` signalled from any thread, drained on the main thread), honouring the core's non-`Send` executor seam; a Core Text measurer (`CTFramesetter` for bounded and wrapped measurement, line metrics, the font cascade for fallback) with a cache invalidated by font and text-size changes; the accessibility role and trait table, from the portable roles to the Apple vocabulary, shared by `NSAccessibility` and `UIAccessibility` front ends; Foundation locale, collation, number, and date formatting; Keychain storage; the Xcode driver (`xcodebuild`, `xcrun`, `simctl`, `devicectl`, `codesign`, `altool`/`notarytool` where relevant).
- **`rustnative-uikit`**: the realized tree — `RnLayoutView`, a container view that places its children at the portable layout's frames and never runs Auto Layout over them; the node-kind table (label → `UILabel`, button → `UIButton` with a configuration, text field → `UITextField`, multi-line text → `UITextView`, toggle → `UISwitch`, slider → `UISlider`, progress → `UIProgressView`, list → a recycled cell host in `UIScrollView`, image → `UIImageView`, …); the native registry with one owner per view and recycling; the scene bridge (an application delegate and a scene delegate class registered once, forwarding to the selected backend); the idiom launcher; `UITextInput` for custom text targets; the touch bridge and the gesture-recognizer arbitration against host recognizers; `UIDragInteraction`/`UIDropInteraction` behind the portable drag model; `UIAccessibilityElement` subclasses for virtual elements and posted notifications for property, layout, and announcement changes; `CADisplayLink` pacing; the shared UIKit services (clipboard, notifications, URL opening, permissions, the share sheet and pickers).
- **The group decision.** Recorded in `platform-groups.md` before either mobile backend is written: Apple = macOS, iOS, iPadOS; UIKit = iOS, iPadOS. Both crates are created now rather than when a second member lands, because both members are specified and built together; this is the one deviation from the rule, and the record says why.

**Simulator proves:** measurement against Core Text, the realized tree's frames, ownership and recycling counts, run-loop wakes. **Device proves:** the same, and measurement agreement between simulator and device.

**Acceptance tests:** an ownership test that creates and drops a thousand views and balances retain counts; a measurement suite (complex scripts, bidirectional text, clusters, fallback, breaking) through Core Text; a wake from a worker thread renders exactly once on the main thread; the idiom launcher selects the iPadOS backend on an iPad simulator and the iOS backend on an iPhone simulator from one bundle.

---

## Phase 2 — The iPadOS crate and the realized tree

**Design.**
- **`IpadOsPlatform`** (`platform.rs`): `new()`, `with_app_id(id)`, `run`, `capabilities` (computed from the device, the host version, the scene's current mode, attached keyboards and pointers, the pencil's reported features, and the connected displays, and followed when they change), `style_capabilities` → `IPADOS`, `unit_mapping` → `IPADOS_UNITS`, `native_extension`.
- **Roots**: each connected `UIWindowScene` gets a `UIWindow` whose root view controller hosts the window root's `RnLayoutView`; the portable window root's identity survives a scene disconnecting and reconnecting.
- **The iPad's own controls**: `NavigationStyle::Sidebar` → `UISplitViewController` in its column style, collapsing to a stack when the window's horizontal size class becomes compact and expanding back without losing the selected route; popovers anchored to their source node, falling back to the host's own adaptive presentation in compact width; context menus through `UIContextMenuInteraction` from the portable menu model; toolbars and the navigation bar from the command model.
- **Style**: `rustnative_style::IPADOS` — on framework-owned boxes, colours, borders, per-corner radii, shadows, opacity, and gradients through the view's `CALayer`; on native controls, what the control's configuration or appearance API realizes, and the rest answered unavailable for the control (2.2). `IPADOS_UNITS`: one logical pixel is one UIKit point; `rem` is 16 points scaled by the person's preferred content size relative to the default; positions and sizes round to the display's pixel grid (1/scale of a point) half away from zero.

**Simulator proves:** every node kind realized, sidebar collapse and expansion across a size-class change, the style table read back from the layers and controls. **Device proves:** fidelity against the host's own applications (row 1 of Milestone 41's suites).

**Acceptance tests:** the shared guarantee suites (Milestone 41) over the iPad simulator harness; `the_ipados_capability_table_is_what_uikit_paints` (read back from the layer and control properties); a sidebar test that resizes a window through compact and regular width and asserts the same view controllers and the same route.

---

## Phase 3 — Windows the person arranges

**Design.**
- `MultipleWindows` through scene sessions: opening a window root activates a scene session (with a user activity carrying the root's identity), closing one requests its destruction, and a scene the host connects on its own (from the application switcher, a drag that creates a window, an external display) is matched to a window root by that identity or offered to the application as a new one.
- `WindowMode` from the scene's state — full screen, side-by-side split with its fraction, the overlay window, a freely resized window — and size classes per axis from the trait collection, both delivered through the typed environment and followed while the application runs (invalidation limited to readers, Milestone 39).
- Live resize: a resize relayouts on the same views, at the display's frame rate, with no render beyond the readers of what changed (2.10); geometry requests (`WindowManagement`, approximated) through the host's request API, with a refusal delivered as an outcome.
- External displays: a scene on an external display is an ordinary window root with its own scale and traits; moving a window between displays restyles and remeasures on the same views, like Linux mixed DPI.
- Restoration: each scene's state through its restoration activity, decoded into the portable restoration contract (Milestone 30); the scene collision cases from Phase 0 run against the real host.

**Simulator proves:** split, overlay, and resizable-window modes the simulator offers; size-class changes; restoration after the simulator kills the process. **Device proves:** live resize at the device's refresh rate, external displays, discarding from the application switcher, the multitasking memory limit.

**Acceptance tests:** `a_window_mode_change_reaches_its_readers_only`; `live_resize_keeps_every_view_and_meets_the_frame_budget`; `a_disconnected_scene_restores_its_window_root`; the lifecycle conformance suite's scene cases on the device.

---

## Phase 4 — Input

**Design.**
- **Keyboard.** Commands with shortcuts become `UIKeyCommand`s on the focused responder chain, so the host's shortcut overlay lists exactly the commands that work; the portable `MenuBar` is built into the main menu through `UIMenuBuilder` (and rebuilt when commands change), shown as a menu bar on host versions that have one (`GlobalMenuBar` answered per host version). Full keyboard navigation through the host's focus system — focus groups from the portable focus model, Tab and arrow movement, the focus ring on framework-owned boxes — as Milestone 61's non-pointer navigation. Key events (`pressesBegan`/`pressesEnded`) to the portable key model with modifiers, and hardware-keyboard composition through `UITextInput`.
- **Pointer.** `UIPointerInteraction` on nodes that declare hover or a cursor; hover through hover recognition (`Hover` realized); per-node cursors mapped onto the host's pointer styles — the text beam, hidden, system shapes, and custom shapes — with the mapping table in `docs/ipados.md` and `Cursors` answered approximated; native controls keep the host's own pointer effects untouched; a secondary click opens the node's context menu; wheel and two-finger scrolling through scroll views and, for the framework's own pan recognizers, through their allowed scroll types, delivered as `WheelDelta::Pixels`; trackpad pinch and rotation through the same recognizers as touch; `PointerLock` through the view controller's pointer-lock preference, answered from whether the scene is eligible.
- **Pen.** Pencil touches as `PointerKind::Pen` with Phase 0's fields — force normalised to the portable pressure range, altitude and azimuth, roll where reported, hover distance from pencil hover — and the coalesced and predicted touches attached; the pencil interaction's double-tap and squeeze to commands; scribble into native text fields left to the host, and indirect scribble registered for the framework's custom text targets; the host's ink canvas available as adopted host content through Milestone 40.
- **Drag and drop.** Cross-application drags, multi-item sessions, and spring-loaded navigation targets through the UIKit group's drag bridge, with the iPad's extra cases (dropping onto a window to create one).
- **Gesture arbitration.** The framework's recognizers against the host's system gestures and edges, with each conflict case named in `docs/ipados.md` and tested through recorded input.
- **Keyboard avoidance.** The keyboard layout guide (following an undocked keyboard) feeds the system insets; docked, floating, and split keyboards, and the shortcut bar alone with a hardware keyboard attached, are each a test case (Milestone 61).

**Simulator proves:** key commands, the main menu, the focus system, pointer hover with the simulator's pointer, recorded pen samples through the translation layer, drag within and between simulator applications, keyboard insets for each keyboard state. **Device proves:** trackpad gestures and scrolling feel, pencil pressure, tilt, hover, roll, and squeeze, pointer lock, latency.

**Acceptance tests:** `a_shortcut_shown_in_the_overlay_is_the_one_that_works`; `the_main_menu_follows_its_commands`; `keyboard_navigation_reaches_every_focusable_node_in_order`; `pen_samples_carry_tilt_hover_and_coalesced_points`; `a_floating_keyboard_moves_the_insets_not_the_layout_root`; gesture-arbitration conflict cases.

---

## Phase 5 — Accessibility

**Design.** The portable `AccessibilityTree` through `UIAccessibility` from the UIKit group: native controls keep their own elements and are overridden only where the model states something; framework-owned boxes and virtual elements become accessibility elements with frames in screen coordinates (correct in every window mode and on external displays); container ordering, actions as custom actions, adjustable values, live regions as announcements, and layout and screen-changed notifications posted after a render, never inside one. iPad paths verified beside VoiceOver: Full Keyboard Access (the focus system, Phase 4), pointer accessibility, Switch Control, and Voice Control's names and numbers across several open windows.

**Simulator proves:** the element tree, frames, traits, and notifications, read back through the host's accessibility inspection interfaces. **Device proves:** VoiceOver speech and navigation — a recorded pass by a person, owed like Narrator's and Orca's.

**Acceptance tests:** the accessibility assertion suite (Milestone 41) over the iPad harness; `virtual_elements_have_screen_frames_in_every_window_mode`; `announcements_are_posted_after_the_render`.

---

## Phase 6 — Services and surfaces

**Design.**
- **Anchored services**: the share sheet, document picker, photo picker, and print interaction presented as popovers from their source node (Phase 0's anchor), adapting in compact width the host's way.
- **Documents**: the document browser and the host's file provider, in-place opening, security-scoped access with the grant's lifetime bound to the portable document (Milestone 48); recent documents.
- **Shared with Milestone 36 through the UIKit group**: clipboard, notifications (with actions as messages), URL opening, universal links and URL schemes routed to the scene they target, permissions through the portable states with the mapping documented in `permissions.md` (and only iPad differences noted), Keychain storage, HTTP with certificate pins, constrained background work, locale facilities.
- **The iPad's own**: printing (`Printing`), external displays (a window root per display scene; a non-interactive presentation scene where the application asks for one), picture-in-picture for host media content.
- **Surfaces (Milestone 57)**: widgets from the restricted tree in every family the iPad offers, including the extra-large one; live activities where the host version offers them on an iPad; share and action extensions; remote push; store billing with server-side validation; each answered for iPadOS separately, and each extension generated as its own target from `rustnative.toml`.

**Simulator proves:** presentation and anchoring, document flows, notifications, widgets, extensions, the billing sandbox. **Device proves:** push delivery, external displays, picture-in-picture, Keychain under device lock.

**Acceptance tests:** `a_share_request_without_an_anchor_is_refused_before_it_reaches_the_host`; `a_document_grant_ends_with_its_document`; per-surface tests with recorded payloads; the permission mapping table checked against the states the device reports.

---

## Phase 7 — Animation, lists, canvas, surfaces, host content

**Design.** `Timeline` frames from `CADisplayLink` with the preferred frame-rate range set from the animation's needs (up to the display's maximum), and reduced motion from the host's setting; virtual lists with anchoring and view recycling in `UIScrollView` (the Linux and Windows contracts); the canvas drawn with Core Graphics and Core Text into a layer, with virtual elements for accessibility; native surfaces as `CAMetalLayer`-backed views told their scale when it changes (external displays); host content (`Web` through `WKWebView`, `Media` through the host's player with picture-in-picture, `Camera` through the capture preview layer); per-property mappers over the UIKit realization.

**Acceptance tests:** a timeline paced at the display's rate in the simulator and on device; virtual-list anchoring under insertion; canvas drawing goldens; a native surface following a display change.

---

## Phase 8 — The §11 gates

**Design.**
- **Inspection** (Milestone 44): realized views, the census, the overlay drawn in a framework-owned layer above the window root.
- **Embedding** (Milestone 40): our tree into a caller-supplied `UIView` or view controller, and a `UIView` adopted as a leaf; the library-only mode; an adoption example (`examples/adoption-uikit`).
- **Developer loop** (Milestone 43): `rustnative dev ipados` against the simulator and against a paired iPad, with state-preserving restart within the `dev_loop_restart_ms` budget, and live theme edits.
- **Conformance** (Milestone 41): the iPadOS column of `new-backend-checklist.md`, every row naming its test; text, layout under text scaling and pseudo-localization, accessibility, modal operation, and host-object lifetime; fidelity against the host's own applications.
- **Interaction depth** (Milestone 61): scroll coordination, system insets and keyboard avoidance, gesture physics, the host's text-editing surface (selection, the edit menu, scribble), and non-pointer navigation.
- **Budgets** (Milestone 42): `budgets/ipados.toml` — startup phases, resident memory under a split-screen memory limit, artifact size of the iPad slice, frame times during a live resize, pointer and pencil input latency — on the low-end and high-end profiles, enforced by `rustnative bench --target ipados` on the device.
- **Lifecycle conformance** (Milestone 45): the scene cases on the device.

**Acceptance tests:** each row of the checklist column; `rustnative bench --target ipados --check` passing on both profiles.

---

## Phase 9 — Toolchain, packaging, documentation, and the full verification

**Design.**
- **CLI**: `rustnative build|run|package ipados`; `Platform::Ipados` gains its backend; `doctor` rows for Xcode, the iOS SDK, simulators, a paired iPad, the signing identity, and the provisioning profile; `rustnative new` templates that declare `ios` and `ipados` together by default, with an iPad layout example.
- **Packaging** (Milestones 32, 50, 59): `Info.plist` generated from `rustnative.toml` — device family (iPad, or iPhone and iPad for a universal bundle), the scene manifest with multiple-scene support, all four orientations, document types and in-place opening, URL types and associated domains — entitlements from capability answers, the privacy manifest from the build (Milestone 51), the asset catalogue with iPad icons and the store's iPad screenshot sizes among the generated launch assets; signing with a managed credential; IPA output; TestFlight upload; store submission through Milestone 59's path.
- **Documentation**: `docs/ipados.md` (the guide — targets and the universal bundle, windows the person arranges, the keyboard, pointer, and pen, the cursor mapping, gesture conflicts, the capability and style tables, the unit mapping, packaging); `README.md` and `PLAN.md` status; `BUILD_STATUS.md`'s entry with what was verified, on which devices and host versions, and what was not.
- **The full verification**: the Apple gate on the Mac; the simulator suite on at least two iPad simulator sizes; the device suite on each iPad in the hardware table, with the pencil, the keyboard and trackpad, and the external display attached; the recorded VoiceOver pass; the budgets on both profiles.

**Done when** `PLAN.md` §8's definition holds for iPadOS on real iPad hardware: every Milestone 70 bullet realized or answered as an honest capability, the checklist column complete, the budgets met, and `BUILD_STATUS.md` recording what ran on which device.
