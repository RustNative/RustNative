# Budget files

`PLAN.md` Milestone 42. There is one file per shipped target:
`windows.toml`, `linux.toml` (measured on a Linux desktop session —
`rustnative bench --target linux`; CI's runners have no compositor or GPU to
measure GTK on, so it is run on the reference machine), `headless.toml`,
`web.toml` (measured by
`examples/web-bench` in headless Edge on a throttled profile), and
`serverless.toml` (measured by `rustnative bench` itself, running
`examples/web-notes` under its emulators). `rustnative bench --target <target>`
measures the target and compares the results with its file. With `--check`,
the command fails when a measurement is over budget, just as a failing test
fails the build. CI runs it on every push (`.github/workflows/ci.yml`, job
`budgets`).

```toml
target = "windows"

[budget]
cold_start_ms = { max = 450, tolerance = 0.75 }
build_time_clean_s = { max = 300, tolerance = 0.5, optional = true }
```

Each key has these fields:

- `max`: the budget.
- `tolerance`: the declared noise of that measurement, as a fraction of `max`.
- `optional`: `true` marks a key that is measured only on request.

A measurement fails when it is above `max × (1 + tolerance)`.

The run also fails in two other cases:

- **Unbudgeted.** A measurement has no key in the file, so the file cannot
  fall behind what is measured.
- **Unmeasured.** A key is not measured and not `optional`.

Each run writes `target/budget-report.json`.

## Keys

All scenarios run from `examples/bench-app`, built in release. A key's value
is the median over the scenario's repeated runs. `--low-end` pins the
scenarios to one core (on Windows through `SetProcessAffinityMask`; CI runs
the headless ones under `taskset -c 0`).

### Startup (Windows)

The startup phases come from the `rustnative_core::perf` phase model
(`C62`). Each is measured from the process's creation time
(`GetProcessTimes`). The screen is a form: a heading, a counter, and forty
labelled text fields, 125 native controls in all.

| Key | Meaning |
|---|---|
| `runtime_ready_ms` | the backend is initialized and can create windows |
| `first_frame_ms` | the first window exists with its tree realized |
| `first_content_ms` | the first window's first paint |
| `interactive_ms`, `cold_start_ms` | the first idle after the first paint: the application answers input at once |
| `resident_memory_mb` | the working set once interactive |
| `artifact_size_kb` | `bench-app.exe`, release |

### Frames and input (Windows)

| Key | Meaning |
|---|---|
| `frame_time_p50_ms`, `frame_time_p99_ms`, `frame_time_max_ms` | intervals between animation frames while a 900 ms size transition runs repeatedly, over 3.2 s |
| `input_latency_ms`, `input_latency_max_ms` | a `BM_CLICK` posted to the real button window, timed until the change it causes is realized on the native controls (median and worst of 31) |
| `filter_input_latency_ms`, `filter_input_latency_max_ms`, `filter_results_ms` | keystrokes posted to the edit control of `examples/filter-demo` (200 000 rows), each timed until its echo is realized while the filter runs behind it (median and worst of 12), and the time from the last keystroke until its result is shown |

### Headless

| Key | Meaning |
|---|---|
| `launch_ms` | launching the bench screen on the headless backend and settling it |
| `input_latency_ms` | a click through hit-testing and the input path, to the settled change |
| `filter_input_latency_ms` | a keystroke into `examples/filter-demo`'s filter over 200 000 rows: the event and the render it causes (median of 12) |

### Shared core and compile steps (both targets)

| Key | Meaning |
|---|---|
| `tree_diff_us_1k_nodes` | diffing two thousand-label trees that differ in ten |
| `layout_us_1k_nodes` | laying out a thousand labels |
| `render_us_1k_nodes` | an event re-rendering a component of a thousand rows |
| `rsx_compile_ms_per_kloc` | lowering about a thousand lines of `.rsx` with `rustnative_markup::compile`: the markup's build-time cost |
| `class_resolve_us` | resolving a nine-class string against the default vocabulary: the style spelling's build-time cost |

### Build time (`--build-times`)

| Key | Meaning |
|---|---|
| `build_time_clean_s` | `cargo build -p hello-label` in an empty target folder |
| `build_time_incremental_s` | the same after the application's main file is saved again |

## Owed with their backends

These keys are declared by the plan but have no file yet. Each belongs to a
target that does not exist yet:

- **Web**: per-route client payload, largest content paint, interaction
  responsiveness, and layout shift (`C42-4`).
- **Edge**: per-request cold start and CPU time per route.
- **Server**: container image size.
- **Embedded**: RAM, flash, and boot-to-first-frame (`C83-2`).
- **Devices**: the low-end device matrix.

## Changing a budget

A budget is changed in the same commit as the change that moves it. The
commit says why. The measured values are recorded next to each key, as
comments.

## Web keys (`web.toml`)

| Key | Meaning |
|---|---|
| `route_script_kb` | The JavaScript the home route (one island) loads: the runtime and its island's module. |
| `unrelated_routes_script_growth_kb` | What ten unrelated routes, each with a heavy module, add to it (0: each route loads only its own modules). |
| `startup_ms` | Navigation to the islands being ready, on CPU x4, 150 ms latency, 1.6 Mb/s. |
| `lcp_ms` | Largest contentful paint on that profile. |
| `cls` | Cumulative layout shift (framework-controlled content reserves its size). |
| `inp_ms` | A click's interaction to next paint on that profile. |


## Serverless keys (`serverless.toml`)

Measured by a release build of `rustnative` (the emulators run in its
process): `examples/web-notes`' sign-in page, as a function behind `serve
lambda` and as a `wasm32-wasip1` module behind `serve wagi`.

| Key | Meaning |
|---|---|
| `lambda_artifact_kb` | The release function binary. |
| `lambda_cold_start_ms` | Process start, the runtime-API handshake, and the first response. |
| `lambda_warm_ms` | A warm invocation, median of five. |
| `lambda_memory_mb` | The function's peak working set (Windows). |
| `edge_artifact_kb` | The release edge module. |
| `edge_cold_start_ms` | Compiling the module in the emulator's interpreter, and the first request. |
| `edge_warm_ms` | A request on a fresh instance of the compiled module, median of five. |
| `edge_memory_mb` | The module's linear memory after a request. |
| `edge_fuel_sign_in_mfuel` | The sign-in route's CPU in millions of fuel units, warm: a per-route budget, which `serve wagi --route-fuel` enforces at run time. |
