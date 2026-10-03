# Linux accessibility

`PLAN.md` Milestone 34. The portable accessibility model
(`rustnative_core::AccessibilityTree`) is applied to GTK's own accessibility
API (`GtkAccessible`), and GTK carries it to the AT-SPI2 bus, where Orca and
every other Linux assistive technology read it. The backend never speaks
AT-SPI itself: what a screen reader sees is what GTK publishes for the
widgets the backend configured.

## Mapping

| Portable | GTK (`GtkAccessible`) | AT-SPI2 |
|---|---|---|
| role | `accessible-role`, fixed at construction (a role change replaces the widget) | `GetRole` |
| name, description | the `LABEL` and `DESCRIPTION` properties | `Name`, `Description` |
| range value | `VALUE_MIN`, `VALUE_MAX`, `VALUE_NOW` (custom controls implement `GtkAccessibleRange`, so a set value comes back) | the `Value` interface |
| text value | `VALUE_TEXT` | `Value.Text` |
| checked, expanded, selected, busy | states | `GetState` |
| read-only, required, heading level, position in set | properties | attributes and states |
| labelled-by, described-by, controls | relations | `GetRelationSet` |
| automation id | the widget's buildable id | `AccessibleId` (see below) |
| live region | `gtk_accessible_announce` when the name or value changes | an announcement event |
| Invoke on a custom control | the widget's `activate` action | `Action` "activate" |
| a canvas' virtual elements | `RnVirtual` objects, children of the canvas' accessible (`GtkAccessible`'s accessible-object interface) | ordinary accessible children, with their own roles, names, bounds, and actions |

Only what changed since the last render is applied.

## How it is verified

The integration tests (`gtk::accessibility_integration`) read the tree back
over AT-SPI2 itself, as Orca does — a small GDBus client on its own thread
(`gtk::atspi_reader`) walks the application's accessible tree on the
accessibility bus and checks roles, names, descriptions, states, relations,
values, actions, the virtual elements, live-region announcements, and that a
removed node leaves the tree.

## The automation id

GTK reports a widget's buildable id as its AT-SPI `AccessibleId` only from
the versions that implement the property: GTK 4.14 (Ubuntu 24.04) answers
an empty string, and the tests then check the buildable id on the widget
instead; GTK 4.22 (Kali Rolling) answers it, and the tests check
`AccessibleId` itself.
