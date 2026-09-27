# Offline applications (Web milestone I)

An application that declares a `Pwa` — in code, `ServerApp::pwa(Pwa::new("Notes"))`,
or from the `[web.pwa]` table of `rustnative.toml` through
`rustnative_server::web::pwa_config` — works offline and can be installed.

```toml
[web.pwa]
name = "Notes"
short_name = "Notes"
start_url = "/"
display = "standalone"
theme_color = "#1f6feb"
background_color = "#ffffff"
precache = ["/about"]
```

## What every page gets

- `<link rel="manifest">` and `<meta name="theme-color">` in its head, and the
  runtime, which registers the service worker (`/_rn/sw.js`, controlling the
  whole origin). An offline-capable application ships the runtime on every
  page, including pages with nothing interactive.
- `/manifest.webmanifest`, and plain icons in the theme colour at
  `/_rn/icon-192.png` and `/_rn/icon-512.png` when the application gives none.

## What the service worker does

| Request | Strategy |
|---|---|
| The runtime, client modules, the worker script (hashed, immutable) | Precached at install; cache first. |
| A page | Network first; the last copy when there is no network, then the offline page (`/_rn/offline`). |
| A server call (`POST /_fn/…`) with no network | Kept in IndexedDB and answered `503` (`x-rn-queued: 1`), so the client logic learns it did not go through; delivered, oldest first and each once, through Background Sync where the browser has it, and whenever a page sees the network come back. |

## Updates

The worker is versioned by the build (`ServerApp::version`, or the runtime's
hash). A new build's worker installs and waits; the runtime marks the page
(`data-rn-update`) and fires `rn:update`. The application offers the update,
and `rn.applyUpdate()` activates it and reloads the page with its islands'
state kept. `rn.checkUpdate()` asks for a new build at once;
`AppService::set_version` swaps the build in place (a deploy).

## Verified

In headless Edge (`crates/rustnative-server/tests/offline_browser.rs`): the
worker installs and controls the page; with the network emulated offline for
the page and the worker, a visited page and its island load and work; a
server call made offline is queued, delivered after reconnecting, and
delivered once; a new build is announced and applied with the island's state
kept; `Page.getAppManifest` reports the manifest with no errors.
Installation itself (the browser's install prompt) is not exercised headless.
