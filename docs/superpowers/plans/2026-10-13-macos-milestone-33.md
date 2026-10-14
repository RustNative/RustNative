# Milestone 33 Implementation Plan (the macOS backend)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan phase-by-phase. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Status: planned, not started, and not executable on this project's current hardware.** Every phase after Phase 0 needs a Mac with Xcode (`PLAN.md` 2.13). Nothing below is a record of work done; `BUILD_STATUS.md` is where work done is recorded.

**Goal:** Build the macOS backend — `PLAN.md` "Milestone 33 — macOS backend" — to the definition of a complete backend in `PLAN.md` §8, including every §11 gate that exists when the work starts, and every macOS half another milestone records as owed.

**Architecture:** One backend crate, `rustnative-macos`, over the Apple platform-group crate it shares with iOS and iPadOS (`docs/conformance/platform-groups.md`):

- **`rustnative-apple`** (macOS, iOS, iPadOS): the Objective-C runtime bindings and the retain/release and autorelease-pool convention as one ownership module; main-thread affinity as a type (`MainThreadMarker` behind the core's `UiThread`); Core Text measurement behind `IntrinsicMeasurer`; the main run loop as the scheduler's wake source; the accessibility role table shared by `NSAccessibility` and `UIAccessibility`; Foundation locale, collation, and formatting; the Keychain; the Xcode toolchain driver. If iOS and iPadOS (Milestones 36 and 70) are built first, this crate exists already and macOS is its third member; if macOS is first, Phase 1 creates it.
- **`rustnative-macos`**: AppKit — `NSWindow` per window root, a framework container view (`RnLayoutView`, an `NSView` subclass that places children at the portable layout's frames and never runs Auto Layout over them), AppKit controls, the responder chain, `NSTextInputClient`, the macOS menu bar, `NSAccessibility`, and the macOS services, surfaces, and packaging — plus its own capability table, style table, unit mapping, conformance column, and budgets.

A backend never depends on another backend: `rustnative-macos` depends on `rustnative-apple` and `rustnative-core`, never on the UIKit crates. The host's iPad-to-Mac port of UIKit is not this backend (`PLAN.md` 2.2, Milestone 70).

**Tech Stack (to be confirmed when work starts):** Rust 2024 at the workspace MSRV; the `objc2` family — `objc2`, `objc2-foundation`, `objc2-app-kit`, `objc2-quartz-core`, `objc2-core-text`, `objc2-core-graphics`, `block2`, `dispatch2` (MIT/Apache-2.0) — at versions whose MSRV is at or under the workspace's; `raw-window-handle` 0.6 for native surfaces (`CAMetalLayer`). Rust targets `aarch64-apple-darwin` and `x86_64-apple-darwin`, shipped as one universal binary (`lipo`). The minimum macOS version is decided in Phase 0 against the features below and recorded in `docs/macos.md`.

**Spec:** `PLAN.md` §2 (2.2–2.5, 2.9, 2.10, 2.13, 2.14), §8 (the backend definition, the Apple paragraph, Milestone 33), §11 (Milestones 39–45, 58, 59, 61, 67); `docs/conformance/new-backend-checklist.md`; `docs/conformance/platform-groups.md`; `docs/conformance/permissions.md`; `docs/deploy/update-rules.md`; `docs/styling.md`; the owed lines of `BUILD_STATUS.md` naming Milestone 33, "the deferred backends", or "desktop".

## Scope decision (2026-10-13)

Written by the 2026-10-12 whole-framework review, which found Milestone 33 specified more thinly than Milestones 35 and 70 and without an implementation plan. Therefore:

- **This document is the plan, not the work.** No crate, test, budget file, or template exists for macOS yet. The code touchpoints are listed below.
- **In scope when work starts:** every Milestone 33 bullet; every item of §8's backend definition; the §11 gates that exist then; the macOS halves recorded as owed by Milestones 39–48, 50, 51, 57–59, 61, and 67; the Apple group crate if no Apple backend precedes it.
- **Out of scope, and why:** iOS and iPadOS (Milestones 36 and 70), beyond the Apple group crate; Mac Catalyst, which is UIKit and not this backend; executable updates for a Mac App Store build, which the store forbids (`docs/deploy/update-rules.md`).
- **Verification bar (2.13):** a compile check from Windows or Linux is not a verification; results recorded as verified are from a Mac. A recorded VoiceOver pass by a person is owed like Narrator's and Orca's.

## Hardware and accounts needed

| Need | Why | Minimum |
|---|---|---|
| A Mac with Apple silicon and current Xcode | building, signing, notarization, the device suite | one; shared with Milestones 36 and 70 |
| An Intel Mac, or Rosetta-free verification of the `x86_64` slice on one | the universal binary's second slice runs where it will run | one, or recorded as unverified |
| An Apple Developer Program membership | Developer ID signing, notarization, the Mac App Store | one, shared with 36 and 70 |
| A second display with a different backing scale | mixed-scale windows, a window moving between displays | one external display |
| A trackpad and a mouse | momentum scrolling, force touch, gestures, a wheel | both |

## Global Constraints

- Everything in the Linux and Android plans' Global Constraints holds: the core stays platform-free; MSRV as declared; `#![deny(missing_docs)]`; clippy pedantic clean with every `#[allow]` reasoned and every `unsafe` block carrying `SAFETY:`; `cargo deny check` clean; capabilities advertised only once realized; transient interaction never renders; both syntaxes and both style spellings for anything new.
- The workspace keeps building on Windows and Linux. The Apple crates' host dependencies are `[target.'cfg(target_vendor = "apple")'.dependencies]`; elsewhere each crate compiles to its portable half and `MacosPlatform::run` returns `Error::UnsupportedHost`. `rustnative-host` selects `MacosPlatform` on `target_os = "macos"` (today it answers `NoBackend` there, naming this milestone).
- AppKit is touched from the main thread only (`UiThread` from `MainThreadMarker`); a callback arriving while a render holds backend state enqueues and returns — the AppKit form of "post, never re-enter".
- Every Objective-C object the backend owns is held through the ownership module's retained handle; every callback into Rust runs inside an autorelease pool.
- Capability answers that differ by machine, macOS version, sandbox, or attached hardware are computed at run time and followed when they change.

## Verification gate

The workspace gate plus a macOS gate, `rustnative-tools/gate-apple.sh`, run on the Mac:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace                                  # the portable workspace, on macOS
cargo test -p rustnative-macos                          # the backend against real AppKit
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
cargo +<msrv> check --workspace --all-targets
cargo deny check
```

Before a Mac exists, `cargo check -p rustnative-apple -p rustnative-macos --target aarch64-apple-darwin` from Windows or Linux is expected to type-check the crates; whether it does is confirmed in Phase 0 and, if so, added to CI (which already checks the portable crates for the Apple targets).

## Crates

| Crate | Kind | Change |
|---|---|---|
| `rustnative-apple` | library (group) | new, unless an Apple backend precedes this one |
| `rustnative-macos` | library (backend) | new |
| `rustnative-host` | library | `HostPlatform` = `MacosPlatform` on macOS; its services |
| `rustnative-style` | library | `MACOS`, `MACOS_UNITS`; `macos` in `[style] targets` checked against the table |
| `rustnative-macros` | proc-macro | `table_for("macos")` |
| `rustnative-build` | library | `Info.plist`, entitlements, the asset catalogue, the privacy manifest |
| `rustnative` (CLI) | binary | `build`/`run`/`package macos`; Apple `doctor` rows; notarization |
| `rustnative-conformance` | library | the macOS column's compile-fail and equivalence cases |

### Code touchpoints, found while planning (2026-10-13)

- `crates/rustnative/src/platform.rs` — `Platform::Macos` gains its backend; `planned_milestone` stops returning 33.
- `crates/rustnative-host/src/lib.rs` — the `cfg(target_os = "macos")` arm and its `host` module.
- `crates/rustnative-macros/src/lib.rs` — `table_for` gains `"macos"`; `PLANNED_TARGETS` loses it.
- `crates/rustnative-core/src/capability.rs` — `GlobalMenuBar` is answered yes for the first time; `Surface(TrayExtra)` (the status item), `Surface(JumpList)` (the dock menu), and `Surface(TaskbarProgress)` (the dock tile's progress) are answered.
- `budgets/` — `macos.toml` with `SCHEMA.md`'s keys (Phase 8).
- `docs/deploy/update-rules.md` — the macOS rows become "built" in Phase 6.

---

## Phase 0 — Decisions and portable widenings (no Mac needed)

1. **The minimum macOS version**, against: `NSTextInputClient`, the accessibility protocols, `UNUserNotificationCenter`, the status item's menu, `NSWindow` tabbing, the privacy-manifest requirement.
2. **The `objc2` family's versions and MSRV**, and whether `cargo check --target aarch64-apple-darwin` works from Windows and Linux.
3. **The group decision** (if this is the first Apple backend): create `rustnative-apple` now, with its members' shared contract written down in `platform-groups.md`.
4. **Momentum scrolling in the portable wheel model**: `WheelDelta::Pixels` gains a phase (`Began`, `Changed`, `Ended`, `Momentum`) so a scroll container can tell the person's fingers from the momentum that follows, and rubber-banding is the host's — verified first on Windows' precision touchpads and GTK's kinetic scrolling.
5. **Document-window conventions in the portable document model** (Milestone 48): edited state, the represented file, autosave and versions as capability answers.

**Acceptance tests:** the workspace gates on Windows and Linux; the wheel phase delivered by the Windows and Linux translation layers with recorded input.

## Phase 1 — The Apple group crate

As `docs/superpowers/plans/2026-10-04-ipados-milestone-70.md` Phase 1 specifies `rustnative-apple` (ownership, `UiThread`, the run-loop executor and wake source, Core Text measurement with its cache invalidated by font and text-size changes, the role table, Foundation locale facilities, the Keychain, the Xcode driver), built once for every Apple backend.

**Acceptance tests:** a thousand views created and dropped with balanced retain counts; the measurement suite (complex scripts, bidirectional text, clusters, fallback, breaking) through Core Text; a wake from a worker thread renders exactly once on the main thread.

## Phase 2 — The crate and the realized tree

- **`MacosPlatform`**: `new()`, `with_app_id(id)` (single-instance through `NSRunningApplication` and the app's own Apple-event handler for launch URLs), `run`, `embed`/`start_external` (Phase 8), `capabilities` computed from the machine and followed, `style_capabilities` → `MACOS`, `unit_mapping` → `MACOS_UNITS`, `native_extension`.
- **Roots**: one `NSWindow` per window root, its content view an `RnLayoutView`; `WindowState` from the window (minimized, zoomed, full screen); modal windows as sheets or modal windows per the portable modal parent.
- **Node kinds**: label → `NSTextField` (label style), button → `NSButton`, text input → `NSTextField`, multi-line text → `NSTextView` in a scroll view, toggle → `NSSwitch`, check box and radio → `NSButton` types, slider → `NSSlider`, progress → `NSProgressIndicator`, select → `NSPopUpButton`, list box → `NSTableView`, date picker → `NSDatePicker`, tab bar → `NSTabView`'s segmented control, image → `NSImageView`, separator → `NSBox`; a native registry with one owner per view and recycling.
- **Style**: `rustnative_style::MACOS` — framework-owned boxes realize colours, borders, per-corner radii, shadows, and opacity through their `CALayer`; native controls realize what their appearance API offers (colours on text, `bezelColor` on buttons, fonts) and answer the rest unavailable for the control (2.2). `MACOS_UNITS`: one logical pixel is one point; `rem` follows the person's text-size preference where the host has one; positions round to the backing store's pixel grid (1/`backingScaleFactor`) half away from zero; a window moving to a display of another scale restyles and remeasures on the same views.

**Acceptance tests:** the shared guarantee suites (Milestone 41) over an AppKit harness; `the_macos_capability_table_is_what_appkit_paints`; fidelity against the host's own applications.

## Phase 3 — Input

- Keys through the responder chain to the portable key model (with the Command key as `KeyModifiers::meta` and shortcuts matched in the menu bar's terms); `NSTextInputClient` for custom text targets (marked text as `Composition`); Full Keyboard Access as Milestone 61's non-pointer navigation.
- Pointer: mouse and trackpad to the portable pointer model, hover through tracking areas (`Hover`), per-node cursors through `NSCursor` (`Cursors`), pressure from force-touch trackpads (`Pen` answered no — a trackpad's pressure is not a pen), magnify and rotate gestures to the framework's recognizers, scroll with Phase 0's phases and momentum.
- Drag and drop with `NSDraggingDestination` and `NSPasteboard`, multi-item.
- Gesture arbitration: scroll versus pan against `NSScrollView`'s own elasticity, each case named in `docs/macos.md`.

**Acceptance tests:** `a_shortcut_reaches_its_command_from_the_menu_bar_and_the_keyboard`; `marked_text_arrives_as_composition`; `momentum_scrolling_is_the_hosts_and_ends_with_its_phase`; drag-and-drop with recorded pasteboard items.

## Phase 4 — Accessibility

The portable `AccessibilityTree` through `NSAccessibility`: native controls keep their elements and are overridden only where the model states something; framework-owned boxes and virtual elements are `NSAccessibilityElement`s with frames in screen coordinates; actions, values, relationships, live regions as announcements, and notifications posted after a render. Verified with the Accessibility Inspector's interfaces from a test, and with VoiceOver by a person.

**Acceptance tests:** the accessibility assertion suite over the AppKit harness; `virtual_elements_have_screen_frames_on_every_display`.

## Phase 5 — The menu bar, windows, and lifecycle

- The portable `MenuBar` as the application's main menu with the application-menu conventions (About, Settings, Services, Hide, Quit) supplied by the backend; command-bound items following their commands as the menu opens; `GlobalMenuBar` answered yes.
- Window tabbing and full screen as `WindowMode` values; window placement saved and restored through the host's restoration mechanism (Milestone 39).
- Lifecycle: `applicationWillTerminate`, sleep and wake (`NSWorkspace` notifications), App Nap, and memory pressure (`DispatchSource` memory-pressure events) as `Lifecycle` events, flushed first.

**Acceptance tests:** `the_application_menu_follows_the_hosts_conventions`; `sleep_flushes_and_reports_suspending`; `memory_pressure_reports_low_memory`.

## Phase 6 — Services, surfaces, and updates

- Services through the portable contracts: clipboard (`NSPasteboard`), file dialogs (`NSOpenPanel`/`NSSavePanel`, security-scoped bookmarks inside the sandbox, `DocumentService` over them), URL opening and notifications (`NSWorkspace`, `UNUserNotificationCenter` with actions as messages), the share sheet (`NSSharingServicePicker`, anchored to its node), HTTP with certificate pins (`URLSession`), permission states through TCC (the mapping in `permissions.md`), Keychain secure storage, crash-safe state files under `~/Library/Application Support/<app-id>`, locale formatting, printing (`NSPrintOperation`), serial ports (IOKit), constrained work.
- Surfaces (Milestone 57): the status item and its menu (`TrayExtra`), the dock menu (`JumpList`), dock-tile progress (`TaskbarProgress`), notifications with actions; widgets through WidgetKit's restricted tree where the host version offers them.
- Crash capture (Milestone 51) through the portable crash report, written by a signal and exception handler into the application's support folder, read by `rustnative crash`.
- Updates (Milestone 50): outside the Mac App Store, the shared signed-manifest verifier, staged rollout, and side-by-side bundles swapped atomically; inside it, the store's own update and model payloads only.

**Acceptance tests:** each service's tests against the real host; the permission mapping checked against TCC's answers; `an_update_is_verified_staged_switched_and_rolled_back_when_it_fails` on macOS.

## Phase 7 — Animation, lists, canvas, surfaces, host content

`Timeline` frames from `CADisplayLink` (macOS 14+) or a display link per display; reduced motion from the accessibility setting; virtual lists with anchoring and view recycling; the canvas drawn with Core Graphics and Core Text, with virtual elements; native surfaces as `CAMetalLayer`-backed views told their scale when it changes; host content (`WKWebView`, `AVPlayerView`, the camera preview layer); per-property mappers.

## Phase 8 — The §11 gates

- **Inspection** (Milestone 44): realized views, the census, the overlay.
- **Embedding** (Milestone 40): our tree in a caller's `NSView`, an `NSView` adopted as a leaf, guest-runtime mode under the host's run loop; `examples/adoption-appkit`.
- **Developer loop** (Milestone 43): `rustnative dev macos` with state-preserving restart within its budget, and live token edits.
- **Conformance** (Milestone 41): the macOS column of `new-backend-checklist.md`, every row naming its test.
- **Interaction depth** (Milestone 61): scroll coordination with momentum, keyboard navigation, the host's text-editing surface (the services menu, dictation, the character palette).
- **Budgets** (Milestone 42): `budgets/macos.toml` — startup phases, resident memory, artifact size of the universal binary, frame time during a transition, input latency — enforced by `rustnative bench --target macos`.
- **Extended style range** (Milestone 67): answered per property, for native controls and framework-owned boxes.

## Phase 9 — Toolchain, packaging, documentation, and the full verification

- **CLI**: `rustnative build|run|package macos`; `doctor` rows for Xcode, the SDK, the signing identity, and the notarization credential; the template's macOS target.
- **Packaging** (Milestones 32, 50, 59): an `.app` bundle with `Info.plist` generated from `rustnative.toml` (identity, version, URL types, document types), entitlements from the capability answers (the sandbox, network, files, camera), the privacy manifest from the build (Milestone 51), the asset catalogue and icon; the universal binary; Developer ID signing with the hardened runtime, notarization and stapling, a `.dmg` or `.pkg`; Mac App Store submission through Milestone 59's path with a managed credential.
- **Documentation**: `docs/macos.md`; `README.md`, `PLAN.md`, and `BUILD_STATUS.md`.

**Done when** `PLAN.md` §8's definition holds for macOS on a Mac: every Milestone 33 bullet realized or answered as an honest capability, the checklist column complete, the budgets met, and `BUILD_STATUS.md` recording what ran on which machine and macOS version.
