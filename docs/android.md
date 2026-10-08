# The Android backend

`PLAN.md` Milestone 35. A Rust Native application runs on Android phones,
tablets, and foldables with the same components, events, and state as on
the desktops. Its nodes become platform `View`s (`TextView`, `Button`,
`EditText`, `SeekBar`, …), its text is measured and shaped by
`StaticLayout`, its accessibility tree is the `AccessibilityNodeInfo` tree
TalkBack reads, and its services are Android's own: the Storage Access
Framework, the Keystore, `JobScheduler`, ICU.

| | |
|---|---|
| Crate | `rustnative-android` (`AndroidPlatform`), with its Java host library (`java/dev/rustnative/android`, platform APIs only: no AndroidX, no Kotlin) |
| Android | 8.0 (API 26) and newer; built against API 35 |
| ABIs | `arm64-v8a`, `armeabi-v7a`, `x86_64`, `x86` |
| Tested | Redmi Note 14 (Android 16, HyperOS 3) and the API 35 emulator |

## Start

Install the Android SDK (command-line tools, a platform, build-tools), the
NDK, and a JDK 17 or newer — Android Studio's bundled JDK will do — and the
Rust targets:

```sh
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android i686-linux-android
```

`rustnative doctor` finds them through `ANDROID_HOME`/`ANDROID_SDK_ROOT`,
`ANDROID_NDK_HOME` (or the newest NDK under the SDK), and `JAVA_HOME`, and
says what is missing. Then, with a phone connected (USB debugging on) or an
emulator running:

```sh
rustnative run android
```

An application is a library on Android: Android loads it into its own
process and calls in. Put the application in `src/lib.rs` and export its
entry; the desktop `main.rs` calls the same function:

```rust
// src/lib.rs
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut application = Application::new(Root::new(()), Window::new("Notes", Size::new(640, 400)));
    HostPlatform::new().with_app_id(APP_ID).run(&mut application)?;
    Ok(())
}

#[cfg(target_os = "android")]
rustnative_android::export_main!(run);
```

```toml
# Cargo.toml
[target.'cfg(target_os = "android")'.dependencies]
rustnative-android = "0.1"
```

`rustnative build android` asks Cargo for a `cdylib` (`cargo rustc --lib
--crate-type cdylib`), so a desktop build never links a shared library it
does not need.

### What `run` does here

On the desktops `run` owns the thread until the application ends. On
Android the main thread belongs to Android's `Looper`: the launcher
activity's `onCreate` calls the exported `main`, and `AndroidPlatform::run`
**adopts** the application — it moves it out of the caller's binding
(`Application::take`), attaches it to the activity, realizes the first
frame, and returns `Ok(())`. From then on the looper drives it. `main`
should return once `run` has; an error it returns is written to the log
(`adb logcat -s RustNative`).

The application outlives its activities. Rotation, a font-scale change, a
dark-mode switch, a fold, and multi-window resizing are handled in place
(the manifest declares every configuration change), so the same `View`s
stay. An activity the system recreates anyway reattaches to the same
application. A process the system killed starts `main` again, and the state
store brings back what was flushed (see Lifecycle).

## How the tree is realized

| Node | Android |
|---|---|
| Column, Row | `RnLayout` (a `ViewGroup` the portable layout engine places children in); inside a `ScrollView`/`HorizontalScrollView` when it scrolls |
| Label | `TextView` |
| Button | `Button` |
| Text input, password, multi-line | `EditText` with the matching input type |
| Tab bar | `RnTabs`: a row of selectable tabs with the tab-list role |
| Check box, toggle, radio | `CheckBox`, `Switch`, `RadioButton` |
| Slider | `SeekBar` |
| Progress | `ProgressBar` (horizontal; indeterminate while unknown) |
| Select | `Spinner` over an `ArrayAdapter` |
| List box | `ListView` |
| Date picker | a `Button` showing the date, opening a `DatePickerDialog` |
| Spinner (number) | `RnStepper` |
| Separator | a `View` in the theme's divider colour |
| Link | a `TextView` in the theme's link colour, with the link role |
| Image | `ImageView` over a `Bitmap` |
| Canvas | `RnCanvasView`, replaying the draw list on `android.graphics.Canvas`; text through `StaticLayout` |
| Web content (host content) | `WebView` |
| A native surface | `RnSurfaceView` (a `SurfaceView`); its `Surface` is an `ANativeWindow` in `raw-window-handle` form |
| Foreign | the application's own `View` (`register_foreign`, `register_foreign_class`) |
| Virtual list | the visible range only; rows leaving it give their views to rows entering it |

Layout is the portable engine's, in density-independent pixels; `RnLayout`
places each child at its rectangle in device pixels, rounding edges (not
sizes) so neighbours never overlap or gap. Text and controls are measured
by prototype views and `StaticLayout`, remembered until the font scale,
fonts, or theme change. Right-to-left mirroring is the engine's; Android
mirrors what each view draws (`supportsRtl`).

