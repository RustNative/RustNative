# Styling across hosts

How one style vocabulary reaches hosts that can do very different amounts with
it. The principle is `PLAN.md` 2.14 and the rule for native controls is 2.2;
this document is the working reference for both. Each section says whether
what it describes is **built** or **planned**, and which milestone owns it:

| Part | Status | Milestone |
|---|---|---|
| The portable range, both spellings, capability tables, unit mappings | built (Windows, Linux, Android, Web, headless) | 58 |
| Declared targets (`[style] targets`) | built | 58, 2026-10-12 review |
| Guards, the extended range, structural variants, `approximations` | planned | 67 |
| Windows per-monitor scaling | built | 69 (first part) |
| Windows control sets: WinUI 3 by default, WinUI 2 and the common controls as fallbacks, one table each | planned | 71 |
| Windows composition through each control set's visual layer | planned | 69 |
| Relationships on another node's state or size, generated content | planned | 68 |
| The whole Tailwind CSS v4 vocabulary, current with upstream, proven against upstream's compiler | planned | 73 |
| Host passthrough | optional | 68 |

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
the first (2.2). The tables are `rustnative_style::WINDOWS`, `LINUX`,
`ANDROID`, `WEB`, and `HEADLESS`.

What a declaration does on a target, by that target's answer:

| Answer on the target | Unguarded | `supports-[…]:` | `supports-exact-[…]:` | `not-supports-[…]:` |
|---|---|---|---|---|
| Realized | applies | applies | applies | does not apply |
| Approximated | applies, and the build lists it | applies, and the build lists it | does not apply | does not apply |
| Unavailable | **build error** for that target | does not apply | does not apply | applies |

With `approximations = "refuse"` in `rustnative.toml`, an unguarded or
`supports-[…]:` declaration that a declared target only approximates is a build
error too.

*Built today*: an unguarded declaration of a property a declared target's
table marks unavailable fails the build at the class, for every declared
target, whatever machine compiles the project. *Planned (Milestone 67)*: the
guards that resolve such a failure, and `approximations`.

## Declared targets

*Built.* The `[style]` table of `rustnative.toml` names the backends the
application is realized on:

```toml
[style]
file = "app.css"
targets = ["windows", "linux", "android"]
```

`classes!` and `styles!` check each unguarded declaration against every
declared target's table, and a failure names the class, the property, the
target, and the target's reason. Adding a target surfaces every unguarded use
at once. A web project lists `["web"]` (`rustnative new --web` writes it), so
its server binary — compiled for Windows or Linux — is checked against the
Web's table, which realizes shadows, rather than against the compiling
machine's. Planned targets (`macos`, `ios`, `ipados`, `tui`, `embedded`) are
accepted and checked once their tables exist; an unknown name is an error.

For `windows`, the table checked is that of the project's primary control set
(*planned, Milestone 71*): `[windows] control_sets = ["winui3", "winui2",
"win32"]` by default, in order of preference. A declared fallback set is
checked too, and what it leaves out is written down once rather than
discovered: with `[windows] fallback_styles = "list"`, the default, the build
lists each declaration a fallback set will not show; with `"refuse"`, every
declared set is held to the primary's rule.

Without `targets`, the native backend of the operating system the project is
compiled for is checked — right for a native application built where it
runs, and the reason a project spanning hosts should declare them.

*Planned (Milestone 67)*: `approximations = "refuse"`, and `rustnative expand
--targets`, which prints what each declared target will show for a class
string.

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
  platform. There is no variant per Windows control set or per architecture: a
  difference between control sets is a capability question, answered by the
  table of the set the window was realized in.
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

