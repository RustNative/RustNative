# Vendored files

## `vendor/tailwind-theme-4.1.13.css`

The default theme of **Tailwind CSS v4.1.13** — the token namespaces
(colour, spacing, radius, text, font, shadow, breakpoint, easing, …) that
the utility vocabulary resolves against when a project's `app.css` does not
replace them.

- Upstream: `tailwindcss@4.1.13`, file `theme.css`, as published to npm
  (fetched from `https://cdn.jsdelivr.net/npm/tailwindcss@4.1.13/theme.css`).
- SHA-256: `5d1f0002f98471fa59bc53ec2bd2488ff3e4661d965f7fa665408218d63187cb`.
- Licence: MIT, Copyright (c) Tailwind Labs, Inc. — the full text is
  `vendor/LICENSE-tailwindcss`.
- Unmodified. The parser (`src/sheet.rs`) skips what the style model has no
  use for (`@keyframes`, `--theme()` defaults, untyped namespaces) rather
  than the file being edited.

**Updating the pin** is deliberate: replace the file, update the version in
its name, this section, and `DEFAULT_THEME` in `src/vocabulary.rs`, and run
`cargo test -p rustnative-style`, whose palette tests compare converted
colours against the published values.
