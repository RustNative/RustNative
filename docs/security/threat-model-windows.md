# Threat model: Windows applications

`PLAN.md` Milestone 51. What a RustNative application on Windows exposes,
who might attack it, and what stops them. Each mitigation names the code
or test that holds it.

## Assets

- The person's data, held in the application's state store and files.
- Their credentials and tokens (`SecureStorage`, Milestone 57).
- The integrity of the application itself: its binary, its updates, and
  the code it loads.

## Entry points and mitigations

| Entry point | Threat | Mitigation |
|---|---|---|
| Deep links (`url-schemes`) and a second launch's arguments | A crafted link drives the application | Links arrive as `Event::OpenUrl`; the application decides; nothing runs from a link by itself (Milestone 30) |
| Files the person opens | A malformed file exploits a parser | Parse untrusted formats in an `IsolatedWorker`: low integrity, a job with a memory cap and no UI access, killed with its owner (`native::isolated`, tested) |
| Network responses | A hostile server or a network attacker | `WinHttp` uses the system trust store; certificate pinning per host (`CertificatePins`, Milestone 47) |
| Updates | A forged update | Ed25519-signed manifests with the key pinned at build; digest checked before anything switches; side-by-side install with rollback (`update`, tested) |
| Third-party capability packages | A package reaching beyond its purpose | Services only through `ScopedServices` and the package's declared grants; HTTP origins and file paths checked at each call (`NotGranted`, tested) |
| Foreign native objects and escape hatches | Code outside the framework's model | Adoption is explicit (Milestone 40) and scoped to a node; nothing is ambient |
| Inspection (`RUSTNATIVE_INSPECT`) | A local process reading or editing state | Off unless the variable is set; loopback only; per-process token, compared in constant time and published in a file only its user can read (Milestone 44; see *Files that hold secrets* below) |
| The development agent (`rustnative dev-agent`) | Another local user swapping the executable it runs, or reading it | Writes each build to a fresh, unpredictable name in a private per-user folder, created without following links, and runs that path (see *Files that hold secrets* below) |
| Crash reports | Personal data in a dump | Reports stay on the machine; sending them is the application's choice, under telemetry consent |
| Telemetry | Silent collection | Off by default; consent recorded; the OTLP exporter sends nothing without it (`docs/telemetry-policy.md`) |

## Files that hold secrets

Two files carry something another local user must not touch: the inspection
server's endpoint file holds its token, and the development agent writes the
executables it is sent and then runs them. Neither lives in the shared
temporary directory. On Linux `/tmp` is shared by every user: a file written
there at the default umask is world-readable, and another user can create the
directory first or plant a symbolic link at the name about to be written. On
Windows `%TEMP%` is already per-user, but the same code runs on both, so it
takes the same care everywhere.

| Platform | Base directory |
|---|---|
| Linux and other Unix | `$XDG_RUNTIME_DIR` (per-user, `0700`, cleared at logout), else `$XDG_CACHE_HOME`, else `~/.cache` |
| macOS | `$TMPDIR`, the per-user `/var/folders/…` directory |
| Windows | `%LOCALAPPDATA%`, whose ACL admits only the user, SYSTEM, and administrators |

If none of these is set, the shared temporary directory is used with the
user's id in the folder's name, and the checks below are what keep another
user out.

| File | Where | How it is created |
|---|---|---|
| Inspection endpoint (address and token) | `<base>/rustnative-inspect/<pid>.json` | Directory `0700`; file `0600`, `O_CREAT \| O_EXCL \| O_NOFOLLOW`; removed when the server stops |
| A build sent to the development agent | `<base>/rustnative-dev-agent/<random>-<name>` | Directory `0700`; file `0700`, `O_CREAT \| O_EXCL \| O_NOFOLLOW`, under a 128-bit random prefix; removed when the next build replaces it or the agent stops |

Before a directory is written to or read from, it is checked: it must be
a directory and not a link to one, and on Unix it must belong to the current
user and grant nothing to group or others. One that fails is refused with its
path and the reason, not repaired, because a directory someone else could
write to may already hold their files. A file is always created, never opened,
so nothing already at its name (a stale file, a link) is written through or
run. `rustnative inspect` and `rustnative dev` read endpoints only from a
directory that passes the same check.

Tokens are compared with `rustnative_core::secret::constant_time_eq`, backed
by the `subtle` crate. The inspection server, the development agent, and
`rustnative-server` (CSRF tokens, bearer tokens, OAuth state, passkey
challenges) all use it.

Held by `rustnative_core::secret` and its tests (owner-only modes, a refused
open directory, a link not followed), `inspect::server`'s test that the
published endpoint is `0600` in a `0700` directory, and `rustnative`'s
`tests/dev.rs`.

## Out of scope

- A local administrator, or malware already running at the person's
  integrity level: the operating system's boundary, not the application's.
- Physical access to an unlocked session.
