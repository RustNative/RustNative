# Windows fidelity

`PLAN.md` Milestone 41: fidelity is measured against the host's own first-party
applications — never against another backend. Each row is held by a test in
`rustnative-windows` (`native::guarantees_integration` unless noted).

| Convention | What the framework does | Test |
|---|---|---|
| Controls are the system's own | Labels are `Static`, buttons `Button`, fields `Edit`, tab strips `SysTabControl32`, a canvas or surface a plain child window; nothing is owner-drawn | `controls_are_the_systems_own_classes` |
| Focus visuals | Tab traversal sends `WM_CHANGEUISTATE (UIS_CLEAR, UISF_HIDEFOCUS \| UISF_HIDEACCEL)` so focus rectangles and accelerator underlines appear, as the dialog manager does | `keyboard_traversal_shows_focus_cues` |
| High contrast | Every control takes the system's colours for its role (`COLOR_WINDOWTEXT`/`COLOR_WINDOW`, `COLOR_BTNTEXT`/`COLOR_BTNFACE`) over the theme and the application's own styles, on the existing windows | `high_contrast_uses_the_system_colours` |
| Text size | Every font — themed or declared — is scaled by the person's text scale, and layout measures in the scaled font | `the_text_scale_scales_every_font_and_the_layout_follows` |
| Reduced motion | Animations snap to their end state | `native::animation_integration::native_reduced_motion_arrives_immediately` |
| Colour scheme | `dark:` styles follow the system setting live (`WM_SETTINGCHANGE`) | `native::style_integration::the_windows_capability_table_is_what_the_backend_applies` |
| Right-to-left | The window mirrors natively (`WS_EX_LAYOUTRTL`), switchable at run time | `native::integration::native_right_to_left_locale_mirrors_the_window` |
| Scrolling | Wheel scrolling moves the native viewport (three lines per detent) without re-rendering | `native::input_integration::native_wheel_goes_to_interested_nodes_and_scrolls_containers_without_rendering` |
| Text rendering and input | The system's own stack (GDI/Uniscribe in the controls; IME composition) | `text_goes_through_the_systems_own_stack`; `native_ime_composition_on_a_focusable_container` |
| Context menus and drag | Native menus (`TrackPopupMenu`, menu bars); OLE drag-and-drop | `native_menu_dispatch`; `native_drop_target_negotiates_and_delivers_files` |
| Modal loops | Rendering, animation, and tasks continue during menu tracking and window sizing | `the_application_keeps_running_inside_a_menu_loop`, `the_application_keeps_running_inside_the_size_loop` |

**Not yet matched, recorded rather than claimed:** layout is not scaled by DPI
(one logical pixel is one device pixel; the unit mapping says so), and the
system's accessibility settings beyond contrast, text size, and motion
(cursor size, caret width) are not read.