| Family | Utilities | Windows — WinUI 3 | Windows — common controls | Linux | Web | Headless | Terminal |
|---|---|---|---|---|---|---|---|
| Borders | `border-2`, `border-t-4`, `border-dashed`, `rounded-tl-lg` | per-side width and anti-aliased per-corner radius on boxes and controls; dashed and dotted on boxes | box: approximated (GDI pens; per-corner radius as a region, not anti-aliased); control: system border | GTK CSS | CSS | yes | style approximated with box-drawing characters; radius unavailable |
| Backgrounds | `bg-linear-to-r`, `bg-radial`, `bg-conic`, `from-*`, `via-*`, `to-*`, images, `bg-clip-text` | all realized: linear and radial as XAML brushes, conic drawn by Direct2D into a composition surface, gradient text as a foreground brush; on boxes and as a control's background resource | box: linear realized (`GradientFill`), radial and conic unavailable; control: unavailable | GTK CSS | CSS | yes | unavailable |
| Shadows and masks | `inset-shadow-*`, `text-shadow-*`, `drop-shadow-*`, `mask-*` | shadows on boxes and controls; text shadows from the text's alpha mask; masks on boxes, images, canvases (unavailable on a control's own pixels) | unavailable outside composed containers (Milestone 69) | by its table | CSS | yes | unavailable |
| Transforms | `translate-*`, `scale-*`, `rotate-*`, `skew-*`, `origin-*`, `perspective-*`, `rotate-x-*`, `rotate-y-*`, `transform-3d`, `backface-hidden` | every 2D and 3D transform on boxes and controls, through the element's transform matrix; the control stays live | translate realized by moving the window without relayout; the rest unavailable | by its table | CSS | yes | unavailable |
| Effects | `blur-*`, `brightness-*`, `contrast-*`, `grayscale`, `hue-rotate-*`, `invert`, `saturate-*`, `sepia`, `backdrop-*`, `mix-blend-*`, `bg-blend-*` | every filter and blend on boxes, images, canvases, and drawn text through composition effects; every backdrop filter over anything beneath, controls included; unavailable on a control's own pixels | unavailable | by its table | CSS | yes | unavailable |
| Motion | `transition-*`, `duration-*`, `ease-*`, `delay-*`, `animate-*`, `starting:` | Milestone 27 timeline, run on the compositor thread wherever the curve maps exactly | Milestone 27 timeline | GTK frame clock, same timeline | CSS generated from the timeline model | yes | timeline at the terminal's frame rate |
| Text | `tracking-*`, `leading-*`, `underline`, `decoration-*`, `uppercase`, `text-center`, `truncate`, `line-clamp-*`, `font-stretch-*`, `font-features-[…]`, `tabular-nums` | all realized through the text elements' properties and DirectWrite; on an editable control, what the control exposes | decoration, alignment, truncation, case realized; line clamp approximated; letter spacing, line height, and font features unavailable on controls | GTK CSS | CSS | yes | case, truncation; others unavailable |
| Layout extras | `aspect-*`, `z-*` | shared layout engine | shared layout engine | shared layout engine | CSS from the same meaning | yes | shared layout engine |
| Interaction | `cursor-*`, `pointer-events-none`, `select-none`, `outline-*`, `ring-*`, `scrollbar-*` | realized; a control's focus visual takes the ring's colour and thickness; scrollbars on the host's own scroller | cursor and pointer events realized; ring approximated on boxes, controls keep the system focus visual | by its table | CSS | yes | no pointer cursor; ring as attributes |
| Control parts | `placeholder:`, `caret-*`, `selection:`, `accent-*` | placeholder, selection, and accent through the controls' own properties and resources; caret where exposed | mostly unavailable (the common controls do not expose them) | by its table | CSS | yes | mostly unavailable |

WinUI 2 answers each row in its own table, realizing most of what WinUI 3 does
through the operating system's composition layer.

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

## Relationships on another node

*Planned (Milestone 68).* `group-*`, `peer-*`, `in-*`, and `has-*` make a node's
style depend on another node's interaction or form state; they resolve by
bounded lookups — up to the nearest named group or ancestor in that state, back
to an earlier sibling, down to direct children — through a dependency index
that re-resolves only the nodes whose conditions name the node that changed,
without a render. They are part of the vocabulary a developer arrives with, so
they are built rather than left to the props route; that route stays the one
the component library prefers wherever typed state reads more clearly.

## Container queries

*Planned (Milestone 68).* `@container`, `@sm:`, `@md:`, `@max-*:`, and
`@min-[…]:` decide by a container's resolved inline size. As in the browser, a
query container's inline size may not depend on its children, so a layout
settles in at most one extra pass; a container that breaks the rule is reported
by the layout diagnostics.

## Generated content

*Planned (Milestone 68).* `before:` and `after:` as decorative child nodes the
reconciler inserts, hidden from the accessibility tree, keyed from their parent.

## The whole Tailwind CSS vocabulary

*Planned (Milestone 73).* The vocabulary is Tailwind CSS v4 at a pinned upstream
release, taken whole, and the pin follows the current release. Every upstream
utility, variant, modifier, and directive sits in one row of a generated
coverage table: compiled in a range, compiled as a relationship, passthrough
only, or refused with the reason and the replacement. A corpus generated from
upstream's own compiler, only when the pin moves and never in a build, holds the
declarations upstream emits for each class, and the framework's must equal them.

What upstream spells as a selector is, here, almost always a fact the framework
already holds, and compiles from that fact:

- `aria-*:` from the node's accessibility state; `data-*:` from the node's own
  typed values (`with_data`, or a `data-*` attribute in markup);
- `checked:`, `invalid:`, `user-invalid:`, `required:`, `placeholder-shown:`,
  `read-only:`, and the other form states from the forms model; `open:`,
  `inert:`, and `focus-within:` from the node and the focus path;
- `contrast-more:`, `forced-colors:`, `portrait:`, `print:`, `pointer-fine:`,
  and the other media features from the environment; `max-*:` and stacked
  breakpoints from the size classes;
- `starting:` as an animation from the node's initial values; `not-*:` as a
  negated condition.