Styles apply only where a node differs from the theme (`Theme.DeviceDefault`,
light and night): an unstyled application looks like the device's other
applications, Material You colours included. A styled background, border,
or radius becomes a `GradientDrawable` (inside a `RippleDrawable`, so
touches still show), state styles a `StateListDrawable` and
`ColorStateList`. The style capability table is `rustnative_style::ANDROID`:
every property realized except a named font family (approximated: Android
resolves generic families, and a bundled font must be an asset) and box
shadows (approximated by elevation).

Animation runs on `Choreographer` frames; transitions apply to the views'
own properties (alpha, translation, scale, rotation) and stop asking for
frames when nothing moves.

## Input

Touch, mouse, and stylus arrive as `MotionEvent`s with their pointer kind,
pressure, and tilt; hover and the mouse wheel included. Keys arrive through
the activity, so a command's shortcut works from anywhere in the window
(`Ctrl` and `Meta` shortcuts on hardware keyboards; the system's keyboard
shortcuts list shows them). Game controllers are `KeyEvent` buttons and
`MotionEvent` axes delivered as `Event::Gamepad` to nodes that ask for
them. The soft keyboard is the system's: platform text fields keep their
own, and a focused canvas or column with the text-input role is a custom
text target whose `InputConnection` reports composition as on the desktops.
Drag and drop is `View.OnDragListener` on nodes that accept drops; dropped
files arrive as `content://` URIs.

**Back.** System back — the button, the gesture, a keyboard's `Escape` —
invokes the standard `BACK` command while a component declares it enabled,
and is the system's own back (leaving the activity) otherwise. On
Android 14 and newer the predictive back gesture's progress reaches the
declaring component as `Event::BackProgress` before the command, so a
screen can preview what going back reveals.

## Accessibility

Each view carries the portable model through an `AccessibilityDelegate`:
the class name TalkBack announces (`android.widget.Button`, `CheckBox`,
`SeekBar`, …, or a role description where no class fits), name, description,
state, value (`RangeInfo`), heading, relations (`setLabeledBy`, traversal
order), collection position, pane titles, and actions. A canvas's virtual
elements come from an `AccessibilityNodeProvider`. Live regions announce
politely or assertively. The device suite reads the tree back through
`UiAutomation`, as TalkBack does, and again with TalkBack running. See
[`android/accessibility.md`](android/accessibility.md).

## Lifecycle

| Android | Portable |
|---|---|
| primary `onResume` | `Lifecycle::Resuming` |
| primary `onPause` | state flushed, then `Lifecycle::Suspending` |
| `onSaveInstanceState` | state flushed (the process may be killed next) |
| primary `onDestroy`, finishing | `Lifecycle::Terminating`; the application ends |
| another window's `onDestroy`, finishing | that window closes |
| `onConfigurationChanged` | host traits re-read, the window relaid out, the same views kept |
| `onTrimMemory` under pressure, `onLowMemory` | `Lifecycle::LowMemory` |
| multi-window, a fold | `keys::WINDOW_MODE`, `keys::POSTURE` |
| a deep link (`ACTION_VIEW` with one of `url-schemes`) | `Event::DeepLink` |
| a share (`ACTION_SEND` of one of `share-types`) | `Event::ShareReceived` |

Persisted state lives in `FileStateStore::for_application()`
(`getFilesDir()/rustnative/state`), written crash-safely. A process the
system killed in the background restores from it when the person returns.
See [`android/lifecycle.md`](android/lifecycle.md).

## Services

| Service | Android |
|---|---|
| `AndroidClipboard` | `ClipboardManager` (Android lets only the focused application read it) |
| `AndroidFileDialogs` | the Storage Access Framework (`ACTION_OPEN_DOCUMENT`, `ACTION_CREATE_DOCUMENT`, `ACTION_OPEN_DOCUMENT_TREE`); the answer is a `content://` URI, read and written with `read_document`/`write_document`, its grant persisted |
| `AndroidSystem` | `ACTION_VIEW` for URLs; notifications on channels, with actions returning as `Event::SurfaceAction`; the share sheet (`share`) |
| `AndroidHttp` | `HttpsURLConnection`: the system's trust store, the application's network security config (cleartext refused by default), the system proxy; certificate pins checked after the handshake, before a byte is written |
| `AndroidLocale` | ICU (`android.icu`) |
| `AndroidSecureStorage` | an AES-GCM key in the Android Keystore — `StrongBox` where the device has one, else the TEE — sealing values in the application's private files |
| `AndroidPermissions` | runtime permissions in the five portable states ([`conformance/permissions.md`](conformance/permissions.md)) |
| `AndroidPrinting` | a PDF (`PdfDocument`) written to a file, or handed to the print UI (`PrintManager`) |
| `AndroidSerial` | unavailable, saying why |
| `AndroidConditions`, `BitmapDecoder` | `ConnectivityManager` and `BatteryManager`; `BitmapFactory`, subsampled then scaled |
| `AndroidWork` | `JobScheduler`: constrained jobs that run even when the application does not (below) |
| `AndroidPush` | Firebase Cloud Messaging when the application ships it (reached by reflection, so others carry no Firebase dependency); unavailable otherwise, saying why |
| `AndroidStore` | unavailable, saying why: the Play Billing bridge is owed |
| `FileStateStore` | crash-safe files under `getFilesDir()/rustnative/state` |

