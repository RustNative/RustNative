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

## Android (`rustnative_android::AndroidPermissions`)

Android is the first host where all five states are real. Runtime
permissions are asked with the system's prompt (`requestPermissions`,
answered in `onRequestPermissionsResult`), and must be declared in the
manifest (`[android] permissions` in `rustnative.toml`); an undeclared one
is refused without a prompt.

| Permission | Android permission | Notes |
|---|---|---|
| Camera | `CAMERA` | |
| Microphone | `RECORD_AUDIO` | |
| Location | `ACCESS_FINE_LOCATION` | `ACCESS_COARSE_LOCATION` alone → `Limited` (the person chose "approximate") |
| Notifications | `POST_NOTIFICATIONS` (API 33+) | below API 33: install-time; `Granted` while notifications are enabled, `PermanentlyDenied` when the person turned them off |
| Contacts | `READ_CONTACTS` | |
| Photos | `READ_MEDIA_IMAGES` (API 33+), `READ_EXTERNAL_STORAGE` below | `READ_MEDIA_VISUAL_USER_SELECTED` alone (API 34+) → `Limited` ("select photos") |
| Bluetooth | `BLUETOOTH_CONNECT` (API 31+) | below API 31: install-time, `Granted` |

| State | When |
|---|---|
| `Granted` | `checkSelfPermission` grants it |
| `Limited` | only the partial permission above is granted |
| `NotAsked` | not granted, and this application has never asked |
| `Denied` | asked and refused, and `shouldShowRequestPermissionRationale` is true: asking again shows the prompt |
| `PermanentlyDenied` | asked and refused, and no rationale: "don't ask again", refused twice, or blocked by policy — only the settings page (`open_settings`) can change it |

Android cannot tell "never asked" from "don't ask again" by itself:
`shouldShowRequestPermissionRationale` is false for both. The backend keeps
its own record of what the application asked (in private preferences),
which settles it. `request` on a `Granted` or `PermanentlyDenied`
permission answers at once without a prompt.

Revoking a granted runtime permission (in Settings, or `pm revoke`) makes
Android kill the application's process; it restarts with the permission
`Denied` or `PermanentlyDenied` as the record and the rationale say.

## Linux (`rustnative_linux::LinuxPermissions`)

An unsandboxed desktop application (a distribution package, a tarball, an
AppImage) is not gated: it has every resource the person's account has, and
every permission is `Granted`. Inside a Flatpak or Snap sandbox, two
resources are asked for at use through the desktop portals; the rest are
static permissions of the sandbox's manifest:

| Permission | Unsandboxed | Sandboxed |
|---|---|---|
| Camera | `Granted` | the Camera portal's `AccessCamera`, asked on request: `NotAsked` until this run's request is answered, then `Granted` or `Denied` |
| Location | `Granted` | the Location portal's session, asked on request, likewise |
| Microphone, Notifications, Contacts, Photos, Bluetooth | `Granted` | `Granted` (the manifest's) |

The portal's permission store is not readable from inside the sandbox, which
is why a new run starts at `NotAsked`. Tests:
`rustnative_linux::services::permissions::tests`.

## The Web

The Permissions API's states through `fx.permission` and
`fx.request_permission` (`rustnative_web::capability`): `granted` →
`Granted`, `denied` → `Denied`, `prompt` (or the browser's `default`) →
`Prompt`, a permission the browser does not know → `Unsupported`; see
`docs/web/capabilities.md`.

## Headless

`FixedPermissions`: configured per test.

## Owed

iOS (36), iPadOS (70), and macOS (33) — hosts where `NotAsked`, `Limited`,
and `Denied` are real — document their mappings here when they land:
macOS's privacy consent (TCC) per service, with `NotAsked` until the first
request and only System Settings changing a refusal (`PermanentlyDenied`);
iOS's authorization status per framework, with `Limited` for a partial
photo library and approximate location. iPadOS reads the same host permission APIs as iOS
through the shared UIKit group crate, so its section states only where an
iPad answers differently — and a permission for hardware a given iPad lacks
is answered as an absent capability, not as `Denied`.
