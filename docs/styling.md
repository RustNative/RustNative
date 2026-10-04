# Styling across hosts

How one style vocabulary reaches hosts that can do very different amounts with
it. The principle is `PLAN.md` 2.14 and the rule for native controls is 2.2;
this document is the working reference for both. Each section says whether
what it describes is **built** or **planned**, and which milestone owns it:

| Part | Status | Milestone |
|---|---|---|
| The portable range, both spellings, capability tables, unit mappings | built (Windows, Linux, Web, headless) | 58 |
| Guards, declared targets, the extended range, structural variants | planned | 67 |
| Windows scaling and composition | planned | 69 |
| Relationships on another node's state or size, generated content, host passthrough | optional | 68 |

## The model

Style is resolved in the core before a backend sees it. Typed properties
(`VisualStyle`, `LayoutStyle`) and utility classes (`classes!`, `class="…"`)
are two spellings of the same declarations; declarations resolve against the
theme; and a backend receives concrete values with no record of which spelling
produced them. There is no selector, no specificity, and no cascade at any
point. Where a style depends on the tree, it does so through a typed
relationship the core resolves, never through a matcher.

## Three ranges

| Range | What is in it | Guard needed |
|---|---|---|
| **Portable** | properties every backend realizes or approximates: colours, fonts, layout, padding and margins, a radius and a border colour, opacity, and display | no — except the shadow, which predates the extended range and which Windows cannot realize |
| **Extended** | typed properties some hosts realize and others do not: border widths and styles, gradients, transforms, filters, motion, text detail, control parts | where the declared targets answer differently |
| **Host passthrough** | host styling with no typed equivalent, on one host | always: it names its host |

The vocabulary is not cut down to what every host shares. That would style
every richer host below what it offers, and fail on the classes a developer
writes first. Instead, every backend answers every property, and the
application says where a difference between its targets is acceptable.

Relationships are not a range. The core resolves them into ordinary properties,
so a relationship is as portable as the properties it sets.

## How a host's answer is used

Every backend answers every property — realized, approximated (and how), or
unavailable (and why) — once for a native control and once for a box the
framework owns, because a host may decorate the second and must never redraw
the first (2.2). The tables are `rustnative_style::WINDOWS`, `LINUX`, `WEB`,
and `HEADLESS`.

What a declaration does on a target, by that target's answer:

| Answer on the target | Unguarded | `supports-[…]:` | `supports-exact-[…]:` | `not-supports-[…]:` |
|---|---|---|---|---|
| Realized | applies | applies | applies | does not apply |
| Approximated | applies, and the build lists it | applies, and the build lists it | does not apply | does not apply |
| Unavailable | **build error** for that target | does not apply | does not apply | applies |

With `approximations = "refuse"` in `rustnative.toml`, an unguarded or
`supports-[…]:` declaration that a declared target only approximates is a build
error too.

*Built today*: an unguarded declaration of a property the Windows table marks
unavailable fails the Windows build at the class. *Planned (Milestone 67)*:
the guards, and checking every declared target rather than the one being
compiled.

## Declared targets

*Planned (Milestone 67).*

```toml
[style]
file = "app.css"
targets = ["windows", "linux", "web"]   # default: every backend the project depends on
approximations = "allow"                # or "refuse"
```

The build checks each unguarded declaration against every declared target's
table. A failure names the class, the property, the target, the target's
reason, and the guard that would resolve it. Adding a target surfaces every
unguarded use at once. `rustnative expand --targets` prints what each declared
target will show for a class string.

## Guards

*Planned (Milestone 67).* A guard is a condition like `hover:` or `dark:`, and
it exists in both spellings.

```rust
// builder — classes
Node::column("card", children)
    .with_class(classes!("rounded-lg supports-[box-shadow]:shadow-lg not-supports-[box-shadow]:border"))
```

```rust
// builder — typed (the method name is settled by Milestone 67; values illustrative)
Node::column("card", children)
    .with_style(VisualStyle::new().border_radius(8))
    .with_style_when(Condition::supports(StyleProperty::Shadow), VisualStyle::new().shadow(Shadow::lg()))
    .with_style_when(Condition::not_supports(StyleProperty::Shadow), VisualStyle::new().border(Color::GRAY_300))
```

```rust
// card.rsx
<Column key="card" class="rounded-lg supports-[box-shadow]:shadow-lg not-supports-[box-shadow]:border">
    {children}
</Column>
```

- **Capability variants** ask the capability table, never the browser's
  `@supports`, so a guard means the same thing on every host. They are the
  default choice: they keep working when a backend gains a property.
