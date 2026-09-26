# Stability and deprecation policy

`PLAN.md` Milestone 52. This page states what an application can rely on
across releases, and what the project does when something must change.
Tooling keeps these promises.

## What is covered

The public API of every published `rustnative-*` crate:

- types, functions, and traits;
- the markup syntax (`rsx!` and `.rsx`);
- the style spellings: utility classes and declarations;
- the `rustnative.toml` format;
- the `rustnative` command line;
- the inspection protocol;
- the wire formats of Milestones 49 and 55.

Two things are not covered:

- items marked `#[doc(hidden)]`;
- anything under `docs/superpowers/`, which holds working notes.

## Versions

The crates follow semantic versioning and are released together under one
version number (a seam count of one, section 11).

- **Before 1.0**, a minor release (`0.x` to `0.x+1`) may break the API. A
  patch release never does.
- **From 1.0**, only a major release may break the API.

## Deprecation

- **Warn first.** An item that will go is marked `#[deprecated(since,
  note)]`. The note names its replacement.
- **The support window.** The item stays for at least two minor releases
  after the one that deprecated it. During that window the old and new
  spellings both work.
- **Markup and style.** The same rule covers the markup syntax and the
  style spellings: the compiler warns about a deprecated element,
  attribute, or class, naming its replacement.

## Automated migration

Every breaking change ships with a **codemod** in `rustnative upgrade`
(`C57-2`).

```sh
rustnative upgrade --from 0.0 --dry-run   # what would change, and what needs a person
rustnative upgrade --from 0.0
```

- **In place.** A codemod rewrites exactly the affected constructs. The
  rest of each file, including its formatting and comments, is left as it
  was.
- **Uncertain cases.** Where a codemod cannot be certain (a `.left` on a
  value of unknown type, say), it prints the position and the change to
  make, and does not guess.
- **The corpus.** Each codemod is tested against a corpus in
  `crates/rustnative/tests/codemod-corpus/`. Each case holds the input,
  the expected output, and the places that must be reported. Running a
  codemod twice changes nothing more.

A breaking change without a codemod is a release blocker, unless no
rewrite is possible. In that case the release notes say so, and `upgrade`
reports every affected position.

| Release | Change | Codemod |
|---|---|---|
| 0.1.0 | `EdgeInsets`' physical `left`/`right` became logical `start`/`end` (Milestone 39) | Rewrites struct literals and plain field access. Reports patterns and field access on values of unknown type. |

## MSRV

The minimum supported Rust version is stated in the workspace
`Cargo.toml` (`rust-version`) and checked on every change. Raising it
requires a minor release before 1.0 and a major release after, and the
release notes say so.

## Capability packages

A capability package declares the framework versions it works with
(`framework = ">=0.1, <0.2"`). Installing a package outside its range
fails with the reason (`PackageError::Framework`), and so does `rustnative
add`. A package never breaks at run time because of a framework upgrade.