A service may be called from any thread; what Android requires on the main
thread is posted there, and blocking work (HTTP, the Keystore, Play
services) runs on a thread of its own.

**Permissions** must be declared in `rustnative.toml`:

```toml
[android]
permissions = ["android.permission.CAMERA", "android.permission.POST_NOTIFICATIONS"]
```

**Background work.** A job registered with `AndroidWork::register` runs in
whatever process `JobScheduler` starts — possibly one with no activity, in
which `main` never runs — so register handlers in a function the library
runs on load:

```rust
fn register_work() {
    AndroidWork::register("sync", || {
        // ... upload what changed ...
        false // done; `true` asks to run again later
    });
}

#[cfg(target_os = "android")]
rustnative_android::export_main!(run, work = register_work);
```

`AndroidWork::schedule("sync", Constraints { network: true, ..Default::default() })`
then queues it.

## Surfaces

| Portable | Android |
|---|---|
| notifications (`Notify`, `NotifyWithActions`) | a notification on the category's channel, created on first use |
| widgets (`UpdateWidget`) | a home-screen widget (`RemoteViews`) declared in `rustnative.toml` |
| tiles (`UpdateTile`) | a quick-settings tile declared in `rustnative.toml` |
| ongoing activities (`Ongoing`) | an ongoing notification with progress |
| the jump list | the launcher's shortcuts (a long press on the icon) |
| share target | an `ACTION_SEND` filter for `share-types` |
| tray, taskbar progress | unavailable: Android has neither |

```toml
[android]
share-types = ["text/plain", "image/*"]

[[android.widgets]]
id = "today"
label = "Today"

[[android.tiles]]
id = "focus"
label = "Focus mode"
```

Up to four widgets and four tiles. Each keeps what the application last
sent, so the launcher and the shade can show it while the application is
not running; a tap opens the application and arrives as
`Event::SurfaceAction`.

## Menus

A window's menu bar is the activity's options menu in the action bar (the
overflow menu on phones), its items following their commands; context
menus are `PopupMenu`s.

## Customizing a widget

Per-property mappers (`register_mapper`) extend or replace how a node's
text, style, accessibility, or visibility reaches its `View`, with the
`View` in hand through JNI (`NativeView`, `java_vm()`).

## Inspection and the development loop

```sh
rustnative run android --inspect   # start it with the inspector attached
rustnative inspect --android tree  # then ask it anything `rustnative inspect` asks
rustnative dev android             # rebuild, reinstall, and restart on save, keeping state
```

`--inspect` starts the inspection server inside the application, on the
device's loopback (port 7920); `inspect --android` forwards that port with
`adb forward` and reads the endpoint and its token with `run-as`, so it
works with debug builds (which `rustnative` gives `INTERNET`, for the
socket). The inspector sees each node's `View` — its class, identity hash
code, and rectangle — the lifetime census, the capability answers, the
style table, and the mappers, and draws the layout overlay over the
window. `dev android` builds for the device's ABI only, snapshots the
running application's state through the inspector, reinstalls, restarts,
and restores it; theme and catalogue changes apply live, without a
rebuild, as on the desktops.

## Packaging

```sh
rustnative package android                 # a release APK and an App Bundle, every ABI
rustnative package android --format apk    # only the APK (or --format aab)
rustnative build android --abi arm64-v8a   # a debug APK for one ABI
```

`rustnative` writes a Gradle project (Android Gradle Plugin 8.7, Gradle
8.10) under `target/rustnative/android/<name>/`, with the manifest, resources, and the host
library generated from `rustnative.toml`, and runs it. Release signing
names a keystore and the environment variables holding its passwords —
never the passwords:

```toml
[android.keystore]
store = "release.jks"
alias = "release"
store-password-env = "NOTES_STORE_PASSWORD"
key-password-env = "NOTES_KEY_PASSWORD"
```

Without `[android.keystore]` a release is left unsigned (`…-unsigned.apk`),
and `rustnative` says so: sign it before installing or publishing.

## Testing

The device suite runs inside a real activity on a connected phone or
emulator, injecting input through the instrumentation as a person's
arrives:

```sh
tools/android-device-test.sh            # every test, on every connected device
tools/android-device-test.sh services   # the tests whose names contain "services"
```

It keeps the screen on for the run, restores the device's settings
afterwards (screen timeout, accessibility services), and exits non-zero on
any failure. Host tests (`cargo test -p rustnative-android`) cover what
needs no device.

## What is owed

See `BUILD_STATUS.md`: Play Billing (needs a Play Console track), Firebase
push end to end (needs a Firebase project), and the items listed there.
