# Milestone 37 Implementation Plan (the embedded backends)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan phase-by-phase. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Status: planned, not started.** The host-side simulator and the embedded Linux profile can be built and verified on this project's machines (WSL, QEMU); the RTOS and bare-metal profiles need the reference boards listed below, and results on them are recorded only once run there (`PLAN.md` 2.13).

**Goal:** Build the embedded backends — `PLAN.md` "Milestone 37 — Embedded backends" — as three named profiles, each to §8's definition in the form its device allows, with the core work they depend on done first.

**Architecture:**

- **Core prerequisites** (Phase 0, in `rustnative-core` and `rustnative-types`): the `no_std` + `alloc` profile of the identity, node, reconcile, layout, and style-resolution layers; a single-threaded executor with no thread pool (`LocalExecutor` without `tokio`, the `threads` feature off); time from the host's clock only (`Clock` everywhere, no `std::time::Instant` in those layers).
- **`rustnative-drawlist`** (the draw-list group, shared with Milestone 38 and the inspection overlay): rasterizing a `DrawList`, damage tracking and dirty-region coalescing, the focus ring and hit testing for hosts with no native controls.
- **`rustnative-embedded`**: the display-driver contract (asynchronous region transfer, so a DMA- or PIO-backed driver overlaps transfer with the next frame), the input-driver contract (buttons, rotary encoders, touch panels, a serial console) mapped onto the portable key and pointer model, the storage-driver contract, and three profiles:
  - **embedded Linux, direct to display** (`/dev/fb0` or DRM/KMS, evdev input): boot-to-UI with no window system (`C83-1`);
  - **embedded Linux with a graphical session**: the Linux backend (Milestone 34) is reused as is, and this profile is only its packaging;
  - **RTOS and bare metal** (`no_std`): an existing HAL's typed peripherals taken by the framework's drivers (`C74-1`), an existing async executor (Embassy or the vendor's) implementing the core's executor contract, driven from the vendor project's `main` (guest integration, Milestones 40 and 52).
- **A host-side simulator**: the same draw list rendered in a window on a development machine with a simulated display size, colour depth, and input (Milestone 45), so most logic is tested without hardware.

**Spec:** `PLAN.md` §2.3 (the embedded paragraph), §8 (Milestone 37), §11; `docs/policy/embedded.md`; `docs/ecosystem-analysis/embedded.md`; `docs/conformance/platform-groups.md`.

## Scope decision (2026-10-13)

Written by the 2026-10-12 review, which found no implementation plan, one CLI value for three profiles, no budgets per device class, the core prerequisites untracked, and no reference hardware named.

- **In scope:** every Milestone 37 bullet, by profile; the core prerequisites; the draw-list group crate if Milestone 38 has not created it.
- **Out of scope:** writing HALs, bootloaders, or RTOS kernels (Milestone 37's non-duplication policy: consumed from the ecosystem).

## Hardware

| Need | Why | Profile |
|---|---|---|
| A Raspberry Pi 4 or 5 with a DSI or HDMI display and a touch panel | direct-to-display Linux, evdev input, boot-to-UI | embedded Linux |
| An RP2040 or RP2350 board with an SPI display (PIO-driven) and buttons | `no_std` on bare metal, DMA/PIO region transfer, buttons and an encoder | bare metal |
| An STM32 or ESP32 board with a display, under Embassy | an existing async executor implementing the core's contract | RTOS |
| QEMU (`aarch64` Linux) | CI for the embedded Linux profile without the board | embedded Linux |

## Global Constraints

- The `no_std` layers allocate only through `alloc` and only in bounded amounts, with a bounded-allocation mode in which allocation failure is a handled outcome (Milestone 51).
- No periodic wake without a pending deadline; documented wake sources; a watchdog integration with a safe-state panic path.
- Capabilities state what the device has: no windows, no accessibility services where there are none (2.3), no clipboard, no network unless the profile has one.

## Phase 0 — The core prerequisites (no hardware)

1. `rustnative-core`'s identity, node, reconcile, layout, and style-resolution layers build `no_std` + `alloc` (a `std` feature on by default); CI builds them for `thumbv7em-none-eabihf` beside `rustnative-types` (which already does).
2. A threadless executor: `LocalExecutor` usable with `threads` off, and the executor contract implementable by an external async executor (a test implements it over a minimal hand-written one).
3. Time from `Clock` only in those layers.
4. CLI: `Platform::Embedded` split into `embedded-linux` and `embedded` (bare metal and RTOS), each failing with "Milestone 37" until built.

**Acceptance tests:** the core's `no_std` build in CI; the guarantee suites running over the threadless executor.

## Phase 1 — The draw-list group and the simulator

Damage tracking and region coalescing shared with Milestone 38; a rasterizer to an RGB565/RGB888 framebuffer with the colour-depth rule documented; the focus ring; the simulator window (on the host's own backend) with display size, depth, and input simulated.

## Phase 2 — Embedded Linux, direct to display

DRM/KMS (or `/dev/fb0`) output with page flipping, evdev input (touch, keys, an encoder), a boot-to-UI service unit, the state store and secure storage on the device's filesystem, the Yocto layer and Buildroot package that integrate an application the way anything else on the device is integrated (Milestone 50); firmware updates through an existing A/B scheme (RAUC or SWUpdate) behind the update rules.

**Acceptance tests:** under QEMU in CI: boot to first frame, input, damage-only redraw; on the Pi: the same, with the boot-to-first-frame budget.

## Phase 3 — RTOS and bare metal

The display, input, and storage driver contracts over `embedded-hal` 1.0 traits and the boards' HALs; Embassy as the executor on the RTOS board; the RP2040/RP2350 PIO display driver's asynchronous region transfer overlapping the next frame; the watchdog and safe-state panic path; the idle-current measurement on the reference board.

**Acceptance tests:** on each board: first frame, input through every input kind, a measured idle current, a provoked panic reaching the safe state.

## Phase 4 — The §11 gates, reduced

- **Conformance**: an embedded column per profile in `new-backend-checklist.md`, answering or stating inapplicable each row.
- **Budgets** (Milestone 42): `budgets/embedded-linux.toml` and `budgets/embedded.toml` — RAM, flash (or image size), boot to first frame, frame time, idle current — per device class, enforced in CI on QEMU and the simulator, and on the boards when run.
- **Inspection** over the serial console or a socket (Milestone 44).
- **A constrained text profile** declaring the scripts each profile supports (Milestone 48).

## Phase 5 — Toolchain, documentation

`rustnative build|package embedded-linux|embedded --board <board>` driving each target's own toolchain (cargo with the right target and linker scripts, `probe-rs` for flashing, the Yocto/Buildroot recipes); `docs/embedded.md`; the update rule's embedded row made concrete; `README.md`, `PLAN.md`, `BUILD_STATUS.md`.

**Done when** each profile meets Milestone 37's bullets on its reference hardware (or QEMU for embedded Linux), the budgets are met per device class, and `BUILD_STATUS.md` records what ran on which board.
