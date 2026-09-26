# Portable API audit (Milestone 39)

Two audits, done while there is one native backend — the last moment they
are cheap.

## 1. Desktop affordances, as capabilities

| Affordance | Portable form | Capability | Windows | Headless |
|---|---|---|---|---|
| Window management | `Window`, `WindowState`, `ComponentContext::windows` | `WindowManagement`, `MultipleWindows` | yes | yes (model) |
| Menus | `MenuBar`, `MenuItem::command` | `Menus` | yes | no |
| Shortcut maps | `Command::shortcut`, `Application::handle_shortcut` | `CommandShortcuts` | yes | yes (through `press`) |
| Hover and cursors | `PointerEnter`/`PointerLeave`, `Node::with_cursor` | `Hover`, `Cursors` | yes | no |
| Drag and drop | `InputInterest::drop_target`, `DragData` | `DragAndDrop` | yes | no |
| Multi-window state | `Application::open_window`, per-window environment | `MultipleWindows` | yes | yes |

A host that lacks one answers the capability negatively; no backend is
expected to imitate an affordance its host does not have.

## 2. Windows assumptions found in the portable API, and their resolution

| Found | Resolution |
|---|---|
| `EdgeInsets` had `left`/`right`: a physical vocabulary that cannot mirror | fields are now `start`/`end`; `EdgeInsets::left(direction)`/`right(direction)` answer the physical question; `EdgeInsets::logical(top, end, bottom, start)` |
| Layout rectangles were implicitly physical | `LayoutResult::rects` are logical; `physical_rects` mirrors for hosts without their own mirroring, so a host with it (Windows `WS_EX_LAYOUTRTL`, a browser) is not mirrored twice |
| Host settings were read by components (motion) or not at all (scheme, scale, contrast, locale) | the typed environment (`rustnative_core::environment`), fed by each backend |
| Menu items were enabled and checked once, when built | `MenuItem::command` binds live command state, applied by the host as the menu opens |
| Keyboard shortcuts were application code comparing `KeyCode`s | `Command::shortcut`, matched once in the core against live declarations |
| Low memory was not a lifecycle event (Windows has no message for it) | `Lifecycle::LowMemory`, realized on Windows from the resource notification |
| Rendering re-rendered every component on every change | the invalidation contract (`docs/invalidation.md`): dirty components and readers of changed values only |
| Time was read from the OS clock directly where it was read at all | `Clock`, through `Services::clock` and `Executor::now` |
| Tasks had to be `Send` (a browser main thread and a single-core loop cannot provide that) | `LocalExecutor`, `ComponentContext::spawn_local` |

Still Windows-shaped, recorded so the next backend does not rediscover it:

- `KeyCode::Unknown(u32)` carries a native virtual-key code — deliberately,
  as an escape hatch; each backend documents what the number means.
- `Theme::default()` names a font family (`system-ui`) that each backend
  resolves to its own UI font.
