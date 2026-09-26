# Permission mapping per host (Milestone 39)

The portable states are `NotAsked`, `Granted`, `Limited`, `Denied`, and
`PermanentlyDenied` (`rustnative_core::permission`).

## Windows (`rustnative_windows::WindowsPermissions`)

Windows gates camera, microphone, and location for desktop applications
through the privacy consent store, and never prompts an unpackaged
application — the person decides in Settings, which `open_settings` opens.

| Permission | Read from | Portable state |
|---|---|---|
| Camera | `HKCU\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\webcam` `Value`, and `…\webcam\NonPackaged` `Value` | `Deny` in either → `PermanentlyDenied`; otherwise `Granted` |
| Microphone | `…\ConsentStore\microphone` (same shape) | same |
| Location | `…\ConsentStore\location` (same shape) | same |
| Notifications, Contacts, Photos, Bluetooth | not gated for desktop applications | `Granted` |

`NotAsked`, `Limited`, and `Denied` do not occur on Windows for a desktop
application: there is no prompt to not have been shown, no partial grant,
and no denial an application can ask past. `request` therefore returns the
current state.

## Headless

`FixedPermissions`: configured per test.

## Owed

Android (35), iOS (36), macOS (33), and the Web (E) — the hosts where
`NotAsked`, `Limited`, and `Denied` are real — document their mappings here
when they land.
