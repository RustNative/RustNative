# The rendering-surface hand-off

`PLAN.md` Milestone 40: a tree node that owns a host-native rendering surface,
laid out and clipped by the framework, with documented lifetime, resize, DPI,
and present semantics. It is the shape an application with its own rendering
core and a native UI shell needs. The node is `Node::native_surface` (builder)
or `<Surface key="…" />` (markup), added in Milestone 29; this document is its
contract. The Windows backend implements it; the headless backend models it.

## Lifetime

- The surface exists from the render that inserts its node to the render that
  removes it. The framework creates its native object (on Windows, a child
  window of the class `RustNativeFrameworkSurface`) and destroys it.
- The application first learns of it from `Event::SurfaceResized`, which
  carries a `SurfaceId`; `rustnative_windows::native_surface(id)` turns that
  into a `SurfaceHandle` implementing `raw-window-handle`'s traits, the form
  `wgpu`, `ash-window`, and `glutin` accept.
- A handle kept past the node's removal does not dangle: its
  `window_handle()` reports `HandleError::Unavailable` once the window is gone.
  An application drops its swapchain when the node goes away (Windows may reuse
  a handle value).
- The same node keeps the same surface across renders — resizes, moves, and a
  scheme or theme switch never recreate it.

## Resize

- `Event::SurfaceResized` is raised whenever the surface's laid-out size
  changes, including the first time it is laid out. `size` is in the backend's
  device pixels — what a swapchain is created with.
- The framework never paints the surface and never erases its background: every
  pixel is the application's. A resize invalidates nothing on its own; the
  application re-creates or resizes its swapchain on the event.

## DPI

- `scale_factor` is device pixels per layout unit — `GetDpiForWindow / 96` on
  Windows. An application renders its content at that scale.
- When the window's DPI changes (it moved to another monitor, or the display
  setting changed), Windows sends `WM_DPICHANGED` with a suggested window
  rectangle: the framework moves the window to that rectangle and raises
  `SurfaceResized` **for every surface**, even one whose size did not change,
  with the new `scale_factor` — held by
  `native::graphics_integration::a_dpi_change_moves_the_window_and_re_reports_every_surface`.
- Recorded honestly (the Windows unit mapping, `rustnative_style::WINDOWS_UNITS`):
  the framework's layout is not yet scaled by DPI, so a surface's size in
  layout units is its size in device pixels; the scale factor tells the
  renderer how dense those pixels are.

## Present

- Presentation is the application's: the framework does not synchronize with
  the application's swapchain and never presents on its behalf. Painting
  (`WM_PAINT`) is validated without drawing, so the window manager never
  overwrites a frame.
- A surface inside a scroll container, or partly covered by a sibling, is
  clipped by the framework's layout (`WS_CLIPSIBLINGS | WS_CLIPCHILDREN`); the
  application renders its whole surface and the host shows the visible part.
- Rendering may happen on another thread: `SurfaceHandle` is `Send + Sync`. The
  events arrive on the UI thread.
