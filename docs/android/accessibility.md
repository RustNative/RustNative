# Accessibility on Android

The portable accessibility model (`docs/accessibility.md`) reaches
TalkBack, Switch Access, Voice Access, and every other accessibility
service as the `AccessibilityNodeInfo` tree. Each realized view carries an
`AccessibilityDelegate` the backend drives from the portable
`AccessibilityTree` after every render, for what changed only; a canvas's
virtual elements are virtual views of an `AccessibilityNodeProvider`.

| Portable | Android |
|---|---|
| role | the class name services read (`android.widget.Button`, `CheckBox`, `SeekBar`, `ProgressBar`, `EditText`, `ImageView`, `TabWidget`, …), or a role description where no widget class says it |
| name, description | `contentDescription` (when it is not the visible text), hint text |
| range value | `RangeInfo`, with `ACTION_SET_PROGRESS` and scroll forward/backward |
| text value | `stateDescription` (API 30+) |
| checked, selected, expanded, busy, required | checkable/checked, selected, expand/collapse actions, the state description |
| heading | `setHeading` (API 28+) |
| read-only | not editable |
| position in set | `CollectionItemInfo` |
| labelled-by | `setLabeledBy` |
| automation id | `setViewIdResourceName`, the node's key (what UI Automator and Espresso find it by) |
| live region | `accessibilityLiveRegion`, and an announcement for an assertive change |
| hidden | `IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS` |
| virtual elements | an `AccessibilityNodeProvider`'s virtual views, explored by touch |
| actions a service performs | the native control's own action first (a `SeekBar` sets its progress), else `Event::AccessibilityAction` |

## Verified how

The device suite (`device_tests/accessibility.rs`) reads the application's
own window through `UiAutomation` — the same `AccessibilityNodeInfo`s
TalkBack reads — and compares roles, names, states, ranges, relations, and
virtual children with the portable tree; it performs actions as a service
does and checks they reach the component; and it runs again with TalkBack
itself started (`enabled_accessibility_services`), restoring the device's
setting afterwards. The automation connects with
`FLAG_DONT_SUPPRESS_ACCESSIBILITY_SERVICES`, so a running TalkBack is not
stopped while the suite reads.

On HyperOS, TalkBack's first start shows a tutorial over the application;
the suite reads the application's window by package rather than "the
active window", so the tutorial does not hide it.
