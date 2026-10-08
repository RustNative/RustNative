# The lifecycle on Android

Android owns an application's process and decides when its activities
come and go. The backend maps that onto the portable lifecycle
(`Lifecycle`) and state-restoration contracts, so an application written
for the desktops survives what Android does to it.

## The application outlives its activities

`AndroidPlatform::run` adopts the application into the process; activities
attach to it and detach from it:

- **Configuration changes are handled in place.** The manifest declares
  every configuration change (orientation, screen size, density, font
  scale, night mode, locale, keyboard, …), so a rotation, a fold, a dark-mode
  switch, or a multi-window resize re-reads the host traits and lays the
  window out again with the same views.
- **A recreated activity reattaches.** Should Android recreate an activity
  anyway, its views go and the next activity gets new ones from the same
  application, with its components and their state.
- **Another window is another activity** (`RnWindowActivity`). Closing it closes that window; finishing the
  primary activity ends the application.

## Steps

| Android | Portable |
|---|---|
| primary `onResume` | `Lifecycle::Resuming` |
| primary `onPause` | state flushed, then `Lifecycle::Suspending` |
| `onSaveInstanceState` | state flushed: the process may be killed next |
| primary `onDestroy`, finishing | `Lifecycle::Terminating`; the application ends |
| another window's `onDestroy`, finishing | the window closes (`Application::close_window`) |
| `onDestroy`, not finishing | the window's views go; the window stays for the next activity |
| `onConfigurationChanged` | host traits re-read; the window relaid out |
| `onTrimMemory(RUNNING_LOW, RUNNING_CRITICAL, MODERATE, COMPLETE)`, `onLowMemory` | `Lifecycle::LowMemory` (caches dropped); `UI_HIDDEN` and `BACKGROUND` are not pressure — suspension already covered them |
| `onMultiWindowModeChanged` | `keys::WINDOW_MODE` |
| a folding feature (window extensions) | `keys::POSTURE` |

## Process death

Android kills a backgrounded process when it needs the memory, without
calling anything. Everything the application persists (`Persisted` state,
navigation stacks) is flushed to `FileStateStore` at every `onPause` and
`onSaveInstanceState`, so it is on disk before the process can die. When
the person returns, Android starts a new process, the launcher activity
runs `main` again, and the state store restores what was flushed before
the first frame. Nothing needs to be done for it beyond giving the
application a `FileStateStore`.

## Intents

A deep link (`ACTION_VIEW` with one of `rustnative.toml`'s `url-schemes`) —
at launch, or to the running application through `onNewIntent` — arrives as
`Event::DeepLink`, after the restored first frame when it starts the
process. Content shared to the application (`ACTION_SEND` of one of
`share-types`) arrives as `Event::ShareReceived`. A tap on a notification,
widget, tile, or launcher shortcut arrives as `Event::SurfaceAction`.
