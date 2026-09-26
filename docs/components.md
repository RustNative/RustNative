# Components

`PLAN.md` Milestone 48. The `rustnative-components` crate is what
applications are built from, above the native primitives. Its styles are
written against semantic role tokens, so one token set restyles all of it
(`docs/tokens.md`). `examples/gallery` is an application built only from the
library and one token set.

## Using a component

Every component is a `Component` with a `…Props` type. That makes it usable
in both syntaxes:

```rust
// Builder
let save = context.child_with_props::<ActionButton, _>(
    "save",
    ActionButtonProps { text: "Save".into(), variant: ButtonVariant::Primary, command: Some(SAVE) },
    ActionButton::new,
);

// Markup; props left out take their defaults
let field = rsx! { in context, <TextField key="name" label="Name" value={name} /> };
```

Values a component changes are **bound**. The parent passes a `Store`, the
component writes to it, and the parent reads it with `context.select`.
Actions are **commands**. A button bound to a command invokes it, and the
component that declared the command handles it. The menu, the shortcut,
and the button all reach the same place.

## The components and their accessibility

Each role below is asserted by the library's tests
(`crates/rustnative-components/tests/library.rs`).

| Component | Realized as | Semantics |
|---|---|---|
| `ActionButton` | the host button | button; primary, secondary, and destructive variants |
| `TextField` | the host edit | text input named by its label; an error message is a polite alert |
| `SearchField` | the host edit | text input named "Search …" |
| `RadioGroup` | host radio buttons | a named group of radios |
| `Card` | a container | a group named by its title, which is a level-3 heading |
| `Badge` | a label | static text in a tone: neutral, accent, or danger |
| `Avatar` | a picture or initials | an image named by the person |
| `EmptyState` | labels and an optional action | a heading and its message |
| `ProgressIndicator` | the host progress bar | a progress bar, determinate or not |
| `Toolbar` | host buttons | a toolbar of command buttons |
| `Dialog` | a container | a dialog; button order follows the host's idiom |
| `Toast` | a label | a polite status that dismisses itself |
| `TabView` | a tab strip and a panel | a tab list and a tab panel |
| `ListView` | a virtual list | a list with position-in-set and selection |
| `SectionedView` | a virtual list of sections | headings, then a list per section |
| `DataTable` | a table of cells | a table; the sort is a view over the rows |
| `TreeView` | a list of tree items | a tree with level, expanded state, and position |
| `CommandPalette` | an edit and a list | a dialog that filters the command registry |
| `AdaptiveNavigation` | a bottom bar, rail, or sidebar | a tab list of destinations |
| `Chart` | a canvas | a table of every data point, read by arrow keys |

The native controls of Milestone 48 part 1 (`Node::checkbox`, `slider`,
`select`, `date_picker`, and so on) are primitives the components build on.
Each is the host's own control.

## The behaviour layer

`rustnative_components::behaviour` holds focus, keyboard, and selection for
composite controls. None of it depends on appearance (`C19`):

- `ListSelection`: single, multiple, and range selection.
- `TabsBehaviour`: tabs with automatic or manual activation.
- `MenuNav`: menus, including type-ahead.
- `TreeNav`: trees, with expand and collapse on the arrow keys.
- `GridNav`: two-dimensional grid navigation.
- `Combobox`: an input with a filtered popup.
- `DateEntry`: segmented date entry.

The library's components are built on these. A custom-drawn or terminal
control uses the same types, so its keyboard behaviour matches the
library's.

## Adaptive navigation and the command palette

`AdaptiveNavigation` picks its form from the window's size class
(`C22-3`):

- **Compact:** a bottom bar.
- **Medium:** a rail.
- **Expanded:** a sidebar.

`CommandPalette` filters the commands the application declared
(`palette_matches`) and invokes the one chosen (`C20-3`).

## Grids: typed layout data a container owns

`Node::grid` places its children in tracks (`C18-1`):

