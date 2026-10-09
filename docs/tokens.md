# Design tokens

`PLAN.md` Milestone 48. A design system becomes running UI through one
artifact: the style file's `@theme` block (Milestone 58).

- A design tool exports a token file.
- `rustnative tokens import` writes it into the `@theme` block.
- The class spelling (`classes!`, `class="…"`) and the typed spelling both
  read from that block.

A theme written by hand is the same artifact.

## Importing

```sh
rustnative tokens import tokens.json        # into the project's style file
rustnative tokens import tokens.json --out theme.css
```

The input is the W3C Design Tokens format. The import writes a block
between `/* tokens:begin … */` and `/* tokens:end */`. Importing again
replaces that block, so the rest of the style file is untouched. The result
is compiled before it is written, so a token file that produces an invalid
theme is refused.

| Token `$type` | Theme namespace |
|---|---|
| `color` | `--color-…` |
| `dimension` in a group named `spacing` or `space` | `--spacing-…` |
| `dimension` in a group named `radius` or `radii` | `--radius-…` |
| `dimension` in a group named `font-size` or `text` | `--text-…` |
| `fontFamily` | `--font-…` |
| `fontWeight` | `--font-weight-…` |

- A token's name is its group path joined with `-`: `brand.blue.500` becomes
  `--color-brand-blue-500`.
- An alias such as `"{brand.blue.500}"` becomes `var(--color-brand-blue-500)`.
- Types without a theme namespace are reported and skipped: shadows,
  durations, gradients, and composite typography.

## Brand values and semantic roles

The schema keeps two kinds of token apart.

- **Brand values** are absolute and are applied as given. The brand's blue
  is the same on every host.
- **Semantic roles** name a purpose: the accent, the surface, the text on
  it. A role may follow the host:

  ```json
  "color": {
    "$type": "color",
    "accent": {
      "$value": "{brand.blue.500}",
      "$extensions": { "rustnative.host": "accent" }
    }
  }
  ```

  Its `$value` is the fallback. In the style file, the role carries a note:

  ```text
  --color-accent: var(--color-brand-blue-500); /* host: accent */
  ```

  The build turns that note into `Theme::with_host_role`.

A token system that can only express absolute values cannot honour a host.
One that can only express roles cannot express a brand. This schema expresses
both.

The host roles are `accent`, `on-accent`, `surface`, `on-surface`,
`highlight`, `on-highlight`, `border`, and `muted`. Each backend supplies a
`HostPalette` for them:

| Role | Windows source |
|---|---|
| `accent` | `DwmGetColorizationColor`, the accent chosen in Settings |
| `on-accent` | black or white, whichever reads on the accent |
| `surface`, `on-surface` | `COLOR_WINDOW`, `COLOR_WINDOWTEXT` |
| `highlight`, `on-highlight` | `COLOR_HIGHLIGHT`, `COLOR_HIGHLIGHTTEXT` |
| `border` | `COLOR_BTNSHADOW` |
| `muted` | `COLOR_GRAYTEXT` |

On Windows the palette is read at start. It is read again on
`WM_SETTINGCHANGE` and `WM_DWMCOLORIZATIONCOLORCHANGED`, so a new accent
restyles the running application. The headless backend uses a fixed
reference palette, which makes goldens identical on every machine.

## The component library's roles

`rustnative-components` writes every style against roles, never against
brand values:

- `accent` and `on-accent`
- `danger` and `on-danger`
- `surface` and `on-surface`
- `muted` and `border`

Its defaults are in `crates/rustnative-components/components.css`. The
accent, surface, text, border, and muted roles follow the host. Danger is a
brand value.

A style pairs a fill with the text colour of the same pair — `accent` with
`on-accent`, `surface` with `on-surface`, `danger` with `on-danger` — so
both follow the host or neither does. A fixed light fill under the host's
text reads in a light scheme and vanishes in a dark one (the neutral badge
once paired a fixed `subtle` fill with `on-surface`; it is now a surface
badge with an outline).

An application's theme passes through `rustnative_components::with_roles`.
Where the application defines a role, its own value is kept. Where it does
not, the library's default is used. An imported token set therefore
restyles the whole library.
