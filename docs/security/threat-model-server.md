# Threat model: servers

`PLAN.md` Milestone 51, for `rustnative-server` applications.

| Entry point | Threat | Mitigation |
|---|---|---|
| Every request | Forgery, injection, clickjacking, sniffing | The secure defaults and their tests: `docs/server/security-checklist.md` |
| Authentication | Credential stuffing, session theft, token forgery | Rate limiting; Argon2id; sealed `__Host-` session cookies; HMAC tokens; passkeys (counter check) |
| Authorization | Reaching another person's data | Access in each route's type; row policies for reads (`C39`); the admin surface behind a policy |
| Database | SQL injection, schema drift | Bound parameters only; `query!` checked at compile time; migrations with a dry run |
| Background work | Duplicate effects, poison messages | Idempotency keys, exactly-once transactional steps, dead letters (Milestones 49, 56) |
| Deployment | A bad release, a tampered artifact | Immutable revisions, percentage rollout, one-command rollback; a single signed artifact with integrity-checked assets (Milestone 50) |
| Deploy control API (`rustnative deploy local`, loopback) | A web page in the operator's browser posts a revision pointing at its own server and promotes it (request forgery, DNS rebinding), so the public proxy sends every client — cookies included — to it | A per-run random token, kept in a `0600` file in a per-user directory (`$XDG_RUNTIME_DIR/rustnative`, else `~/.rustnative/run`) and required as `Authorization: Bearer` on every route, compared in constant time; `Host` must be `127.0.0.1:<port>` or `localhost:<port>`; any `Origin` or `Sec-Fetch-Site` other than `none` is refused; `Json` bodies must be `application/json` (`415` otherwise), so a no-preflight `text/plain` post never parses |
| Deployment previews (`x-revision` on the public proxy) | Any client reaches a revision nobody promoted: an unreviewed build, or an old one kept for rollback | `x-revision` is honoured only with the run's preview token in `x-preview-token`, compared in constant time, else `403`; a splitter without a token serves no previews; the proxy strips both headers before forwarding (`tests/deploy.rs::a_preview_needs_the_preview_token_and_the_revision_never_sees_it`) |
| Service exposure | Operational endpoints leaking | `/__inspect`, `/__cache`, and the deploy control API are behind policies or loopback-only |
| Supply chain | A compromised dependency | `cargo deny` in the gate (licenses, bans, advisories, sources); SBOM and inventory from `rustnative compliance` |
| Observability | Secrets in logs and traces | `Secret<T>` never prints or serializes; spans carry paths, not query strings |
