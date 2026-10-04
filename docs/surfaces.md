# Surfaces beyond the main window, and product services

`PLAN.md` Milestone 57. Milestone 39 defined the vocabulary. This page
says, for each surface and service, what the Windows backend realizes and
what it answers `Unavailable` (with the reason).

## Asking for surfaces

An application asks through the `Surfaces` handle in its services. The
backend applies what was asked after each change, and a surface answers
with `Event::SurfaceAction`, routed to the primary window's root
component like a menu choice.

```rust
fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
    let surfaces = context.services().surfaces();
    surfaces.show_tray("Notes", vec![TrayMenuItem::new("export", "Export")]);
    surfaces.set_jump_list(vec![JumpTask { label: "New note".into(), arguments: "notes://new".into() }]);
    surfaces.set_progress(Some(0.4));   // None removes it
    surfaces.notify("Export finished", "Your notes were exported.");
    // …
}

fn update(&mut self, event: Event) {
    if let Event::SurfaceAction { action, .. } = event {
        // "export" (a menu item's id), "activate" (the icon), "notification"
    }
}
```

## Surfaces (`Capability::Surface`)

| Surface | Windows | How |
|---|---|---|
| `TrayExtra` | **Realized** | `Shell_NotifyIconW`: an icon, a tooltip, and a menu. A click on the icon, a menu choice, and a click on its notification arrive as `Event::SurfaceAction`. |
| `JumpList` | **Realized** | `ICustomDestinationList`: tasks, each starting the application with its arguments (a URL reaches the running instance as `Event::OpenUrl`, Milestone 30) |
| `TaskbarProgress` | **Realized** | `ITaskbarList3::SetProgressValue` and `SetProgressState` |
| `Widget` | Unavailable | Windows 11 widgets need a packaged widget provider (MSIX with a COM widget-provider registration). This build has none. |
| `Extension` (share target) | Unavailable | A share target needs MSIX package identity and `uap:ShareTarget` activation. This build has none. |
| `LiveActivity`, `Tile`, `InstantApp`, `CompanionDevice` | Unavailable | Not Windows concepts (live tiles were retired in Windows 11) |

## Notifications

Notifications are realized through the notification area's balloon
(`NIF_INFO`), which Windows 10 and 11 show as a toast.

- **Clicks.** A click arrives as `Event::SurfaceAction` with the action
  `"notification"` (`surfaces::NOTIFICATION`).
- **Toast actions.** WinRT toasts with several buttons
  (`ToastNotificationManager`) need an Application User Model ID. They
  also need either package identity or a Start-menu shortcut that carries
  that ID and a COM activator. A plain executable has neither, so
  multiple-button toasts are **owed** with MSIX identity.

## Services

| Service | Windows | Notes |
|---|---|---|
| `SecureStorage` | **Realized** | Credential Manager. Values are sealed with DPAPI for the signed-in user. `hardware_backed: false` (the logon secret protects it, not a TPM). `biometric_gating: false`. |
| Feature flags (`Flags`) | **Realized** (portable) | Compiled defaults are overridden by remote configuration, then by local overrides (`RUSTNATIVE_FLAGS`). `to_cache` keeps the remote values for offline use. |
| `PushService` | Unavailable | WNS channels (`PushNotificationChannelManager`) need Store-associated package identity. The server-side sender exists (Milestone 49: `wns_request`). |
| `CommerceService` | Unavailable | The Microsoft Store's `StoreContext` needs a Store-associated application. `FakeStore` implements the contract for development and tests, and checks its own receipts (`FakeStore::validate`). Validating a real store's receipts on the server is owed with the store's billing. |

## The reference application

`examples/product-services` shows a tray icon and menu, a jump list,
taskbar progress for an export, a notification whose click returns to the
application, a token in secure storage, and a feature switched on
remotely. It uses no native code of its own.

The mobile reference application in the plan (a widget, a share
extension, push, and purchases) is owed with Milestones 35, 36, and 70.
