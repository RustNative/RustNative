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
| Service exposure | Operational endpoints leaking | `/__inspect`, `/__cache`, and the deploy control API are behind policies or loopback-only |
| Supply chain | A compromised dependency | `cargo deny` in the gate (licenses, bans, advisories, sources); SBOM and inventory from `rustnative compliance` |
| Observability | Secrets in logs and traces | `Secret<T>` never prints or serializes; spans carry paths, not query strings |
