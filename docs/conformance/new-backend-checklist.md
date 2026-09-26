# New-backend conformance checklist (Milestone 39)

Every item a backend must satisfy — or answer as an honest capability —
before it is called complete (`PLAN.md` §8). Each names the test that proves
it for the backends that exist today: **Windows** (`rustnative-windows`) and
**headless** (`rustnative-headless`). A future backend adds its column and
its tests; an item with no test is not satisfied.

Test paths are relative to the crate named; `native::…` tests live in
`crates/rustnative-windows/src/native/`.

| # | Obligation | Windows | Headless |
|---|---|---|---|
| 1 | Desktop affordances expressed as capabilities a host may refuse (window management, menus, shortcut maps, hover, cursors, drag-and-drop, multi-window) | `platform::tests::capabilities_only_advertise_realized_backend_features` | `platform::tests::capabilities_only_advertise_what_the_model_realizes` |
| 2 | No Windows assumption left in the portable API | reviewed in `portable-api-audit.md`; each resolution has its own row below | — |
| 3 | Right-to-left as a layout-model property: start/end, mirroring applied once | `integration::native_right_to_left_locale_mirrors_the_window` (host mirroring, runtime switch, same objects) | `tests/portable_surface.rs::a_right_to_left_locale_mirrors_rows_and_start_insets` (backend mirroring); core: `LayoutResult::physical_rects` doc test |
| 4 | Safe areas, cutouts, hinges, split-screen as environment values | answered: safe area zero, posture flat, `WINDOW_MODE` from the snapped width (`host_traits::window_mode`) | `tests/portable_surface.rs::content_stays_clear_of_the_safe_area` |
| 5 | Permission states beyond a boolean, with a request flow and a documented mapping | `services::permissions::tests::every_permission_has_an_answer_on_this_machine`; mapping in `permissions.md` | `FixedPermissions` (core `permission::tests`) |
| 6 | Gesture arbitration with the conflict cases enumerated | scroll-vs-pan applied in `native::input::pointer` (pans inside a scroll container yield under `DeferToHost`) | core `input::arbitration::tests::the_table_is_what_the_documentation_says` |
| 7 | Thread affinity enforced by type where possible, asserted elsewhere | `rustnative-core/tests/affinity.rs` (tree-holding types are `!Send`); `UiThread` required by `native_handle` | same |
| 8 | One ownership module per backend, conventions asserted | `native::ownership` (module docs) + `integration::native_gdi_resource_lifecycle`, `registry` tests | n/a (no host objects) |
| 9 | A documented escape-hatch contract | `rustnative_core::handle` docs; `integration::native_handle_validates_and_goes_stale` | n/a |
| 10 | Panic and teardown policy restoring host state, verified by a deliberate panic | `integration::native_panic_restores_capture_and_cursor_clip` | n/a (no host state) |
| 11 | Typed environment fed from host traits; invalidation limited to readers | `host_traits::tests::*`; `WM_SETTINGCHANGE` handler | core `tests/invalidation.rs::an_environment_change_renders_only_its_readers` |
| 12 | Command model bound by menus, buttons, and shortcuts, routed by focus | `integration::native_commands_drive_shortcuts_and_menu_state` | `tests/portable_surface.rs::a_shortcut_reaches_its_command_from_a_focused_field`; core `tests/commands.rs` |
| 13 | Adaptive layout: size classes per axis and container-relative decisions | size classes on resize (core `Application::dispatch_to_window`); container sizes reported from `Runtime::render`/`relayout` | `tests/portable_surface.rs::a_container_decides_by_its_own_size_class` |
| 14 | Per-property native mappers, extendable per kind or per instance | `integration::native_text_mapper_replaces_one_control` | n/a |
| 15 | Surface vocabulary answered honestly | `platform::tests` asserts no `Capability::Surface(_)` until Milestone 57 | same |
| 16 | Platform-group crate decision recorded before a group's second member | `platform-groups.md` | — |
| 17 | Capability grants: services obtainable only through a scoped grant (shape) | core `grant::tests::grants_are_scoped`; enforcement is Milestone 51 | same |
| 18 | Typestate for handles, grants, validated values | `typestate.md`; core `handle::tests` | same |
| 19 | Cursors per node | `integration::native_declared_cursor_is_shown` | n/a (no pointer) |
| 20 | A style capability table: every property realized, approximated (how), or unavailable (why); unavailable is a build error for the target (Milestone 58) | `rustnative_style::WINDOWS`; `native::style_integration::the_windows_capability_table_is_what_the_backend_applies` (fonts, colours, border, region read back); compile-fail `rustnative-conformance/tests/style_ui_windows` | `rustnative_style::HEADLESS`; `tests/portable_surface.rs::the_headless_style_table_realizes_everything` |
| 21 | A unit mapping: host unit, `rem` following the text setting, one rounding rule | `rustnative_style::WINDOWS_UNITS`; core `style::resolve::tests::a_rem_follows_the_text_scale_and_rounds_half_away_from_zero` | `rustnative_style::HEADLESS_UNITS` |
| 22 | Runtime token resolution: theme, scheme, and text-scale changes restyle existing native objects | `native::style_integration` (same HWNDs across a scheme and a token switch) | core `tests/style_equivalence.rs::environment_variants_follow_the_environment_without_re_rendering` |

Owed by deferred backends (Milestones 33–38, Web A–K): their own column in
this table, and in particular real safe areas and hinges (35, 36), host
gesture recognizers competing with ours (35, 36, Web D), and terminal
restoration (38).
