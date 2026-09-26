# The published comparison: methodology

`PLAN.md` Milestone 41: one reference application realized natively and
compared against the self-drawing and embedded-engine approaches on
assistive-technology support, input methods, automation, and system settings,
with the method published beside the results. The fidelity argument is only
worth making if it is measured.

## The application

`examples/reference-app` — the reference screen of the layout conformance suite
(`rustnative_conformance::reference`): a heading, wrapping prose, a tab strip,
labelled fields, and actions. The comparison builds the same screen in each
approach with that approach's own idioms, not a port of this one's code.

## What is measured, and how

| Dimension | Measurement | Pass condition |
|---|---|---|
| UIA tree completeness | Walk the window with `IUIAutomation::ElementFromHandle` and a raw-view tree walker; list every element's control type, name, and bounding rectangle | Every visible control is an element with the right control type and a non-empty name; no element without a visible counterpart |
| Labelled-by and relationships | Read `LabeledBy` on each field | Each field names its visible label |
| Automation | Invoke each button, set each field's value, select each tab through UIA patterns only | Each action takes effect in the application |
| Input methods | Compose Japanese and Chinese text through the system IME in each field | The composition window appears at the caret; committed text arrives intact |
| Keyboard | Tab through the screen; activate with Space/Enter | Every control reachable in reading order; focus rectangles visible |
| High contrast | Turn on a high-contrast theme | Every control takes the system colours with no application code |
| Text size | Set text to 150 % and 200 % | Fonts scale; nothing is clipped |
| Reduced motion | Turn off animations | Transitions snap |
| Screen reader | The checklist in `docs/conformance/windows-screen-reader-pass.md` | Every item passes |

## Results

| Dimension | Rust Native (Windows) | Self-drawing approach | Embedded-engine approach |
|---|---|---|---|
| UIA tree completeness | measured: `native_uia_reads_names_types_relationships_and_positions` | not yet measured | not yet measured |
| Labelled-by | measured: same test | not yet measured | not yet measured |
| Automation | measured: `native_uia_patterns_round_trip_through_the_component` | not yet measured | not yet measured |
| Input methods | measured: `native_ime_composition_on_a_focusable_container` (composition); a live CJK IME session is manual and **not yet recorded** | not yet measured | not yet measured |
| Keyboard | measured: `native_focus_traversal`, `keyboard_traversal_shows_focus_cues` | not yet measured | not yet measured |
| High contrast | measured: `high_contrast_uses_the_system_colours` | not yet measured | not yet measured |
| Text size | measured: `the_text_scale_scales_every_font_and_the_layout_follows`, `the_reference_screen_fits_its_text_at_every_scale` | not yet measured | not yet measured |
| Reduced motion | measured: `native_reduced_motion_arrives_immediately` | not yet measured | not yet measured |
| Screen reader | **not yet recorded** (a person's pass) | not yet measured | not yet measured |

The other two columns require building the reference screen in those
approaches' toolkits and running the same measurements; until that is done they
say "not yet measured" rather than repeating anyone's claims about them.
