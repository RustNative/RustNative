# Guarantees

`PLAN.md` Milestone 41: every claim the framework makes is a guarantee with a
named test, or it is not made. A backend is not called complete until it passes
every row it can answer. The shipped backends are **Windows** and the
**headless** reference backend, and the **Web** backend (Web milestones A–K),
whose guarantees are in their own table below; the deferred native backends
(Milestones 33–38) owe their own column.

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

## The Web backend

Browser suites run in headless Edge (or Chrome) through
`rustnative-web-testing`; a machine without one skips them and says so.
Firefox and Safari are not run.

| Guarantee | Test |
|---|---|
| **Realizer agreement**: the JavaScript realizer writes what the Rust one writes, for every node kind | `rustnative-web/tests/runtime.rs::the_javascript_realizer_writes_what_the_rust_one_writes`; every node kind as its semantic element under a strict policy: `rustnative-web/tests/browser_dom.rs` |
| **Client subset**: client logic compiled to JavaScript agrees with the Rust, failures included | `rustnative-web/tests/client_subset.rs` (`a_todo_list_agrees_in_both_languages`, `numbers_text_and_control_flow_agree_including_their_failures`, `effects_are_requested_and_answered_alike`, `pointers_wheels_compositions_and_capabilities_agree`) |
| **Hydration without a difference**: an island attaches to the server's markup, and after each event its DOM is the server's rendering of the state the runtime reports | `rustnative-server/tests/pages_browser.rs::pages_in_a_browser` |
| **Selective attachment**: a page with no island ships no JavaScript; a route loads only its own modules | `rustnative-web/tests/pages.rs::a_page_with_no_island_ships_no_javascript`; `budgets/web.toml` (`unrelated_routes_script_growth_kb = 0`) |
| **Deterministic render**: a render waits for its data the same way every time; the request's tasks end with the response | `rustnative-web/tests/pages.rs::a_render_waits_for_its_data_the_same_way_every_time`, `the_requests_tasks_end_with_the_response` |
| **Streaming**: the fallback reaches the socket before the data exists, and is filled in place | `rustnative-server/tests/pages.rs::a_streamed_pages_fallback_reaches_the_socket_before_its_data_exists` |
| **Progressive enhancement**: a form posts without JavaScript | `rustnative-web/tests/pages.rs::a_form_posts_without_javascript`; in a browser with scripts off, `pages_in_a_browser` |
| **JavaScript = WebAssembly**: one client component as generated JavaScript, as WebAssembly, and as WebAssembly in a worker leaves an identical DOM after the same events | `rustnative-server/tests/wasm_browser.rs::webassembly_subtrees_in_a_browser` |
| **Mode equivalence** (`W-MF-5`): one application, the same state — client-side, server-rendered, as a function, on the edge — has an identical DOM before and after the same interactions | `rustnative/tests/serverless.rs::the_ui_is_the_same_in_every_mode` |
| **Stateless serverless**: a function's invocation keeps nothing, and its spawned work does not outlive it; an edge request is a fresh instance; one session is good in every shape | `rustnative-server/tests/serverless.rs::work_an_invocation_spawned_does_not_outlive_it`; `rustnative/tests/serverless.rs::one_application_answers_in_every_shape` |
| **Host limits**: a late invocation is refused; a route over its fuel budget and a module over its memory ceiling are stopped | `rustnative-server/tests/serverless.rs::an_invocation_past_its_deadline_is_refused`; `one_application_answers_in_every_shape` |
| **Edge actors** (`C46`): one actor, serialized and persisted, locally and on the edge | `examples/edge-actors/tests/local.rs`; `rustnative/tests/serverless.rs::an_actor_session_serializes_and_persists_on_the_edge` |
| **Offline**: an installed application works offline and replays its queued calls on reconnect | `rustnative-server/tests/offline_browser.rs::an_offline_application_in_a_browser` |
| **Loading path**: responsive images, subset fonts, no layout shift, budgets for startup, LCP, CLS, INP | `rustnative-server/tests/loading_browser.rs::the_loading_path_in_a_browser`; `budgets/web.toml` |
| **Static export**: an exported site works from a static host; source maps lead back to the Rust | `rustnative-web/tests/export.rs` |
| **Custom elements** (`C43-1`): a client component as a custom element on a page the framework did not render | `rustnative-web/tests/element.rs::a_custom_element_on_a_plain_page` |
| **Input and capabilities**: pointer, wheel, composition, clipboard, navigation, and permission-gated capabilities | `rustnative-server/tests/input_browser.rs::input_navigation_and_services_in_a_browser`; the permissions policy opens only declared capabilities: `rustnative-server/tests/pages.rs::the_permissions_policy_opens_only_declared_capabilities` |
| **The development loop**: a new web application reloads on save with its islands' state, and shows a broken build over the page | `rustnative/tests/web_dev.rs::a_new_web_application_develops_in_the_browser` |

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
