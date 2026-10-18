# L10 — CLI and tooling (`rustnative-cli`), and L11 — CI, budgets, and supply chain (deep read, iteration 1)

## L10 — CLI

Read:
- `cli.rs`: dispatch, `Fmt`, `Test --browser`, the `tui` env set
- `markup.rs`: `format_file`, `format_macro_calls`
- `web.rs`: `answer_static`, `serve_static_on`, `read_request_head`, `proxy`
- `lsp.rs`: position handling
- `dev.rs`: bind
- `bench.rs`: the env block
- `set_var` sites across the crate

| ID | Sev | Finding | Evidence |
|---|---|---|---|
| L10-F1 | **P1 (data loss)** | **`rustnative fmt` deletes the user's comments and rewrites files non-atomically.** `Command::Fmt` → `markup::format_file` → `format_markup`, which drops comments inside markup (L2 ✅). The result is written back with `std::fs::write`, not temp-then-rename. The comparison input is normalised `\r\n` → `\n`, so every CRLF file on Windows is "unformatted" and gets rewritten as LF. An error part-way through a multi-file run leaves some files rewritten. There is no "refuse to format when the formatter would drop tokens" guard. | `rustnative/src/cli.rs:671-700`; `markup.rs:245` |
| L10-F2 | P2 | **The LSP measures columns in Unicode scalars, not UTF-16 code units.** `position` takes `character` and converts with `chars().take(character)`, and the server never negotiates `positionEncoding`. LSP's default is UTF-16, so on any line with an astral character (emoji, many CJK extension characters) completions, hovers, and diagnostics land at the wrong column. The LSP is a long-lived process that parses with `syn`/`proc-macro2` on every change, so L2's unbounded proc-macro2 source-map growth becomes a memory leak for the length of an editor session. | `lsp.rs:150-175, 548-553` |
| L10-F3 | P3 | `serve_static_on` serves one connection at a time (one slow client stalls the preview server). Paths are not percent-decoded, so an exported file whose name contains a space or non-ASCII never serves; traversal is correctly refused. `bench` sets four `CARGO_PROFILE_DEV_*` variables process-wide with `unsafe set_var` (documented single-threaded). `cli` sets `RUSTNATIVE_BROWSER_REQUIRED` and `RUSTNATIVE_TARGET` the same way. | `web.rs:311-341, 397-411`; `bench.rs:450`; `cli.rs:553, 888` |
| L10-F5 | P3 (security) | **The PFX password reaches `signtool` as a command-line argument** (`/p <password>`, `sign.rs:50-54`). The module says "`rustnative` never holds a password", and it reads the password from the variable `--password-env` names, but the argument list puts it in the process table, where any same-user process (and logging such as Sysmon or CI process audits) can read it. The fix is `signtool`'s certificate-store or `/csp`/`/kc` modes, or a short-lived PFX re-export. Android keystore passwords stay in environment variables (`store_password_env`). | `rustnative/src/package/sign.rs:50-54, 92` |
| L10-F4 | info (positive) | Dev, preview, and emulator servers bind loopback by default. The static server refuses `..`, `\`, and `:`. Request heads are capped at 64 KiB. The dev error page escapes. `rustnative test --browser` makes the browser suite mandatory, but CI does not use it (L4-F13). | as cited |

### Indicators added (L10)

- **L10.1 Tooling never loses user text** (F1): a formatter round-trip property: for every `.rs`/`.rsx` in a corpus (the repo, plus a generator of markup with comments in every position), the multiset of comment and string tokens is unchanged after `fmt`, and the output re-parses to an equal `Markup`. Writes go through temp + rename, and the input's line endings are preserved.
- **L10.2 LSP protocol fidelity** (F2): `positionEncoding` is negotiated (`utf-8` when offered, else UTF-16 with conversion). Test lines contain astral, combining, and CJK characters, and every request is checked against a reference column mapper. A 10 000-edit session keeps RSS bounded (asserted).

## L11 — CI, budgets, supply chain

Read: `.github/workflows/ci.yml` (all jobs), `deny.toml`, `rust-toolchain.toml`, the workspace `Cargo.toml` lints and `rust-version`, the `budgets/` file list, and `.gitignore` (the stackdumps are untracked).

| ID | Sev | Finding | Evidence |
|---|---|---|---|
| L11-F1 | **P1 (instrument gap)** | **No dynamic-analysis job.** There is no Miri, no AddressSanitizer or ThreadSanitizer, no fuzzing (`cargo-fuzz`/libFuzzer appear nowhere), and no coverage measurement. Yet the riskiest code is exactly what those tools catch: Win32, GTK, JNI, and C-ABI `unsafe` (the interop `OwnerSlot`'s `unsafe impl Send`, `HandleTable`), hand-written parsers fed untrusted bytes (TUI `decode`, i18n catalogs, `.ril` IDL, `stored_entries` zip, the CSS and markup parsers, update manifests), and the cross-thread scheduler (L3-F25 waker under lock). Every parser finding in L1, L2, L5, and L6 would have come from a fuzzer. | `.github/workflows/ci.yml` (`grep` for miri/fuzz/sanitizer/llvm-cov: none) |
| L11-F2 | P2 (instrument) | **Skip-if-absent instruments are never required in CI**: the browser (`RUSTNATIVE_BROWSER_REQUIRED`), Node for the translator, and the Android emulator. The jobs pass when the tool is missing. Property tests run at fixed small case counts (`with_cases(64)` and similar), and no scheduled deep-run job raises them. | L4-F13, L7-F4; `ci.yml` |
| L11-F3 | P2 (reproducibility) | `rust-toolchain.toml` says `channel = "stable"` (unpinned), and CI uses `dtolnay/rust-toolchain@stable`. A new stable release can change lints and break `-D warnings` builds with no code change; this has already happened (memory: "CI runs newer Rust than local"). The MSRV job guards the floor (1.85), but nothing guards the ceiling. | `rust-toolchain.toml`; `ci.yml` |
| L11-F4 | P3 (supply chain) | Third-party actions are pinned by mutable tags (`actions/checkout@v4`, `Swatinem/rust-cache@v2`, `taiki-e/install-action@cargo-deny`, `dtolnay/rust-toolchain@stable`), not commit SHAs. There is no release workflow: packages, signing, SBOM, and provenance are produced by hand, so there is no reproducible-build check. `deny.toml` sets `multiple-versions = "warn"`. | `ci.yml`; `deny.toml` |
| L11-F5 | P3 (coverage) | `windows-cross-wine` is disabled. macOS runs `cargo check` only (TUI tests "owed to a Mac"). Real-cursor hover and two Android signing tests are `--skip`ped on CI. These are documented, but no ledger tracks them as debt with an owner and a date. | `ci.yml` |
| L11-F6 | info (positive) | `-D warnings` applies to rustc and rustdoc. Clippy runs with `--all-features` on Linux and Windows. An MSRV job exists. `cargo deny` (licenses, sources, yanked) and `cargo audit` run. Budgets per target are checked by `rustnative bench --check`, with reports uploaded. A one-core headless run catches scheduling assumptions. TUI runs under tmux and ssh. Android runs on an emulator. Windows interactive-desktop tests run. | `ci.yml` |

### Indicators added (L11)

- **L11.1 Dynamic analysis is a gate** (F1): Miri on `rustnative-core`, `-interop`, `-drawlist`, `-update`, and `-types` tests; ASan and TSan nightly jobs on Linux; `cargo-fuzz` targets for every parser of untrusted input (TUI decoder, i18n catalog, IDL, zip `stored_entries`, CSS/markup/style parsers, update envelope, Live `ClientFrame`, HTTP request head), with 10 minutes per target per night and corpora committed. A crash is a P1 bug.
- **L11.2 Required instruments** (F2): every harness that can skip has a `*_REQUIRED` variable, and CI sets all of them. A nightly job runs property tests at 100× the PR case count.
- **L11.3 Pinned toolchain plus a canary** (F3): `rust-toolchain.toml` pins an exact version. A scheduled job builds on the latest stable and opens an issue when it breaks.
- **L11.4 Supply-chain pinning** (F4): actions pinned by SHA (Dependabot keeps them current), and a release workflow that builds, signs, and attaches an SBOM and SLSA provenance.

## Iteration 2 (session 3): the rest of the CLI

Read in full: `templates/kits/{auth,admin,commerce}.rs` (code `rustnative generate kit` writes into applications) and `emulate/mod.rs`. Risk-scanned (searched for `Command::new`, arguments, secrets, environment variables, writes, path joins, and escaping; marked `skimmed`): `package/{android,msix,linux,mod}.rs` and the rest of the CLI (`generate`, `project/*`, `toolchain/*`, `doctor`, `pgo`, `upgrade`, `config`, `deploy`, `emulate/{lambda,wagi}`, and the others). The CLI downloads nothing (no network fetch in `toolchain`, `doctor`, or `upgrade`), and keystore passwords travel by environment-variable *name*.

| ID | Sev | Finding | Evidence |
|---|---|---|---|
| L10-F6 | **P1 (security)** | **The generated sign-up makes the first account to register an administrator.** `templates/kits/auth.rs` inserts with `admin = NOT EXISTS (SELECT 1 FROM accounts)`, and the admin kit grants `/admin` (create, read, update, and delete on every table, `accounts.password_hash` included) to `account.admin`. A freshly deployed application therefore belongs to whoever reaches `/auth/sign-up` first, typically a scanner watching new hosts, and nothing in the kit warns about it or offers a bootstrap token. The kit's other gaps, each from the same template: sign-in answers an unknown name immediately and a known one after Argon2 (timing enumeration); sign-in sets the principal without `Session::renew` (L9-F5); `sign_out` only clears the sealed cookie, so a copied cookie stays valid until expiry (no revocation); `admin` is read from the session's copy of `Account`, so demoting someone takes effect only after 12 hours; and every database error at sign-up is reported as "That name is taken". | `crates/rustnative/templates/kits/auth.rs` `sign_up`, `sign_in`, `sign_out`; `admin.rs` `Admins` |
| L10-F7 | P2 (security) | **The commerce kit's routes are keyed by a buyer named in the path and are public.** `/store/:buyer/checkout` and `/store/:buyer/owned` take the buyer from the URL with `.public()`, so anyone can list anyone's entitlements and purchase in anyone's name. It runs on `FakeStore`, but the kit is presented as a working feature to build on (U24: the identity must come from the principal). | `templates/kits/commerce.rs` `routes`, `checkout`, `owned` |
| L10-F8 | P3 | **CLI details.** `package/linux.rs` `desktop_entry` writes `display_name`, `description`, and `name` into the `.desktop` file unescaped (a newline adds keys, including a second `Exec=`) and leaves `Exec=` unquoted (U26). `RUSTNATIVE_ANDROID_APPLICATION_ID` silently changes a release build's package id (U27). `generate` rewrites the user's `src/lib.rs` with `fs::write` (U25, the L10-F1 shape). The emulators allocate whatever `Content-Length` declares, up to 64 MiB. | `package/linux.rs:212-237`; `package/android.rs:161-168`; `generate.rs:355`; `emulate/mod.rs` `read_request` |
