# Windows idioms

`PLAN.md` Milestone 48 (`C23`). A control can be the same on every host and
still behave differently on each: button order, how things are dismissed,
where destructive actions go. `rustnative_components::Idioms` carries
these differences. Components read them from the environment (`IDIOMS`),
so a test can supply another host's idioms.

`Idioms::windows()` is the default. This page is the whole table for
Windows.

## Dialogs

| Behaviour | Windows | Where it is implemented |
|---|---|---|
| Button order | confirm, then cancel ("OK", "Cancel") | `Dialog`, from `Idioms::confirm_first` |
| Default button | the confirming button | `Dialog`: Enter activates it |
| Escape | cancels, wherever focus is in the dialog | `Dialog`, from `Idioms::escape_dismisses` |
| Destructive confirm | labelled with the verb ("Delete"), not "OK", and styled `danger` | `Dialog` with `destructive: true`, from `Idioms::mark_destructive` |
| Title | a sentence-case heading, no trailing punctuation | the application's text |

On macOS and GNOME, the cancelling button comes first (`Idioms::macos()`).
The library's tests assert both orders.

## Buttons and commands

| Behaviour | Windows |
|---|---|
| The primary action of a page | one `ButtonVariant::Primary` button, in the accent role |
| Keyboard access | every command also has its shortcut (`Shortcut`), shown in the menu |
| Standard commands | Ctrl+Z / Ctrl+Y undo and redo, Ctrl+S save, Ctrl+O open, Ctrl+N new, Ctrl+W close, Ctrl+F find (`command::standard`) |

## Lists, tables, and trees

| Behaviour | Windows |
|---|---|
| Selection | click selects; Ctrl+click toggles; Shift+click extends (`ListSelection`) |
| Keyboard | arrows move, Home and End jump, and a typed character jumps to the next match (`MenuNav`, `ListSelection`) |
| Trees | Right expands, then moves to the first child; Left collapses, then moves to the parent (`TreeNav`) |
| Sorting a table | clicking a header sorts ascending, and clicking again sorts descending (`DataTable`) |

## Transient messages

| Behaviour | Windows |
|---|---|
| Toasts | appear in the window, are announced politely, and dismiss themselves after their duration (`Toast`) |
| Errors in a field | shown under the field, announced politely, with the field keeping focus (`TextField`) |

## Navigation

| Window width (size class) | Navigation (`AdaptiveNavigation`) |
|---|---|
| Compact | bottom bar |
| Medium | rail (the Windows 11 navigation-view "compact" pane) |
| Expanded | sidebar (the navigation-view "left" pane) |

## Documents

| Behaviour | Windows |
|---|---|
| Windows | one window per document |
| Title | `*name - Application` while unsaved; `Untitled` before the first save |
| Closing with unsaved changes | a dialog: Save, Don't Save, Cancel, in that order |
| Recent documents | the File menu's list, newest first, ten entries |