- **Target variants** — `windows:`, `linux:`, `macos:`, `android:`, `ios:`,
  `ipados:`, `web:`, `tui:`, `embedded:` — are for a deliberate difference that
  is not a capability question: a host convention, or a brand decision made per
  platform.
- **Decided per backend, once.** A guard is evaluated against the table of the
  backend the tree is realized on, not against the operating system the code
  was compiled for — one binary can serve a page to a browser and open a native
  window, and one universal bundle runs the iOS backend on an iPhone and the
  iPadOS backend on an iPad from the same compiler target. The answer is a
  constant of the backend; nothing is evaluated per frame. For the same reason
  the build checks unguarded declarations against each declared target's table,
  never by `cfg(target_os)`, which cannot tell `ios` from `ipados`.

## The headless reference

The headless backend realizes every property, by recording its value in the
inspectable model. It is what the equivalence suite, golden tests, and replayed
sessions resolve against (Milestone 45). A property it could not record could
not be tested anywhere, so a property enters the vocabulary only together with
its headless realization.

## The extended range

*Planned (Milestone 67).* Properties arrive by family, each with both
spellings, an equivalence case, and every backend's answer. The answers below
are expectations, and the backend's table is the authority once the family
lands. "Box" means a container the framework owns; "control" a native control.

| Family | Utilities | Windows | Linux | Web | Headless | Terminal |
|---|---|---|---|---|---|---|
| Borders | `border-2`, `border-t-4`, `border-dashed`, `rounded-tl-lg` | box: approximated (GDI pens; per-corner radius as a region, not anti-aliased); control: system border | GTK CSS | CSS | yes | style approximated with box-drawing characters; radius unavailable |
| Backgrounds | `bg-linear-to-r`, `from-*`, `via-*`, `to-*`, radial and conic, images | box: linear realized (`GradientFill`), radial and conic unavailable; control: unavailable | GTK CSS | CSS | yes | unavailable |
| Transforms | `translate-*`, `scale-*`, `rotate-*`, `origin-*` | translate realized by moving the window without relayout; scale and rotate unavailable | by its table | CSS | yes | unavailable |
| Effects | `blur-*`, `brightness-*`, `backdrop-*`, `mix-blend-*` | unavailable | by its table | CSS | yes | unavailable |
| Motion | `transition-*`, `duration-*`, `ease-*`, `delay-*`, `animate-*` | Milestone 27 timeline | GTK frame clock, same timeline | CSS generated from the timeline model | yes | timeline at the terminal's frame rate |
| Text | `tracking-*`, `leading-*`, `underline`, `uppercase`, `text-center`, `truncate`, `line-clamp-*` | decoration, alignment, truncation, case realized; line clamp approximated; letter spacing and line height unavailable on controls | GTK CSS | CSS | yes | case, truncation; others unavailable |
| Layout extras | `aspect-*`, `z-*` | shared layout engine | shared layout engine | CSS from the same meaning | yes | shared layout engine |
| Interaction | `cursor-*`, `pointer-events-none`, `select-none`, `outline-*`, `ring-*` | cursor and pointer events realized; ring approximated on boxes, controls keep the system focus visual | by its table | CSS | yes | no pointer cursor; ring as attributes |
| Control parts | `placeholder:`, `caret-*`, `selection:`, `accent-*` | mostly unavailable (the common controls do not expose them) | by its table | CSS | yes | mostly unavailable |

## Motion

*Planned (Milestone 67).* The motion utilities lower onto Milestone 27's
animation model: a class-spelled transition becomes a `Transition` on the node,
and a keyframe animation in `app.css` an `Animation`. So it is evaluated by the
same `Timeline`, interrupted from its current value and velocity, cancelled
with its component, and skipped or kept under reduced motion — the same way on
every backend that runs frames. In the browser the generated stylesheet carries
the same transition, with a spring approximated by a `linear()` easing. A
property the timeline cannot animate is unavailable under `transition-[…]`.

## Structural variants

*Planned (Milestone 67).* `first:`, `last:`, `only:`, `odd:`, `even:`,
`nth-[n]:`, `nth-last-[n]:`, `empty:`, and `*:`.

- Resolved after reconciliation from the parent's child list, into ordinary
  properties every backend applies.
- A keyed insert, removal, or move re-resolves that parent's children and
  nothing else.
- A hidden node keeps its position. A virtual list counts logical items, so
  `odd:` stays on the same rows while the list scrolls and recycles.
