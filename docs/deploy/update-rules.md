# Update rules by host

`PLAN.md` Milestone 50. Each host decides whether an application may update
itself, and how. This page states each host's rule, so the rule is known
before anyone relies on it.

| Host | Self-update | How RustNative updates |
|---|---|---|
| Windows, portable package (ZIP) | Permitted | `rustnative_windows::update`: `rustnative_update`'s signed manifests (Ed25519 over the published bytes, key pinned at build, application id checked, expiring), staged rollout by installation bucket, side-by-side version directories, and a launcher that rolls back a version failing twice before `interactive` |
| Windows, MSIX installed from a web page or share | Permitted, through App Installer | `rustnative package windows --format msix --appinstaller <url>` writes the `.appinstaller` file that App Installer checks on launch |
| Windows, Microsoft Store | **Forbidden**: the Store updates Store applications | The Store's own update; the updater must not run (`Updater` is not constructed for a Store build) |
| macOS, outside the Mac App Store (Developer ID) | Permitted: a notarized application may replace itself | Owed with Milestone 33: the same signed manifests and staged rollout as Windows, the new bundle verified against the manifest and notarized, swapped atomically beside the running one |
| macOS, Mac App Store | **Forbidden**: the store updates store applications | The store's own update; data and models only (`PayloadKind::Model`) |
| Linux, distribution packages (`.deb`, `.rpm`, pacman) | **Not the application's**: the package manager owns installed files | The distribution's repository or the publisher's own; `rustnative package linux` writes the packages, and the application never writes into its installation |
| Linux, AppImage | Permitted, by replacing the AppImage file | Owed: the signed manifest and staged rollout of Windows over the single AppImage file (the AppImage update information format is the host's convention for it) |
| Linux, Flatpak | **Not the application's**: the Flatpak repository updates it | The repository's own update |
| Server (long-lived) | Not applicable: the operator deploys | `rustnative deploy`: immutable revisions, percentage traffic, rollback |
| Web, static and edge | Not applicable: a deploy is a new revision | `rustnative deploy` (static hosts and edge modules are revisions like any other); a page from an older build that calls the server is told to reload (`x-rn-fn-version`), and an offline-capable application's new service worker waits until the application offers the update and `rn.applyUpdate()` activates it with the islands' state kept (`docs/web/offline.md`) |
| Terminal applications | As the operating system they run on: the rows above for Windows, macOS, and Linux apply to the binary | Owed with Milestone 38: the host's row applies to how the binary was installed (a distribution package is the package manager's; a standalone binary may use `rustnative_update` as the portable ZIP does) |
| iOS | **Forbidden**: executable code arrives only through the App Store (App Store Review Guideline 2.5.2) | Data and models only (`PayloadKind::Model`); owed with Milestone 36 |
| iPadOS | **Forbidden**, under the same rule as iOS: an iPad application is distributed through the same store, often in the same universal bundle | Data and models only (`PayloadKind::Model`), one manifest for both targets of a universal bundle; owed with Milestone 70 |
| Android, from Google Play | **Forbidden** for code: Play updates the application, and its policy forbids updating code any other way | Play's own update (in-app update prompts are Play's API, not this updater); `rustnative_android::update::AndroidUpdater::install` refuses (`UpdateError::Forbidden`, decided from the installing package); model and data payloads allowed (`stage_model`) |
| Android, sideloaded | Permitted, through the system package installer, with the person's confirmation | `AndroidUpdater`: the same signed manifests and staged rollout as Windows; the APK verified against the manifest's SHA-256, then committed to a `PackageInstaller` session — Android shows its confirmation and checks the new APK is signed with the same key; the application declares `REQUEST_INSTALL_PACKAGES` |
| Embedded firmware | Permitted with an A/B or bootloader-verified scheme | Owed with Milestone 37 (`C81`) |

Every self-updating row shares one manifest format, verifier, and decision
(`rustnative_update`; `docs/deploy.md`); a backend holds only how its host
installs a staged version.

## Model and data payloads

An update can carry a model or a data file instead of the application
(`C89`). Its manifest names the application versions the payload works
with, and the payload is verified and staged the same way as an
application update. On hosts that forbid code updates, this is the only
kind of update allowed.