- `Track::Fixed` is a size in pixels.
- `Track::Auto` is the content's natural size.
- `Track::Fraction` is a share of what is left, like CSS `fr`.

A child says where it goes with `LayoutStyle::grid`. A child without a
placement takes the next free cell. Both syntaxes carry it:

```rust
Node::grid("form", GridStyle::new([Track::Fixed(120), Track::Fraction(1)]).gap(8), LayoutStyle::new(), [
    Node::label("name-label", "Name"),
    Node::text_input("name", ""),
    Node::label_with_layout("note", "Required", LayoutStyle::new().grid(GridPlacement::at(1, 0).span(1, 2))),
])
```

```rust
rsx! {
    <Grid key="form" tracks={tracks}>
        <Label key="name-label" text="Name" />
        <TextInput key="name" value="" />
        <Label key="note" text="Required" grid={GridPlacement::at(1, 0).span(1, 2)} />
    </Grid>
}
```

## Lists

`rustnative_data::list` has two parts (`C26`).

- **`Projection`** is a view over a source slice. It holds the indices,
  never copies of the rows. `filter`, `sort_by`, `sort_by_key`, and
  `reversed` rearrange the indices. `group_by` splits them into sections.
- **`diff_keys`** compares the keys a list showed with the keys it shows
  now. It reports removals, insertions, and the fewest moves: every row
  outside a longest increasing subsequence.

Keyed rows are already reconciled by identity. The diff is what a list uses
to animate a change:

- A moved row slides natively if it declares a `Position` transition.
- An inserted row can fade in.
- A removed row can fade out.

`SectionedView` is a compositional list. Each section has a header and its
own layout: a list, a grid of N columns, or a carousel. The sections are
the items of a Milestone 28 virtual list, so only the sections on screen
exist.

## Matched geometry

`Node::with_shared_id("photo-7")` gives a node a shared identity (`C25`).
When a render removes one node with that identity and adds another, the new
node moves and resizes from where the old one was. A thumbnail grows into
its detail view.

- The motion is the arriving node's `Position` transition, or a 250 ms ease
  if it declares none.
- Under reduced motion, the change is instant.
- `rustnative_core::matched_geometry` does the pairing. It is portable.
  Windows has no shared-element transition of its own, so its renderer
  animates the pair with its ordinary geometry transitions.

## Documents

`rustnative_data::document::DocumentController` owns one document (`C27`).
It provides:

- open, save, save as, and revert;
- autosave after a pause in editing;
- dirty state, measured against the saved content, so undoing back to it
  is clean again;
- per-document undo, bound to the standard `UNDO`, `REDO`, and `SAVE`
  commands through `handle`;
- external-change detection (`external_change`).

`RecentDocuments` keeps the ten newest paths. It is serializable, so it is
persisted with the rest of the application's state.

Windows conventions:

- **One window per document.**
- **Title.** `*name - App` while unsaved.
- **Save on an untitled document** asks for a path through the host's save
  dialog (`save_dialog`).
- **Saving writes beside the target and renames over it.** A failed save
  never leaves half a file.

## Charts

`Chart` draws line, area, bar, scatter, and pie charts on the draw-list
path. Its accessible form is a table with a cell for every data point,
named "series, category: value". The arrow keys move through the points,
and each one is announced. It is not an image with a label.

## Host content

See `docs/guides/host-content-controls.md`. Web content, media, and camera
preview are capability-guarded nodes. When a capability is absent, the
application's fallback is shown instead.

## Text profiles

`TextProfile` declares which scripts a target renders. An embedded target
with a bitmap font states `TextProfile::new([Script::Latin])`. An
application checks its catalog strings against it (`check`), so an
unsupported string is found in a test, before a person sees boxes. Windows
renders every script (`TextProfile::full()`).

## Further reading

- `docs/idioms/windows.md`: the per-host idioms.
- `docs/guides/hybrid-rendering.md`: custom-drawn regions inside a native
  tree.
- `docs/tokens.md`: the token pipeline.
