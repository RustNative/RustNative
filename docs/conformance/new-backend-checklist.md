# New-backend conformance checklist (Milestone 39)

Every item a backend must satisfy — or answer as an honest capability —
before it is called complete (`PLAN.md` §8). Each names the test that proves
it for the backends that exist today: **Windows** (`rustnative-windows`),
**Linux** (`rustnative-linux`, whose tests run on Wayland, Xwayland, and Xvfb),
**headless** (`rustnative-headless`), and the **Web** (`rustnative-web`, whose
column says `—` where no test holds the item yet: those items are not
satisfied on the Web). A future backend adds its column and
its tests; an item with no test is not satisfied.

Test paths are relative to the crate named; `native::…` tests live in
`crates/rustnative-windows/src/native/`, and `gtk::…` tests in
`crates/rustnative-linux/src/gtk/`.

| # | Obligation | Windows | Linux | Headless | Web |
|---|---|---|---|---|---|
| 1 | Desktop affordances expressed as capabilities a host may refuse (window management, menus, shortcut maps, hover, cursors, drag-and-drop, multi-window) | `platform::tests::capabilities_only_advertise_realized_backend_features` | `capability_tests::capabilities_only_advertise_realized_backend_features`; per display server: `capability_tests::window_placement_is_answered_by_the_display_server` | `platform::tests::capabilities_only_advertise_what_the_model_realizes` | `fx.capabilities` / `rn.caps()` answer what this browser realizes: `rustnative-server/tests/input_browser.rs::input_navigation_and_services_in_a_browser`; the page's `Permissions-Policy` opens only declared capabilities: `rustnative-server/tests/pages.rs::the_permissions_policy_opens_only_declared_capabilities` |
| 2 | No Windows assumption left in the portable API | reviewed in `portable-api-audit.md`; each resolution has its own row below | the same audit; no Linux assumption was needed in the portable API (Milestone 34 added only display-server capabilities) | — | — |
| 3 | Right-to-left as a layout-model property: start/end, mirroring applied once | `integration::native_right_to_left_locale_mirrors_the_window` (host mirroring, runtime switch, same objects) | `gtk::integration::a_right_to_left_locale_mirrors_the_window` (engine mirroring, GTK drawing direction, runtime switch, same widgets) | `tests/portable_surface.rs::a_right_to_left_locale_mirrors_rows_and_start_insets` (backend mirroring); core: `LayoutResult::physical_rects` doc test | the document carries `dir`, and the browser mirrors: `rustnative-web/tests/pages.rs::a_document_carries_its_language_direction_and_alternates`; logical CSS properties in `docs/web/layout-mapping.md` |
| 4 | Safe areas, cutouts, hinges, split-screen as environment values | answered: safe area zero, posture flat, `WINDOW_MODE` from the snapped width (`host_traits::window_mode`) | answered: no safe area or posture on a desktop window (`gtk::host_traits` module docs) | `tests/portable_surface.rs::content_stays_clear_of_the_safe_area` | — |
| 5 | Permission states beyond a boolean, with a request flow and a documented mapping | `services::permissions::tests::every_permission_has_an_answer_on_this_machine`; mapping in `permissions.md` | `services::permissions::tests::*` (ungated unsandboxed; the Camera portal asked and remembered in a sandbox); mapping in `docs/linux.md` | `FixedPermissions` (core `permission::tests`) | the Permissions API's states through `fx.permission` / `fx.request_permission`: `input_navigation_and_services_in_a_browser`; mapping in `docs/web/capabilities.md` |
| 6 | Gesture arbitration with the conflict cases enumerated | scroll-vs-pan applied in `native::input::pointer` (pans inside a scroll container yield under `DeferToHost`) | scroll-vs-pan applied in `gtk::input::pointer` (a pan inside a scroll container yields under `DeferToHost`) | core `input::arbitration::tests::the_table_is_what_the_documentation_says` | — |
| 7 | Thread affinity enforced by type where possible, asserted elsewhere | `rustnative-core/tests/affinity.rs` (tree-holding types are `!Send`); `UiThread` required by `native_handle` | same (the core); every GTK call is on the GTK thread (`gtk::backend`, `services::on_gtk`) | same | same (the core); the runtime is single-threaded, and a WebAssembly subtree in a worker owns its own tree |
| 8 | One ownership module per backend, conventions asserted | `native::ownership` (module docs) + `integration::native_gdi_resource_lifecycle`, `registry` tests | `gtk::rendering` (`NativeRegistry`: one owner per widget, recycling without double counting) + `gtk::inspect_integration::the_inspector_sees_each_node_s_widget_and_the_census_balances` | n/a (no host objects) | — |
| 9 | A documented escape-hatch contract | `rustnative_core::handle` docs; `integration::native_handle_validates_and_goes_stale` | `gtk::surface_integration::a_rendered_surface_is_a_live_native_surface_that_follows_its_node` (`native_surface`); per-property mappers (row 14) | n/a | — |
| 10 | Panic and teardown policy restoring host state, verified by a deliberate panic | `integration::native_panic_restores_capture_and_cursor_clip` | `gtk::integration::a_panicking_component_is_caught_and_ends_the_run_with_its_message`; grabs released in `gtk::teardown` | n/a (no host state) | — |
| 11 | Typed environment fed from host traits; invalidation limited to readers | `host_traits::tests::*`; `WM_SETTINGCHANGE` handler | `gtk::style_integration::the_desktops_scheme_and_text_scale_reach_the_environment_and_follow_changes` (the Settings portal, then `GtkSettings`) | core `tests/invalidation.rs::an_environment_change_renders_only_its_readers` | — |
| 12 | Command model bound by menus, buttons, and shortcuts, routed by focus | `integration::native_commands_drive_shortcuts_and_menu_state` | `gtk::menu_integration::a_command_bound_item_follows_its_command`; `gtk::input_integration::a_shortcut_reaches_its_command_before_the_focused_field` | `tests/portable_surface.rs::a_shortcut_reaches_its_command_from_a_focused_field`; core `tests/commands.rs` | — |
| 13 | Adaptive layout: size classes per axis and container-relative decisions | size classes on resize (core `Application::dispatch_to_window`); container sizes reported from `Runtime::render`/`relayout` | container sizes reported from `WindowRegistry::render`/`relayout`; the shared suites over the GTK harness: `gtk::guarantees::the_shared_guarantee_suites_hold_on_linux` | `tests/portable_surface.rs::a_container_decides_by_its_own_size_class` | — |
| 14 | Per-property native mappers, extendable per kind or per instance | `integration::native_text_mapper_replaces_one_control` | `gtk::integration::mappers_extend_and_replace_what_the_backend_applies` | n/a | — |
| 15 | Surface vocabulary answered honestly | `platform::tests` asserts no `Capability::Surface(_)` until Milestone 57 | `capability_tests` (the tray only where a tray host runs; no jump list at run time; no global menu bar) | same | — |
| 16 | Platform-group crate decision recorded before a group's second member | `platform-groups.md` | `platform-groups.md` (desktop shell decided at Linux: no shared crate) | — | — |
| 17 | Capability grants: services obtainable only through a scoped grant (shape) | core `grant::tests::grants_are_scoped`; enforcement is Milestone 51 | same | same | same |
| 18 | Typestate for handles, grants, validated values | `typestate.md`; core `handle::tests` | same | same | same |
| 19 | Cursors per node | `integration::native_declared_cursor_is_shown` | `gtk::input_integration::pointer_samples_reach_the_interested_node_in_its_coordinates` (the declared cursor on the widget) | n/a (no pointer) | — |
| 20 | A style capability table: every property realized, approximated (how), or unavailable (why); unavailable is a build error for the target (Milestone 58) | `rustnative_style::WINDOWS`; `native::style_integration::the_windows_capability_table_is_what_the_backend_applies` (fonts, colours, border, region read back); compile-fail `rustnative-conformance/tests/style_ui_windows` | `rustnative_style::LINUX` (every property realized; a family approximated through fontconfig); `gtk::style_integration::the_linux_capability_table_is_what_gtk_paints`; nothing is unavailable, so no compile-fail cases | `rustnative_style::HEADLESS`; `tests/portable_surface.rs::the_headless_style_table_realizes_everything` | `rustnative_style::WEB` (every property a CSS property, from the generated style sheet); under a strict policy in a browser: `rustnative-web/tests/browser_dom.rs` |
| 21 | A unit mapping: host unit, `rem` following the text setting, one rounding rule | `rustnative_style::WINDOWS_UNITS`; core `style::resolve::tests::a_rem_follows_the_text_scale_and_rounds_half_away_from_zero` | `rustnative_style::LINUX_UNITS`; `gtk::rendering::styling::tests::fonts_scale_once_and_round_half_away_from_zero` | `rustnative_style::HEADLESS_UNITS` | `rustnative_style::WEB_UNITS` (CSS pixels; `rem` follows the browser's text size) |
| 22 | Runtime token resolution: theme, scheme, and text-scale changes restyle existing native objects | `native::style_integration` (same HWNDs across a scheme and a token switch) | `gtk::style_integration::the_desktops_scheme_and_text_scale_reach_the_environment_and_follow_changes` (same widgets across a scheme and text-scale change) | core `tests/style_equivalence.rs::environment_variants_follow_the_environment_without_re_rendering` | — |
| 23 | The extended range answered: every property of Milestone 67 realized, approximated, or unavailable, separately for a native control and a framework-owned box, read back from the native objects; the headless backend realizes all of it | — | — | — | — |
| 24 | Guards decided against this backend's table at resolution: a capability or target variant applies exactly where the table says, and an unguarded property this backend cannot realize fails the build for every application that declares it as a target (Milestone 67) | — | — | — | — |
| 25 | Decorate the box, never the control (`PLAN.md` 2.2): any extended-range answer realized on a box comes from the host's drawing or composition services, no native control is captured or owner-drawn to satisfy a style, and the limits of mixing the two are answered as capability | — | — | n/a (no host objects) | — |

Owed by deferred backends (Milestones 33, 35–38, and 70): their own column in
this table, and in particular real safe areas and hinges (35, 36, 70), host
gesture recognizers competing with ours (35, 36, 70), and terminal restoration
(38). iPadOS has its own column, not a share of iOS's: the two backends share
a toolkit, but rows 1, 4, 6, 11, 12, 13, 19, 20, and 21 answer differently on
an iPad — multiple windows the person arranges and resizes (rows 1, 4, 13), the
host's multitasking edges as gesture conflicts (6), size classes and window
modes that change while running (11, 13), hardware-keyboard commands and the
main menu (12), pointer styles as an approximated cursor mapping (19), and the
style table and unit mapping of a separate target (20, 21). Its rows are
planned in `docs/superpowers/plans/2026-10-04-ipados-milestone-70.md`. Owed
on the Web: the rows marked `—`, among them the browser's safe-area insets,
gesture arbitration against the browser's own (`touch-action`), and runtime
token switches without a reload. Owed by every backend once Milestone 67 lands:
rows 23–25, which no backend satisfies yet; on Windows, row 25 is where
Milestone 69's composition prototype is recorded, and row 21's unit mapping
gains its per-monitor scaling there too.
