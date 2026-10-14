# Security policy

## Supported versions

Security fixes are made on the default branch until the project begins
publishing versioned releases; from the first release, the latest minor
version receives them.

## Reporting a vulnerability

Please do not file public issues for suspected vulnerabilities. Report them
privately through GitHub's private vulnerability reporting: the repository's
**Security** tab → **Report a vulnerability**
(<https://github.com/RustNative/RustNative/security/advisories/new>). Include
a minimal reproduction, the affected revision, the expected impact, and any
mitigation already identified.

Maintainers acknowledge a report within five working days, assess it, agree a
disclosure date with the reporter, and publish an advisory with the fix.

## Scope

The framework treats these as security-relevant defects:

- unsafe or native lifetime issues, stale-event delivery, and resource leaks
  in the backends;
- service-boundary errors: a capability grant bypassed, a credential exposed,
  a certificate pin not enforced;
- the server's secure defaults failing: request-forgery protection, the
  content security policy and its nonce, session confidentiality and
  revocation, passkey ceremonies, rate limiting, request size limits, and
  the response cache serving one visitor's response to another;
- update and package integrity: a manifest accepted without its publisher's
  signature, a package unpacked outside its directory, a downgrade.

The threat models are in `docs/security/`.
