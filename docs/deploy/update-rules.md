# Update rules by host

`PLAN.md` Milestone 50. Each host decides whether an application may update
itself, and how. This page states each host's rule, so the rule is known
before anyone relies on it.

| Host | Self-update | How RustNative updates |
|---|---|---|
| Windows, portable package (ZIP) | Permitted | `rustnative_windows::update`: signed manifests (Ed25519, key pinned at build), staged rollout by installation bucket, side-by-side version directories, and a launcher that rolls back a version failing twice before `interactive` |
| Windows, MSIX installed from a web page or share | Permitted, through App Installer | `rustnative package windows --format msix --appinstaller <url>` writes the `.appinstaller` file that App Installer checks on launch |
| Windows, Microsoft Store | **Forbidden**: the Store updates Store applications | The Store's own update; the updater must not run (`Updater` is not constructed for a Store build) |
| Server (long-lived) | Not applicable: the operator deploys | `rustnative deploy`: immutable revisions, percentage traffic, rollback |
| Web, static and edge | Not applicable | Owed with Web milestones J and K |
| iOS | **Forbidden**: executable code arrives only through the App Store (App Store Review Guideline 2.5.2) | Data and models only (`PayloadKind::Model`); owed with Milestone 36 |
| Android | Google Play forbids self-update outside Play; sideloaded builds may use the package installer | Owed with Milestone 35 |
| Embedded firmware | Permitted with an A/B or bootloader-verified scheme | Owed with Milestone 37 (`C81`) |

## Model and data payloads

An update can carry a model or a data file instead of the application
(`C89`). Its manifest names the application versions the payload works
with, and the payload is verified and staged the same way as an
application update. On hosts that forbid code updates, this is the only
kind of update allowed.
