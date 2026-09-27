# The DOM mapping (Web milestone B)

Every framework node becomes the element a browser already knows how to
present, focus, announce, and submit. There is no canvas renderer and no
emulated control: a button is a `<button>`, a check box is an `<input
type="checkbox">`, and the browser's own accessibility tree is what assistive
technology reads.

The mapping is `rustnative_web::dom::Realizer`. It produces
`rustnative_web::dom::Element`, the one browser tree format every path uses:
the HTML renderer writes it, the runtime realizes and patches it, generated
client views build it, WebAssembly subtrees send it, and the live mode streams
it. The generated JavaScript's `rn.el` helpers build the same elements from the
same inputs, and a test holds them equal.

## Elements

| Node | Element | Notes |
|---|---|---|
| `Label` | `<span>` | a `Heading` role at levels 1–6 is `<h1>`–`<h6>`; deeper headings are a `<span role="heading" aria-level>` |
| `Button` | `<button type="button">` | inside a form it becomes a submit button (Web milestone H) |
| `TextInput` | `<input type="text" name="key">` | |
| `Column`, `Row` | `<div>` | a flex container (see `layout-mapping.md`) |
| a `List` column/row whose children are all `ListItem`s | `<ul>` of `<li>` | a child that cannot be a list item is wrapped in one |
| a `Dialog` column | `<dialog open>` | |
| grid (`Node::grid`) | `<div>` | a grid container |
| virtual list | `<div>` scrolling | a spacer before and after the realized items keeps the whole list's scroll range |
| `TabBar` | `<div role="tablist">` of `<button role="tab">` | the selected tab has `aria-selected="true"` and is the only one in the tab order |
| check box | `<label>` around `<input type="checkbox">` and its text | the label is laid out; the input has the id and the state |
| switch (`toggle`) | the same, with `role="switch"` | |
| radio | `<label>` around `<input type="radio">` | radios of one component share a group name, so arrow keys move between them |
| slider | `<input type="range">` | |
| spinner | `<input type="number" step="1">` | |
| progress | `<progress max="100">` | no `value` while indeterminate |
| select | `<select>` | an empty hidden option while nothing is chosen |
| list box | `<select size>` | |
| date picker | `<input type="date">` | |
| separator | `<hr>` | |
| link | `<a>` | |
| multi-line text | `<textarea>` | |
| image | `<img>` | a PNG `data:` URI with its intrinsic `width` and `height`, so it reserves its space before it decodes |
| canvas | `<div>` with an inline `<svg>` | the draw list as SVG shapes; hit regions are transparent shapes with `data-region` |
| native surface, foreign object | `<div>` | with its accessible name, and `data-foreign` naming a foreign kind |

## Identity

Each keyed node's element has an `id`: its wire key (`key`, or `owner~key`
for a node a child component owns), prefixed with its tree's scope — nothing
for the page, `i0-` for the page's first interactive subtree, and so on. The
same key is the element's identity for keyed updates. Element ids are what
accessibility relationships point at.

The browser tree carries no framework markup beyond that: no `data-` key on
every element, and no whitespace between elements, so the parsed document has
exactly the nodes the tree has and the runtime attaches by walking both.

## Accessibility

HTML semantics first, ARIA where HTML has none:

- a `role` attribute only when the node's role differs from what its element
  already is;
- the accessible name as `aria-label` unless it equals the visible text;
  the description as `aria-description`;
- a focusable node whose element is not focusable gets `tabindex="0"`; a
  button the model says is not focusable gets `tabindex="-1"`;
- range values as `aria-valuemin`/`-max`/`-now` where the element has no
  native value; checked, expanded, selected, busy, live-region, and
  position-in-set states as their `aria-*` attributes;
- read-only and required as the HTML attributes on form controls, and
  `aria-readonly`/`aria-required` elsewhere; disabled likewise;
- `labelled_by`, `described_by`, and `controls` as `aria-labelledby`,
  `aria-describedby`, and `aria-controls`, resolved to the element ids of the
  named nodes (a local key resolves within the naming node's component first);
- the automation id as `data-automation-id`.

Hidden nodes carry the `hidden` attribute, which the base stylesheet makes
authoritative (`[hidden]{display:none!important}`), so a hidden node is gone
from the accessibility tree too.

## Verified

`crates/rustnative-web/tests/browser_dom.rs` renders every node kind under the
strict content security policy in headless Microsoft Edge and reads back the
browser's accessibility tree: heading level, button, named text box, checked
check box, switch, radio, slider, spinner, progress bar, combo box, link,
separator, and a list with its items; and no content-security-policy
violation.