Also: `!` as a documented precedence step, negative and fractional values,
arbitrary properties (an ordinary declaration over a typed property, host
passthrough otherwise), a compile error for two classes setting the same
property under the same condition, and conditional composition
(`classes!("px-4", active => "bg-primary")`) with every fragment still a checked
literal. An upstream style file compiles as written — `@import "tailwindcss"`,
single-class `@layer components` rules, `@source`, functional `@utility`,
`@variant`, `@property` — or names each line it refuses; `@plugin` and
`@config` stay refused, since no build runs JavaScript. `rustnative fmt` sorts
class strings into upstream's canonical order, and `rustnative import tailwind`
converts an existing project's style entry point.

Host capabilities upstream has no word for — the host materials
(`material-mica`, `material-acrylic`, …) and elevation tokens — are framework
utilities in a namespace upstream does not use, listed in the coverage table as
framework additions.

## Host passthrough (optional)

*Optional (Milestone 68), taken up on demand.* Host styling with no typed equivalent, allowed only
under a target variant naming a host that speaks a styling language of its own
(`web:[mask-type:alpha]`). The core carries it uninterpreted to that backend
alone; it is outside the equivalence suite, printed by `rustnative expand` as
passthrough and unverified, emitted into the generated stylesheet rather than
inline, and counted per project. It is the escape hatch for style (2.6), not a
styling channel: adding the property to the extended range is always the
preferred route.

## Windows: three control sets, one shell

*Planned (Milestone 71).* The Windows backend realizes the tree in WinUI 3 by
default — the host's current control set, composed through one visual layer —
inside its Win32 windows. Where the Windows App SDK runtime cannot load, it
falls back to WinUI 2 over the operating system's own XAML; where no XAML can
run, to the Win32 common controls the backend was first built on. The choice is
made once at launch and reported, never by `cfg`, and each set has its own
table: `rustnative_style::WINDOWS_WINUI3`, `WINDOWS_WINUI2`, and `WINDOWS` for the
common controls.

Under every set a native control keeps what the host gives it — its shape,
focus visual, text editing, and accessibility — and the backend never captures a
control's output into a bitmap or replaces a XAML control's template. Under the
XAML sets, the host itself transforms, clips, fades, and shadows a live control
without capturing it, so those rows are realized on controls too; filtering,
masking, or blending a control's own pixels stays unavailable, because that
would be capture.

WinUI 3 is the richest native host the vocabulary reaches, and its table is
written to use all of it: every row it leaves unavailable names the host limit
behind it (Milestone 73 tracks that count).

Milestone 69, in this order:

1. **Scale** *(done, 2026-10-14)*. Layout runs in logical pixels, scaled by
   each window's DPI where geometry reaches Win32; fonts, padding, rounded
   regions, canvases, and surfaces follow the same factor; `WM_DPICHANGED` is
   handled live on the same windows (`rustnative_style::WINDOWS_UNITS`). Under
   the XAML sets, XAML scales its own tree by the island's rasterization scale,
   held to the same rounding rule.
2. **Composition** *(planned)*. Under the XAML sets, the control sets' own
   visual layer realizes corners, shadows, gradients, masks, filters, backdrop
   filters, blends, fades, 2D and 3D transforms, and compositor-thread motion.
   Airspace remains only where a child window remains — an adopted foreign
   window, or a surface for a graphics API that needs a window handle. Under the
   common controls, DirectComposition behind framework-owned containers is a
   time-boxed prototype with every child window's airspace limit stated, adopted
   or not on the record.

## What bounds the payoff

The extended range shows unevenly, and that is stated rather than discovered.
It shows in full in the browser and under WinUI 3, widely on Linux, in the
headless backend, and on the layer-backed hosts still to come; WinUI 2 shows
most of it; the common controls, as a fallback, show the narrower answers and
list what they leave out (`fallback_styles`); the terminal shows each guard's
fallback. Guards and declared fallbacks make the difference honest; they do not
make it equal.

The cost is ongoing: every property is an answer owed by every backend and
control set, including those not yet written. That is why properties arrive by
family with their tests, and why a family no host realizes beyond the browser is
not added for the browser alone.

## Where each piece is specified

- `PLAN.md` 2.2 (decorate the box), 2.14 (the six rules and the three ranges),
  Milestone 27 (timelines), Milestone 58 (the portable range), Milestone 67
  (the extended range), Milestone 68 (relationships, container queries, and the
  optional passthrough), Milestone 69 (Windows scale and composition),
  Milestone 71 (the Windows control sets), Milestone 73 (the whole Tailwind
  vocabulary), Web milestone C (the browser's mapping).
- `docs/web/layout-mapping.md`: how the browser receives styles.
- `docs/linux.md`: how GTK receives them.
- `docs/conformance/new-backend-checklist.md`: what a backend owes.
- `docs/tokens.md`: the token file and design-token import.
