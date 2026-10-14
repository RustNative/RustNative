# Milestone 36 Implementation Plan (the iOS backend)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan phase-by-phase. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Status: planned, not started, and not executable on this project's current hardware.** Every phase after Phase 0 needs a Mac with Xcode, and every phase from Phase 2 onward needs an iPhone to be verified (`PLAN.md` 2.13).

**Goal:** Build the iOS backend — `PLAN.md` "Milestone 36 — iOS backend" — to `PLAN.md` §8's definition, with every §11 gate that exists when the work starts and every iOS half another milestone records as owed.

**Built together with Milestone 70.** iOS and iPadOS share a toolkit, a compiler target (`target_os = "ios"`), a universal bundle, and a store record, so they are one track with two targets (`docs/superpowers/plans/2026-10-04-ipados-milestone-70.md`, "Scope decision"). That plan specifies what both share — the Apple group crate (`rustnative-apple`), the UIKit group crate (`rustnative-uikit`) with the realized tree, the scene bridge, the idiom launcher, `UITextInput`, the touch and drag bridge, `UIAccessibility` elements, `CADisplayLink` pacing, and the UIKit services — and this plan specifies **what is the iPhone's own**: `rustnative-ios`, its capability table, style table and unit mapping, conformance column, budgets, and verification, which no iPad result can stand in for.

**Recommended order:** iPadOS plan Phases 0–1 (the portable widenings and both group crates), then this plan's Phases 2–9 interleaved with iPadOS Phases 2–9, each target's own column, tables, and budgets kept separate throughout.

**Spec:** `PLAN.md` §2, §8 (the backend definition, the Apple paragraph, Milestones 36 and 70), §11; `docs/conformance/new-backend-checklist.md`; `docs/conformance/platform-groups.md`; `docs/conformance/permissions.md`; `docs/deploy/update-rules.md` (iOS: no executable updates); `docs/ecosystem-analysis/mobile.md` (`M-OB-*`).

## Scope decision (2026-10-13)

Written by the 2026-10-12 whole-framework review, which found the iPhone-only halves unplanned: the iPadOS plan defers "Milestone 36's realization", and nothing planned the iPhone's capability table, its compatibility-mode answers on an iPad, its budgets, or its checklist column.