- `*:` applies the parent's declarations to its direct children, below each
  child's own declarations: a child's own style always wins.
- In the browser, the generated rule counts the framework's own nodes only, so
  spacers and host-only elements never shift a position.

## Relationships on another node (optional)

*Optional (Milestone 68).* `group-*`, `peer-*`, and `has-*` make a node's style
depend on another node's interaction or form state; they resolve by bounded
lookups — up to the nearest named group, back to an earlier sibling, down to
direct children — through a dependency index that re-resolves only the nodes
whose conditions name the node that changed, without a render.

Until they are built, the component model already covers each case: a card
that knows it is hovered passes that to its children as a prop or through the
environment, and they style themselves with an ordinary state or a conditional
style. That is why this part is optional.

## Container queries (optional)

*Optional (Milestone 68).* `@container`, `@sm:`, `@md:`, and `@min-[…]:` decide
by a container's resolved inline size. As in the browser, a query container's
inline size may not depend on its children, so a layout settles in at most one
extra pass; a container that breaks the rule is reported by the layout
diagnostics. Until then, a container decides by its own size class in code
(Milestone 39).

## Generated content (optional)

*Optional (Milestone 68).* `before:` and `after:` as decorative child nodes the
reconciler inserts, hidden from the accessibility tree, keyed from their
parent. A component can add the node itself today.

## Host passthrough (optional)

*Optional (Milestone 68).* Host styling with no typed equivalent, allowed only
under a target variant naming a host that speaks a styling language of its own
(`web:[mask-type:alpha]`). The core carries it uninterpreted to that backend
alone; it is outside the equivalence suite, printed by `rustnative expand` as
passthrough and unverified, emitted into the generated stylesheet rather than
inline, and counted per project. It is the escape hatch for style (2.6), not a
styling channel: adding the property to the extended range is always the
preferred route.

## Windows: decorate the box, never the control

The Windows backend realizes nodes as Win32 windows and common controls. A
native control keeps what the host gives it — its shape, focus visual, text
editing, and accessibility — and the backend never captures a control's output
into a bitmap to rotate, scale, blur, or shadow it. A container has no native
equivalent: it is a box the framework owns, and the backend may decorate it.

*Planned (Milestone 69)*, in this order:

1. **Scale.** Layout in logical pixels, scaled by each window's DPI; fonts,
   borders, regions, and surfaces following the same factor; `WM_DPICHANGED`
   handled live on the same windows. Today the unit mapping equates a logical
   pixel with a device pixel (`rustnative_style::WINDOWS_UNITS`), and every
   length the extended range adds would be measured at the wrong scale until
   this is done.
2. **Composition for boxes.** DirectComposition — the host's own compositor —
   behind framework-owned containers, for anti-aliased corners, shadows,
   gradients, and fades. Its limit is stated rather than discovered: every
   native child window is its own surface, so a decoration cannot draw over a
   neighbouring control, a translucent or blurred box cannot blend over the
   controls beneath it, and animating a box that holds controls moves real
   windows each frame. It starts as a time-boxed prototype measured against
   `budgets/windows.toml`, and is adopted or not on the record.
3. **The control-set decision, named and not taken.** WinUI 3's controls are
   native too, and composition runs through its whole tree, so it has no
   airspace limit — but it is a different control set and effectively a second
   Windows backend. It is decided as a control-set question, on written
   criteria, not as a styling one.

## What bounds the payoff

The extended range shows unevenly, and that is stated rather than discovered.
In practice it shows in the browser, on Linux, in the headless backend, and on
the layer-backed hosts still to come; on Windows, most transform and effect rows
are unavailable, and an application shows each guard's fallback there. Guards
make the difference honest; they do not make it equal. Milestone 69 is what can
change Windows' answers, and Milestone 67 does not wait for it.

The cost is ongoing: every property is an answer owed by every backend,
including backends not yet written. That is why properties arrive by family
with their tests, and why a family no host realizes beyond the browser is not
added for the browser alone.

## Where each piece is specified

- `PLAN.md` 2.2 (decorate the box), 2.14 (the five rules and the three ranges),
  Milestone 27 (timelines), Milestone 58 (the portable range), Milestone 67
  (the extended range), Milestone 68 (optional relationships and passthrough),
  Milestone 69 (Windows scale and composition), Web milestone C (the browser's
  mapping).
- `docs/web/layout-mapping.md`: how the browser receives styles.
- `docs/linux.md`: how GTK receives them.
- `docs/conformance/new-backend-checklist.md`: what a backend owes.
- `docs/tokens.md`: the token file and design-token import.
