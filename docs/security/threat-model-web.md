# Threat model: the Web backend

`PLAN.md` Web milestones A–K. This covers pages, islands, server functions,
WebAssembly subtrees, offline applications, and the serverless shapes. The
server beneath them is `docs/security/threat-model-server.md`; its checklist
is `docs/server/security-checklist.md`.

## Boundaries

- **The browser.** Everything in it is the person's, and possibly an
  attacker's. The page, the runtime, generated modules, WebAssembly, the
  service worker, and IndexedDB are all readable and modifiable by whoever
  controls the browser.
- **The application.** The server, a function, or an edge module. It
  decides, and it holds secrets.
- **The host** (a function runtime, an edge host, a static host). It
  enforces limits the application states and relies on. In the edge shape
  it mediates all input and output.
- **The data service.** It is reached from the serverless shapes over HTTP,
  with a key.

## Threats and mitigations

| Entry point | Threat | Mitigation | Where it is held |
|---|---|---|---|
| Server-rendered HTML | Script injection through data | Every text and attribute is escaped by the renderer; no API writes raw HTML into a page. The policy allows scripts and styles only with the response's nonce (`script-src 'self' 'nonce-…'`) | `rustnative-web/tests/browser_dom.rs` (a strict policy in a real browser); `security::secure_headers` |
| Page data (`#rn-data`) | Breaking out of the JSON script element | Serialized with `<`, `>`, `&` escaped; parsed with `JSON.parse`, never evaluated | the renderer's page-data writer |
| Islands and runtime | A tampered client state | State that arrives from the browser is input. Server functions re-check access (`signed_in::<U>()`, policies) and validate their inputs; nothing trusts an island's state | `rustnative-web/tests/pages.rs`, server-function tests |
| Server functions | Cross-site request forgery | Double-submit token: the `__Host-csrf` cookie against `x-csrf-token` or the form's `_csrf`, compared in constant time; a bearer token is exempt (it cannot ride along) | `security::check_csrf`; `pages_in_a_browser` (refused without the token) |
| Server functions | A page from an older build calling a changed function | The call carries the build's version; a mismatch is `409` and the page reloads | `rustnative-server/tests/pages.rs::a_call_from_another_build_is_told_to_reload` |
| Sessions | Theft or forgery; a session readable by another shape | Sealed with AES-GCM under one key in a `__Host-`, `Secure`, `HttpOnly`, `SameSite=Lax` cookie; readable by any instance only because every instance has the key | the server's session tests; `tests/serverless.rs` (one session, every shape) |
| Framing | Clickjacking | `frame-ancestors 'none'` | `secure_headers` |
| Browser capabilities | A page using a capability it did not declare | `Permissions-Policy` opens only the capabilities the application declares; requests go through the capability layer | `rustnative-server/tests/pages.rs::the_permissions_policy_opens_only_declared_capabilities` |
| WebAssembly subtrees | Code execution from data; a module escaping its sandbox | `'wasm-unsafe-eval'` is added only to pages that have a subtree. Modules import only `rn_now`, `rn_wake`, `rn_request` and reach services through the host's bridge, which applies the page's policy | `wasm_browser.rs`; `docs/web/wasm-subtrees.md` |
| Custom elements | A host page feeding hostile attributes | Attributes and properties are input: read into the component's state fields by type, never as markup | `rustnative-web/tests/element.rs` |
| Service worker | Private pages served from the cache to another person; a stale build | Pages are `no-store` and served network-first. Only the build's hashed assets are precached, and an update is taken on the person's say | `offline_browser.rs`; `docs/web/offline.md` |
| Offline queue | Replaying a call twice; replaying under another session | Queued calls replay oldest first and leave the queue once the server has answered. A call whose answer was lost on the way back can arrive twice (at least once), so a function that must not repeat takes an idempotency key (`docs/durable.md`). Replays carry the cookie the browser has at replay, and the server's checks run again | `offline_browser.rs` |
| Static export and host | Path traversal out of the export (`..`, `\`, drive letters) | `rustnative serve static` refuses `..`, backslashes, and colons in paths | `crates/rustnative/src/web.rs::tests::paths_that_leave_the_folder_are_refused` |
| Function runtime | Work leaking across invocations; runaway invocations | Each invocation has its own runtime, dropped at the response. A late invocation is refused, and a timed-out instance is replaced | `rustnative-server/tests/serverless.rs` |
| Edge host | A module exhausting the host | Fuel per route, a memory ceiling, a response cap, a deadline on sleeps | `crates/rustnative/tests/serverless.rs` |
| Edge host | A module reaching arbitrary hosts (server-side request forgery) | Outbound HTTP goes through the host, to the allowed `host:port`s only | `emulate::wagi::fetch` |
| Edge host | A module reading the host's files | No preopened directories; every other WASI call is `ENOSYS` | `emulate::wagi` |
| Data service | Another client reading or writing the data | A key in `x-data-key`, compared in constant time, and no route without it; on by configuration only | `examples/web-notes` `local::data_service` |
| Dependencies | A compromised crate in the browser or the host | `cargo deny` in the gate; the runtime is one reviewed file with no npm dependencies | the gate |

## Not covered

- **Other browsers.** Firefox and Safari are not run. The policy, the
  cookies, and the service worker are standard, but only Chromium engines
  are tested.
- **Providers' hosts.** The Lambda and edge emulators follow the providers'
  documented contracts, but a real account's behavior (IAM, network
  policies, a provider's own limits) is not tested.
- **Denial of service at the front.** Rate limiting is per client address
  on the server. Behind a proxy or a provider it belongs there.
