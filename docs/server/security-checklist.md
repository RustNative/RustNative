# Server security checklist

`PLAN.md` Milestone 49. These defaults are on unless an application turns
them off (`Security`). Shipping without one of them is a defect, not a
missing feature. Each line names the test that holds it.

| Protection | Default | Test (`crates/rustnative-server/tests/`) |
|---|---|---|
| Request forgery: unsafe methods need the double-submit token (`__Host-csrf` cookie, sent back as `X-CSRF-Token` or the form's `_csrf`); bearer-authenticated requests and routes declared `csrf_exempt` are the only exceptions | on | `app.rs` `unsafe_requests_need_the_forgery_token` |
| Content security policy: `default-src 'self'`, scripts and styles only with the response's nonce, no objects, no framing, `base-uri 'none'`, `form-action 'self'`; a fresh nonce per response | on | `app.rs` `every_response_carries_the_security_headers` |
| Security headers: `nosniff`, `X-Frame-Options: DENY`, `Referrer-Policy`, `Permissions-Policy`, HSTS; cross-origin isolation is opt-in | on | same |
| Cookies: `Secure; HttpOnly; SameSite=Lax; Path=/` by default; the session and CSRF cookies use the `__Host-` prefix | on | `auth.rs` `a_session_signs_in_and_policies_guard_routes`; `Cookie` doc test |
| Sessions: sealed with AES-256-GCM, so they cannot be read or forged; a tampered cookie is no session | on | `auth.rs` same test |
| Rate limiting: a token bucket per client, 120 a minute; `429` with `Retry-After` | on | `app.rs` `clients_are_rate_limited_and_bodies_bounded` |
| Request size: bodies over 1 MiB are `413` before a handler sees them | on | same |
| Output escaping: `Html` escapes all text; unescaped markup is only a `&'static str` written in the source | by construction | `Html` doc test; `app.rs` form test; `platform.rs` admin test |
| Error pages: a server error's detail is logged, never shown | on | `app.rs` `error_pages_follow_accept_and_hide_internals` |
| Authorization: every route states its access (`public`, `signed_in`, `authorized::<Policy>`); a route that does not is a compile error | by type | `handler` compile-fail doc test; `auth.rs` |
| Passwords: Argon2id with a random salt, in PHC format | on | `password` doc test |
| Passkeys: challenge, origin, relying party, and user presence checked; a counter that does not advance is refused | on | `auth.rs` `a_passkey_registers_then_signs_in_once_per_counter` |
| Federation: PKCE (S256) and a checked state | on | `auth.rs` `federation_uses_pkce_and_checks_the_state` |
| Row policies: reads through `RowPolicies` are limited to what the principal may see | when declared | `data.rs` `row_policies_hold_for_queries_and_single_rows` |
| SQL: queries are bound parameters; `query!` is checked against the schema at compile time; the admin surface takes names only from the model | by construction | `server-demo` compile-fail doc test; `platform.rs` admin test |
| Secrets: `Secret<T>` never prints or serializes its value, and there is no global configuration | by type | `config` doc test |
