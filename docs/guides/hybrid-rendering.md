# Hybrid rendering

`PLAN.md` Milestone 48. A natively realized tree can contain custom-drawn
regions. The portable accessibility model still supplies the semantics
for what is drawn. A purely native framework cannot draw what the host has
no control for. A purely self-drawing one has to rebuild the host's
controls and their accessibility. This pattern avoids both problems.

## The pattern

1. Build the screen from native nodes: labels, buttons, the controls of
   `rustnative_core::control`, and the components of `rustnative-components`.
2. Where the design needs something the host does not have, such as a
   chart, a timeline, or a custom gauge, put a `Node::canvas` there. It
   takes a draw list (Milestone 29). On Windows it is drawn with
   Direct2D.
3. Give the canvas `AccessibilityInfo` that describes **what it shows**,
   not that it is a picture:
   - a role (`Table`, `Slider`, `Group`, …);
   - a name;
   - **elements**, one per thing a person would point at, each with its
     own name and bounds (`AccessibilityInfo::elements`).
4. Handle keys on the canvas so the elements can be reached without a
   pointer. When the focused element changes, update a polite live region.

The canvas is laid out, clipped, scrolled, and mirrored like any other
node. Its semantics join the same UI Automation tree as its native
neighbours.

## An example: the library's chart

`rustnative_components::Chart` follows the pattern exactly:

- The plot is a canvas. Its draw list is built by
  `rustnative_core::graphics`, with the palette taken from the theme.
- Its role is `Table`. Its description says what it plots: "Bar chart of 2
  series over 3 categories…".
- Every data point is an element named "series, category: value", with its
  bar's bounds. A screen reader can list them or move through them.
- The arrow keys move a focus through the points, and a status label
  announces each one.

`crates/rustnative-components/tests/library.rs` asserts every one of these
properties.

## Rules

- **Draw what the host lacks, not what it has.** Drawing a button in a
  canvas throws away the host's focus, keyboard, and accessibility
  behaviour. It also loses high-contrast handling, text scaling, and IME.
  The library's components are native wherever the host has the control.
- **Semantics are data, not an afterthought.** The draw list and the
  accessibility elements come from the same model in the same render, so
  they cannot disagree.
- **Respect the host's settings.**
  - Take colors from theme tokens. The role tokens follow the host
    (`docs/tokens.md`), and high contrast replaces them.
  - Scale text with the environment's `TEXT_SCALE`.
  - Honour reduced motion for anything animated.
- **Hit-testing is the canvas's own.** Pointer events arrive at the canvas
  with local coordinates. Map them to the element under the pointer with
  the same geometry the draw list used.

## Hosts without native controls

On the embedded and terminal targets (owed; see `BUILD_STATUS.md`), more of
the tree is drawn. The same behaviour layer
(`rustnative_components::behaviour`) and the same accessibility model apply,
so the controls behave the same way everywhere.
