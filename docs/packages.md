# Capability packages and feature kits

`PLAN.md` Milestone 52 (`C71`, `C57-3`). A community crate can add a
capability to any application: a service, or a control's native half, with
code for each backend. It needs no fork of the framework and no global
registry. This page covers what such a package declares, how an
application installs it, and how the tool checks it.

## Writing a package

A package implements `rustnative_core::package::CapabilityPackage`:

```rust
impl CapabilityPackage for BatteryPackage {
    type Service = Box<dyn BatteryService>;

    fn manifest(&self) -> PackageManifest {
        PackageManifest::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"), &["windows", "headless"], ">=0.1, <0.2")
    }

    fn build(&self, scope: ScopedServices, backend: &str) -> Option<Self::Service> {
        match backend {
            "windows" => Some(Box::new(WindowsBattery)),
            "headless" => Some(Box::new(Fixed(/* … */))),
            _ => None,
        }
    }
}
```

The manifest, which `PackageManifest::needs(grant)` extends, declares:

- **the backends it has code for**;
- **the framework versions it works with** (`>=0.1, <0.2`, or `0.1`,
  which Cargo reads as `^0.1`);
- **the grants it needs**.

`build` receives a `ScopedServices` that holds only those grants, and
Milestone 51 enforces them at each call.

The same facts go in the package's `Cargo.toml`, where `rustnative add`
and the index read them:

```toml
[package.metadata.rustnative]
contract = "BatteryService"
backends = ["windows", "headless"]
framework = ">=0.1, <0.2"
grants = []
```

`examples/package-battery` is a complete package, written as a third party
would write it. Its test checks that the `Cargo.toml` table and the
manifest agree.

## Installing a package

```rust
let services = Services::default().install(&BatteryPackage, "windows")?;
let battery = services.package::<Box<dyn BatteryService>>("package-battery");
```

`install` checks, in order, that:

1. the package has code for this backend;
2. the framework version is in its range;
3. the package built its service here.

If a check fails, `install` returns a `PackageError` with the reason. Its
service is kept under the package's name in these services, never in a
process-wide registry, so two applications (or two tests) in one process
do not see each other's packages.

## The tool

```sh
rustnative search battery              # the index: what each package does, and whether it fits
rustnative add path/to/package-battery # read its metadata, check it, and add the dependency
rustnative add package-battery         # the same, from the index
```

`add` refuses a package that has no code for the project's backends or
does not accept this framework version, and says which. It also prints
the grants the package asks for.

## The index

An index is a JSON array. The tool ships one, `docs/packages/index.json`,
and `--index <file>` names another, such as an organization's own:

```json
[{
  "name": "package-battery",
  "version": "0.1.0",
  "description": "The machine's battery and power source",
  "contract": "BatteryService",
  "backends": ["windows", "headless"],
  "framework": ">=0.1, <0.2",
  "grants": []
}]
```

## Feature kits

`rustnative generate kit auth|admin|commerce` writes working, tested code
into the project's `src/kits/`, on the server application model
(Milestone 49). The code is the project's to change:

- **auth**: sign-up, sign-in, sign-out, and `/auth/me`.
  - Argon2id password hashes, and sealed session cookies.
  - The request-forgery token on every change.
  - The same answer for an unknown name and a wrong password.
  - The first account administers.
- **admin**: the generated admin surface over the database, for
  administrators only. It builds on `auth`.
- **commerce**: a catalogue, and a checkout that validates each receipt
  on the server before recording the entitlement. It sells through the
  portable `CommerceService`, starting on `FakeStore`.

The kits' sources are compiled and tested in this repository as
`examples/kits`, exactly as `generate` writes them.
