# The layout and style mapping (Web milestone C)

This is the one backend whose host owns layout. The core's layout engine does
not run in the browser; what the core owns instead is the *meaning* of each
layout property, and `rustnative_web::css` states that meaning in CSS so the
browser's flexbox and grid compute what the engine would. There are never two
layout engines deciding the same subtree. This document is the negotiation,
with every approximation named.

## Who computes what

| Concern | Computed by | How |
|---|---|---|
| Positions and sizes | the browser | flexbox for columns and rows, grid for grids |
| Intrinsic measurement, text wrapping | the browser | its own fonts and line breaking |
| What `Fill`, `Auto`, `Fixed`, alignment, and constraints mean | the framework | the CSS below |
| Overflow, clipping, scrolling | the browser | `overflow` from the container's `Overflow` |
| Right-to-left mirroring | the browser | the `dir` attribute and logical properties; the application states only start and end |
| Viewport size, device-pixel ratio, zoom | the browser | CSS pixels; see the unit mapping |
| Responsive conditions (`md:`, `dark:`, `motion-reduce:`, `pointer-coarse:`) | the browser | media queries in the generated stylesheet |
| Interaction states (`hover:`, `focus:`, `active:`, `disabled:`) | the browser | pseudo-classes, so a hover never re-runs anything (2.10) |

## A child in a column (and, with the axes swapped, a row)

The main axis is height in a column and width in a row.

| Layout | CSS | Why |
|---|---|---|
| main axis `Fixed(n)` | `flex:0 0 auto; height:n px` | exactly `n`, never shrunk |
| main axis `Auto` | `flex:0 0 auto` | its natural size |
| main axis `Fill` | `flex:1 0 auto` | the engine gives a filling child its natural size plus an equal share of what is left, and never shrinks it |
| cross axis `Auto` or `Fill` under `Stretch` | `align-self:stretch` | the full cross size |
| cross axis `Fill` under any alignment | `align-self:stretch` | the engine gives it the full cross size too |
| cross axis `Auto` under `Start`/`Center`/`End` | `align-self:flex-start`/`center`/`flex-end` | its natural size, capped by what the parent offers |
| cross axis `Fixed(n)` | `width:n px; max-width:100%` and its alignment (`flex-start` under `Stretch`) | the engine caps a fixed cross size at the space available |
| margin | `margin-block` / `margin-inline` | logical, so start and end mirror |
| constraints | `min-width`, `max-width`, `min-height`, `max-height` | a minimum wins over a maximum, as in the engine's clamp |

Every element also gets `min-width:0; min-height:0`, because the engine never
imposes a content-based minimum the way flexbox does by default.

## Containers

| Container field | CSS |
|---|---|
| `padding` | `padding-block` / `padding-inline` |
| `gap` | `gap` |
| `align_items` | `align-items` (`flex-start`, `center`, `flex-end`, `stretch`) |
| `overflow: Visible` / `Clip` / `Scroll` | `overflow: visible` / `hidden` / `auto` |
| grid tracks `Fixed(n)` / `Auto` / `Fraction(n)` | `n px` / `auto` / `n fr` in `grid-template-columns` and `-rows`; further rows `auto` |
| grid placement | `grid-area: row+1 / column+1 / span rows / span columns` |
| a grid child's size | it fills its cell; a fixed size is capped at the cell and sits at its start |

## The root

A native window's root fills the window. The page's root fills the viewport
(`body` is a column flex container at least the viewport's height, and the
root grows in it) and grows with its content, which the document scrolls.

## Style

The typed style and the declaration vocabulary reach the browser as classes in
one stylesheet, in cascade order: the fixed base rules; the theme's defaults
per node kind (`.rn-label`, `.rn-button`, …); the node's typed layout (`l…`);
its typed visual override, state styles, opacity, and cursor (`v…`); each
declaration set (`d…`); and the parts of its sets that depend on its parent
(`n…`). A class is named by the hash of its rule, so the runtime names a
class exactly as the server did.

Declarations reach the browser *unresolved*: a token stays `var(--name)` with
the page's theme defining it on `:root`, so a theme change is one
re-resolution here as on every backend; `rem` stays `rem`, so it follows the
person's text-size setting live; and every condition becomes the media query
or pseudo-class it names, decided where the person's window and settings
actually are. A declaration of width, height, self-alignment, or display is
written per node, because in flexbox a size means something different on the
main and the cross axis and a display must restore the node's own (flex for a
container, the element's default otherwise).

## Approximations

Named here rather than discovered:

- **Pixel distribution.** When space left over is shared by several filling
  children, the engine gives whole pixels and the remainder to the first;
  the browser shares fractional pixels and snaps to device pixels.
- **Text metrics.** Wrapping and natural sizes come from the browser's fonts,
  so a label's measured size differs from a native backend's by what the
  fonts differ by.
- **Grid auto-placement.** Unplaced children use the browser's dense row
  flow; the engine takes the next free cell. They agree whenever placed
  children come before unplaced ones or leave no holes.
- **The root.** Fills the viewport and grows with its content (the document
  scrolls), where a native root is exactly the window.
- **Borders.** A border colour is a one-pixel solid border; native backends
  keep a native control's system border.
- **Font families.** The whole family list reaches the browser, which uses
  the first installed family; native backends use the first family named.
- **`focus-visible:`** is keyboard focus only in the browser; the Windows
  backend treats it as focus.
- **Canvas text** is drawn in the browser's font.
- **Virtual lists** are sized from their extents: the realized items sit
  between spacers the length of the unrealized items.

## Units

`rustnative_style::WEB_UNITS`: one logical pixel is one CSS pixel; the browser
maps CSS pixels to device pixels by its device-pixel ratio, which follows the
display and the page zoom, so a device-pixel-ratio change needs nothing from
the framework. `rem` is the CSS `rem`. The framework rounds nothing: the
browser lays out in fractional CSS pixels.

## Verified

`crates/rustnative-web/tests/browser_dom.rs`, in headless Microsoft Edge: a
right-to-left row puts its first child at the right edge inside its start
padding, a fixed width is honored exactly, a declared colour applies, and a
`md:` padding changes when the viewport crosses 768 pixels.