- **This document is the plan, not the work.**
- **In scope when work starts:** every Milestone 36 bullet; §8's definition; the §11 gates; the iOS halves owed by Milestones 39–48, 50, 51, 57–59, 61, and 67.
- **Out of scope:** what the iPadOS plan owns (windows the person arranges, the pencil's precision features, the keyboard as primary input, popover presentation, external displays); executable over-the-air updates (forbidden by the host).

## Hardware and accounts needed

| Need | Why | Minimum |
|---|---|---|
| A Mac with Apple silicon and current Xcode | building, signing, the simulator, `xcrun devicectl` | shared with Milestones 33 and 70 |
| An Apple Developer Program membership | device signing, TestFlight, push, the billing sandbox | shared |
| A current iPhone with a notch or Dynamic Island | safe areas, the scene lifecycle, VoiceOver, budgets (high-end) | one |
| An older, small iPhone | the low-end budget profile (`M-OB-4`), a small screen | one, or recorded as owed |
| An iPad | the iPhone application in the host's compatibility mode | shared with Milestone 70 |

## Global Constraints

As the iPadOS plan's Global Constraints, in full; and: nothing distinguishes iPhone from iPad by `cfg` — the idiom launcher is the only place, and everything downstream asks the backend it was handed.

## Verification gate

The Apple gate (`rustnative-tools/gate-apple.sh`) with the iOS lines:

```bash
cargo clippy -p rustnative-ios --target aarch64-apple-ios --all-targets -- -D warnings
tools/apple-sim.sh iphone cargo test -p rustnative-ios --target aarch64-apple-ios-sim
tools/apple-device.sh iphone cargo test -p rustnative-ios --target aarch64-apple-ios
```

## Crates

| Crate | Kind | Change |
|---|---|---|
| `rustnative-ios` | library (backend) | new |
| `rustnative-apple`, `rustnative-uikit` | library (groups) | from the iPadOS plan's Phase 1 |
| `rustnative-host` | library | the launcher selects `IosPlatform` on an iPhone, `IpadOsPlatform` on an iPad, at run time |
| `rustnative-style` | library | `IOS`, `IOS_UNITS` |
| `rustnative-macros` | proc-macro | `table_for("ios")` |
| `rustnative-build` | library | the iPhone's `Info.plist` keys, entitlements, privacy manifest, launch screen |
| `rustnative` (CLI) | binary | `build`/`run`/`package ios`, `doctor` rows |

## Phase 2 — The crate and the realized tree

- **`IosPlatform`**: `new()`, `with_app_id(id)`, `run` (adopts the application as Android's does — the host owns the main loop), `capabilities` from the device and followed, `style_capabilities` → `IOS`, `unit_mapping` → `IOS_UNITS`, `native_extension`.
- **One window root**: an iPhone shows one scene; `MultipleWindows` is answered no, and a request for a second window is refused as an outcome, never silently dropped.
- **The iPhone's controls**: navigation as `UINavigationController` stacks, tab bars as `UITabBarController`, sheets with detents for modal windows, the edit menu; everything else from the UIKit group's realization.
- **Style**: `rustnative_style::IOS` — as iPadOS's table for framework-owned boxes (layers) and native controls (configurations), answered separately. `IOS_UNITS`: one logical pixel is one point; `rem` follows Dynamic Type; positions round to the screen's pixel grid half away from zero.
- **Compatibility mode on an iPad**: when the idiom launcher finds an iPad but the project does not declare `ipados`, this backend runs as the host's iPhone application in compatibility mode and answers so — the iPhone's window size, no multitasking, no pencil hover — rather than stretching.

**Acceptance tests:** the shared guarantee suites over the iPhone simulator harness; `the_ios_capability_table_is_what_uikit_paints`; `a_second_window_is_refused_on_an_iphone`; compatibility-mode answers on an iPad simulator.

## Phase 3 — Lifecycle, safe areas, and keyboard avoidance

- The scene and application lifecycle onto the portable lifecycle and restoration contracts: background, foreground, termination, memory warnings (`LowMemory`), and process death restored from the state store and the scene's restoration activity; the lifecycle conformance suite (Milestone 45) on the device.
- Safe areas, the notch or Dynamic Island, the home indicator, and orientation as layout-model properties (`keys::SAFE_AREA`, size classes).
- Keyboard avoidance through the keyboard layout guide, as system insets (Milestone 61); the back gesture (`UINavigationController`'s edge swipe) as the portable `BACK` command, as Android's predictive back is.

**Acceptance tests:** `process_death_restores_the_navigation_stack_and_persisted_state`; `the_safe_area_is_the_scenes_insets`; `the_keyboard_moves_the_insets_not_the_root`; `the_edge_swipe_is_the_back_command_while_it_is_enabled`.

## Phase 4 — Input and accessibility

- Touch through the UIKit group's bridge, with `UIGestureRecognizer`s arbitrated against the framework's own (each conflict named in `docs/ios.md`); hardware keyboards through key commands; pencil pressure on iPhones that report it (most do not: `Pen` answered from the device).
- Accessibility through the UIKit group's `UIAccessibility` front end, verified with VoiceOver on the iPhone (a recorded pass by a person) and Dynamic Type at every size.

**Acceptance tests:** the accessibility assertion suite; layout under the largest accessibility text size and under pseudo-localization (Milestone 41).

## Phase 5 — Services and surfaces

- Services through the portable contracts: the share sheet (`UIActivityViewController`), the document picker (`DocumentService` over security-scoped URLs), notifications with actions, clipboard, URL schemes and universal links into the deep-link model, permission states (the mapping in `permissions.md`, `Limited` for a partial photo library and approximate location), Keychain secure storage, HTTP with pins, constrained background work (`BGTaskScheduler` behind `WorkScheduler`), locale facilities.
- Surfaces (Milestone 57): widgets from the restricted tree in the iPhone's families, live activities and the Dynamic Island, share and action extensions (each a target generated from `rustnative.toml`), remote push (APNs), store billing with server-side validation.
- Crash capture through the portable crash report; updates limited to model payloads (`PayloadKind::Model`), fingerprint-gated (Milestone 59).

## Phase 6 — Animation, lists, canvas, surfaces, host content

As the iPadOS plan's Phase 7, at the iPhone's refresh rates (ProMotion where the device has it).

## Phase 7 — The §11 gates

- **Inspection**, **embedding** (our tree in a `UIView`, a `UIView` adopted, library-only mode, `examples/adoption-uikit` shared with Milestone 70), **developer loop** (`rustnative dev ios` on the simulator and a paired iPhone), **conformance** (the iOS column of `new-backend-checklist.md`, separate from iPadOS's), **interaction depth** (Milestone 61), **extended style range** (Milestone 67).
- **Budgets** (Milestone 42): `budgets/ios.toml` — startup phases, resident memory under the host's limit, artifact size of the iPhone slice, frame time during a transition, touch latency — on the low-end and high-end iPhones, enforced by `rustnative bench --target ios` on the device.

## Phase 8 — Toolchain and packaging

`rustnative build|run|package ios`; `Info.plist` and entitlements from `rustnative.toml` (iPhone device family, or universal with `ipados`), the launch screen and icons among Milestone 59's generated launch assets, the privacy manifest from the build (Milestone 51), signing with a managed credential, IPA output, TestFlight, and store submission through Milestone 59.

## Phase 9 — Documentation and the full verification

`docs/ios.md`; `README.md`, `PLAN.md`, `BUILD_STATUS.md`. The simulator suite on two iPhone sizes; the device suite on each iPhone in the hardware table; the recorded VoiceOver pass; the budgets on both profiles; compatibility mode on an iPad.

**Done when** `PLAN.md` §8's definition holds for iOS on iPhone hardware, the iOS checklist column is complete, the budgets are met, and `BUILD_STATUS.md` records what ran on which device and iOS version.
