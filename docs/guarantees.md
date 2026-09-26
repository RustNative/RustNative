# Guarantees

`PLAN.md` Milestone 41: every claim the framework makes is a guarantee with a
named test, or it is not made. A backend is not called complete until it passes
every row it can answer. The shipped backends are **Windows** and the
**headless** reference backend; the deferred backends (Milestones 33–38, Web
A–K) owe their own column.

Suites written once and run on every backend live in
`crates/rustnative-conformance`: `src/suites.rs` over the
`host::ConformanceHost` seam (`HeadlessHost` there; the Windows host in
`rustnative-windows`'s `native::guarantees_integration`).

| Guarantee | Headless | Windows |
|---|---|---|
| **Syntax equivalence** (2.9): every node kind and modifier, builder = `rsx!` = `.rsx` | `rustnative-conformance/tests/syntax_equivalence.rs` (backend-independent) | same |
| **Style equivalence** (2.14): every style property, typed = utility = declaration | `rustnative-conformance/tests/style_equivalence.rs` | same, plus the capability table read back: `native::style_integration::the_windows_capability_table_is_what_the_backend_applies` |
| **Unrealizable styles fail the build** | — | `rustnative-conformance/tests/style_ui_windows` (compile-fail) |
| **The invalidation contract** (`docs/invalidation.md`): exactly the components the change concerns re-render | `rustnative-core/tests/invalidation.rs` (`an_event_renders_only_the_component_that_handled_it`, `new_props_render_the_parent_and_the_child_whose_props_changed`, `an_environment_change_renders_only_its_readers`, …) | the same core, driven by the backend |
| **Render-cause tracing**: every render records its cause and component | `ComponentTree::last_render_log` / `RenderCause`, asserted throughout `rustnative-core/tests/invalidation.rs` | same |
| **Transient fast path** (2.10): typing re-renders only the owning component | `suites::typing_renders_only_the_owning_component` | same suite |
| … scrolling renders nothing | `suites::scrolling_renders_nothing` | same suite; `native_wheel_goes_to_interested_nodes_and_scrolls_containers_without_rendering` |
| … animation frames render nothing | — (no frame clock on the model) | `native_transition_moves_the_control_without_rerendering`, `native_frames_stop_when_the_animation_settles` |
| … a virtual list's window moves without rendering until it must | `rustnative-headless/tests/interaction.rs::scrolling_a_virtual_list_renders_only_when_its_range_moves` | `scrolling_inside_a_range_never_renders_and_crossing_one_renders_once` |
| **Batching** (`C09`): one message, one render; no render sees a partial set | `suites::one_message_is_one_render`; `rustnative-core/tests/invalidation.rs::the_batching_guarantee_holds_across_a_message_cascade` | same suite |
| **Scope-bound cancellation**: no task delivers after its owner unmounts | `suites::no_message_after_unmount`; the property over random mount/unmount/time sequences, `rustnative-conformance/tests/cancellation_property.rs` | `suites::no_message_after_unmount` |
| **Native-object lifetime**: realized objects return to baseline | `suites::mount_unmount_returns_to_baseline` | same suite, and the GDI/USER leak gate: `gdi_and_user_objects_return_to_baseline` (100 cycles of a styled subtree); `native_gdi_resource_lifecycle` |
| **Modal-operation conformance**: rendering, animation, and scheduled work continue inside the host's modal loops | — (no host loops) | `the_application_keeps_running_inside_a_menu_loop`, `the_application_keeps_running_inside_the_size_loop`; file dialogs run on their own thread, so the UI thread's loop never stops (`services::dialogs`). **Not verified:** the OLE drag-and-drop loop (it needs a physical button held down) |
| **Fidelity** (`docs/conformance/windows-fidelity.md`) | — | `controls_are_the_systems_own_classes`, `keyboard_traversal_shows_focus_cues`, `high_contrast_uses_the_system_colours`, `the_text_scale_scales_every_font_and_the_layout_follows`, `native_reduced_motion_arrives_immediately`, `native_right_to_left_locale_mirrors_the_window` |
| **Text through the host's stack**: complex scripts, bidi, clusters, emoji sequences, spaceless scripts, CJK | — | `text_goes_through_the_systems_own_stack` (round trip, caret positions, measurement agreeing with the system's) |
| **Layout conformance**: text scales 1.0/1.5/2.0 × pseudo-localized × mirrored; no clipping, no overlap, targets ≥ 24×24 and inside their container, all reachable by Tab | `rustnative-conformance/tests/layout_conformance.rs` | `the_reference_screen_fits_its_text_at_every_scale` (the system's own font metrics) |
| **Accessibility in CI**: roles, names, states, relationships, focus order, live regions | the portable tree: `rustnative-headless` queries (`Query::role`, …) in `tests/portable_surface.rs` | through UI Automation: `native_uia_reads_names_types_relationships_and_positions`, `native_uia_patterns_round_trip_through_the_component`, `native_uia_virtual_elements_are_navigable_invokable_and_disconnected`, `native_uia_live_region_change_is_announced`; focus order: `native_focus_traversal` |
| **A recorded pass with the host's own assistive technology** | n/a | **owed to a person**: `docs/conformance/windows-screen-reader-pass.md` is the checklist; no pass is claimed until one is recorded there |
| **The published comparison** | n/a | `docs/comparison/methodology.md`; the Windows column is measured by the tests above against `examples/reference-app`; the self-drawing and embedded-engine columns are **not yet measured** |

## Findings this milestone's suites made

- **Wrapping labels were measured at one line.** A column measured each
  child's height with no width, so a label that wraps got one line and was
  clipped. The column now measures each child at the width it will get
  (`layout::engine`); the layout conformance suite holds it.
- **Text measured in one font and drew in another** on Windows: the measurer
  used the window's default font. It now measures in the font the node is
  drawn in, at the person's text scale.
- **The text scale did not reach fonts** on Windows, and **high contrast did
  not reach colours**: both are now applied to every realized style, on the
  existing objects.
- **Keyboard traversal left focus cues hidden** on a mouse-activated window;
  Tab now shows them as the dialog manager does.
- **Scheduled work stalled in the host's modal loops** (its wake was handled
  only by the framework's own loop); the window procedure now handles it too.
- **A test harness's windows outlived their runtimes** within one thread; the
  harness now tears its windows down.
