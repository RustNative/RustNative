//! `rustnative bench`: the budget harness (`PLAN.md` Milestone 42).
//!
//! It measures the framework's budget scenarios for a target and compares
//! each measurement with `budgets/<target>.toml`:
//!
//! - the scenarios `examples/bench-app` runs, built in release;
//!   (including the markup and style compile steps, against the same parser
//!   and vocabulary the build uses);
//! - the artifact's size;
//! - with `--build-times`, a clean and an incremental build of
//!   `examples/hello-label`, the development loop's restart on it, and the
//!   first-run target (new project to running application).
//!
//! It writes `target/budget-report.json`. With `--check` it fails on any
//! measurement over its budget, beyond the key's declared noise tolerance,
//! and on any measurement the budget file does not declare, so the file
//! cannot fall behind what is measured.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};

/// The targets with a budget file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum BenchTarget {
    /// The Windows backend.
    Windows,
    /// The Linux backend (GTK 4), measured on a Linux host.
    Linux,
    /// The headless reference backend.
    Headless,
    /// The web backend, in headless Edge (`examples/web-bench`).
    Web,
    /// The serverless shapes (`examples/web-notes` as a function and as an
    /// edge module), under the emulators.
    Serverless,
    /// The Android backend, on the attached device (`examples/hello-label`).
    Android,
}

impl BenchTarget {
    const fn name(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Linux => "linux",
            Self::Headless => "headless",
            Self::Web => "web",
            Self::Serverless => "serverless",
            Self::Android => "android",
        }
    }

    /// The package whose binary runs this target's scenarios.
    const fn package(self) -> &'static str {
        match self {
            Self::Windows | Self::Linux | Self::Headless => "bench-app",
            Self::Web => "web-bench",
            Self::Serverless => "web-notes",
            Self::Android => "hello-label",
        }
    }

    /// The bench-app scenarios this target runs, with how many times each.
    const fn scenarios(self) -> &'static [(&'static str, usize)] {
        match self {
            Self::Windows | Self::Linux => &[
                ("startup", 5),
                ("interaction", 3),
                ("filter", 3),
                ("animation", 1),
                ("core", 1),
                ("compile", 1),
            ],
            Self::Headless => &[("headless", 3), ("core", 1), ("compile", 1)],
            Self::Web => &[("web", 3)],
            // Measured in this process, under the emulators; on the device.
            Self::Serverless | Self::Android => &[],
        }
    }
}

/// One budgeted key.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Budget {
    /// The most it may measure.
    pub max: f64,
    /// The fraction above `max` a measurement may reach before failing: the
    /// declared noise of this measurement on a shared runner.
    #[serde(default)]
    pub tolerance: f64,
    /// Whether it is measured only on request (build times).
    #[serde(default)]
    pub optional: bool,
    /// Whether it times frames a display paces (frame intervals): a
    /// measurement only a machine with a real display can make. A hosted CI
    /// runner's virtual display paces nothing, so there it is reported but
    /// not enforced (`--unpaced-display`).
    #[serde(default)]
    pub display_paced: bool,
}

/// A budget file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BudgetFile {
    /// The target it is for.
    pub target: String,
    /// Each key's budget.
    pub budget: BTreeMap<String, Budget>,
}

/// One key's verdict.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Verdict {
    /// What was measured.
    pub measured: f64,
    /// Its budget.
    pub max: f64,
    /// The most it may measure with the tolerance.
    pub limit: f64,
    /// Whether it is within the limit.
    pub within: bool,
    /// Whether it is reported but not enforced: a display-paced key on a
    /// machine whose display paces nothing.
    pub advisory: bool,
}

/// What a bench run found.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    /// The target.
    pub target: String,
    /// Whether the low-end profile was used.
    pub low_end: bool,
    /// Whether the machine was declared to have no display that paces
    /// frames, so display-paced keys were advisory.
    pub unpaced_display: bool,
    /// Each budgeted key's verdict.
    pub verdicts: BTreeMap<String, Verdict>,
    /// Measurements the budget file does not declare.
    pub unbudgeted: Vec<String>,
    /// Budgeted keys that were not measured (and are not optional).
    pub unmeasured: Vec<String>,
}

impl Report {
    /// Whether everything is within budget and accounted for.
    #[must_use]
    pub fn passes(&self) -> bool {
        self.verdicts.values().all(|verdict| verdict.within || verdict.advisory)
            && self.unbudgeted.is_empty()
            && self.unmeasured.is_empty()
    }
}

/// Compares `measured` with `budgets`; with `unpaced_display`, a
/// display-paced key is reported but not enforced.
#[must_use]
pub fn judge(
    budgets: &BudgetFile,
    measured: &BTreeMap<String, f64>,
    low_end: bool,
    unpaced_display: bool,
) -> Report {
    let mut verdicts = BTreeMap::new();
    let mut unbudgeted = Vec::new();
    for (key, value) in measured {
        match budgets.budget.get(key) {
            Some(budget) => {
                let limit = budget.max * (1.0 + budget.tolerance);
                verdicts.insert(
                    key.clone(),
                    Verdict {
                        measured: *value,
                        max: budget.max,
                        limit,
                        within: *value <= limit,
                        advisory: unpaced_display && budget.display_paced,
                    },
                );
            }
            None => unbudgeted.push(key.clone()),
        }
    }
    let unmeasured = budgets
        .budget
        .iter()
        .filter(|(key, budget)| !budget.optional && !measured.contains_key(*key))
        .map(|(key, _)| key.clone())
        .collect();
    Report {
        target: budgets.target.clone(),
        low_end,
        unpaced_display,
        verdicts,
        unbudgeted,
        unmeasured,
    }
}

/// The repository root: the nearest ancestor holding `budgets/`.
fn root(here: &Path) -> Result<PathBuf> {
    here.ancestors().find(|dir| dir.join("budgets").is_dir()).map(Path::to_path_buf).ok_or_else(
        || {
            Error::Usage(format!(
                "no `budgets/` folder above {} — `rustnative bench` measures the framework's own \
             budgets and runs in its repository",
                here.display()
            ))
        },
    )
}

fn io(what: impl Into<String>) -> impl FnOnce(std::io::Error) -> Error {
    let what = what.into();
    move |cause| Error::Io { what, cause }
}

fn cargo() -> Command {
    Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
}

fn checked(tool: &'static str, mut command: Command) -> Result<std::process::Output> {
    let output = command.output().map_err(|cause| Error::ToolMissing {
        tool,
        hint: "install the Rust toolchain from https://rustup.rs".into(),
        cause: Some(cause.to_string()),
    })?;
    if output.status.success() {
        return Ok(output);
    }
    // What the tool said, so the failure is actionable.
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    Err(Error::ToolFailed { tool, code: output.status.code() })
}

/// The median of each key over several runs.
fn medians(runs: &[BTreeMap<String, f64>]) -> BTreeMap<String, f64> {
    let mut keys: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for run in runs {
        for (key, value) in run {
            keys.entry(key.clone()).or_default().push(*value);
        }
    }
    keys.into_iter()
        .map(|(key, mut values)| {
            values.sort_by(f64::total_cmp);
            let middle = values[values.len() / 2];
            (key, middle)
        })
        .collect()
}

fn numbers(value: &Value) -> BTreeMap<String, f64> {
    value
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(key, value)| Some((key.clone(), value.as_f64()?)))
        .collect()
}

/// A clean and an incremental build of `examples/hello-label`, in its own
/// target folder so nothing else's cache helps.
fn build_times(root: &Path) -> Result<BTreeMap<String, f64>> {
    let target = root.join("target").join("bench-build");
    let _ = std::fs::remove_dir_all(&target);
    let build = || {
        let mut command = cargo();
        command.current_dir(root).args(["build", "-p", "hello-label", "--target-dir"]).arg(&target);
        let started = Instant::now();
        checked("cargo", command).map(|_| started.elapsed().as_secs_f64())
    };
    let clean = build()?;
    // An edit to the application: its main file is rewritten unchanged,
    // which is what an editor's save does to the timestamp.
    let main = root.join("examples/hello-label/src/main.rs");
    let text = std::fs::read(&main).map_err(io("read hello-label's main.rs"))?;
    std::fs::write(&main, text).map_err(io("touch hello-label's main.rs"))?;
    let incremental = build()?;
    Ok(BTreeMap::from([
        ("build_time_clean_s".to_owned(), clean),
        ("build_time_incremental_s".to_owned(), incremental),
        ("dev_loop_restart_ms".to_owned(), dev_loop(root)?),
        ("first_run_s".to_owned(), first_run(root, &target)?),
    ]))
}

/// The development loop's restart: `rustnative dev windows --once` on
/// `examples/hello-label` — a save, the rebuild, the restart, and the
/// state restored (Milestone 43).
fn dev_loop(root: &Path) -> Result<f64> {
    let exe = std::env::current_exe().map_err(io("find rustnative itself"))?;
    let mut command = Command::new(exe);
    command.current_dir(root.join("examples/hello-label")).args(["dev", host_platform(), "--once"]);
    let output = checked("rustnative dev", command)?;
    let text = String::from_utf8_lossy(&output.stdout);
    let restart: crate::dev::Restart = text
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str(line).ok())
        .ok_or_else(|| Error::Usage("`rustnative dev --once` reported no restart".into()))?;
    Ok(restart.total_ms)
}

/// The first-run target (Milestone 43): the three documented commands —
/// `rustnative new`, `cd`, `rustnative run windows` — timed from creation
/// to the application interactive. The dependencies are already compiled
/// in `target` (the clean build above), which excludes their first
/// download and build, as the target states.
fn first_run(root: &Path, target: &Path) -> Result<f64> {
    let exe = std::env::current_exe().map_err(io("find rustnative itself"))?;
    // Outside the framework's workspace, as a developer's project is.
    let parent = std::env::temp_dir().join("rustnative-bench-first-run");
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir_all(&parent).map_err(io("create the first-run folder"))?;
    let started = Instant::now();
    let mut new = Command::new(&exe);
    new.current_dir(&parent)
        .args(["new", "first-run", "--syntax", "builder", "--framework-path"])
        .arg(root);
    checked("rustnative new", new)?;
    let mut run = Command::new(&exe);
    run.current_dir(parent.join("first-run"))
        .args(["run", host_platform()])
        .env("CARGO_TARGET_DIR", target)
        .env("RUSTNATIVE_EXIT_AT", "interactive");
    checked("rustnative run", run)?;
    Ok(started.elapsed().as_secs_f64())
}

/// The desktop platform this machine builds for.
const fn host_platform() -> &'static str {
    if cfg!(target_os = "linux") { "linux" } else { "windows" }
}

/// How a run measures and judges.
#[derive(Debug, Clone, Copy, Default)]
#[allow(clippy::struct_excessive_bools, reason = "independent command-line switches")]
pub struct Options {
    /// Fail on a measurement over budget or unaccounted for.
    pub check: bool,
    /// Pin the scenarios to one core: the low-end reference profile.
    pub low_end: bool,
    /// No display paces frames here: display-paced keys are advisory.
    pub unpaced_display: bool,
    /// Also time builds, the development loop, and the first run.
    pub build: bool,
}

/// Runs the harness.
pub fn run(here: &Path, target: BenchTarget, options: Options) -> Result<()> {
    let Options { check, low_end, unpaced_display, build } = options;
    let root = root(here)?;
    let budget_path = root.join("budgets").join(format!("{}.toml", target.name()));
    let text = std::fs::read_to_string(&budget_path)
        .map_err(io(format!("read {}", budget_path.display())))?;
    let budgets: BudgetFile = toml::from_str(&text)
        .map_err(|error| Error::Usage(format!("{}: {error}", budget_path.display())))?;

    if target == BenchTarget::Serverless {
        let measured = serverless(&root)?;
        return finish(&root, &budgets, &budget_path, &measured, (low_end, unpaced_display), check);
    }
    if target == BenchTarget::Android {
        let measured = android(&root)?;
        return finish(&root, &budgets, &budget_path, &measured, (low_end, unpaced_display), check);
    }

    println!("bench: building the scenarios (release)");
    let mut command = cargo();
    command.current_dir(&root).args(["build", "--release", "-p", target.package()]);
    checked("cargo", command)?;
    // `CARGO_TARGET_DIR` moves the build, as on a Linux host sharing a
    // checkout with Windows.
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| root.join("target"), std::path::PathBuf::from);
    let exe = target_dir.join("release").join(format!(
        "{}{}",
        target.package(),
        std::env::consts::EXE_SUFFIX
    ));

    let mut measured = BTreeMap::new();
    for (scenario, runs) in target.scenarios() {
        println!("bench: {scenario} ×{runs}");
        let mut results = Vec::new();
        for _ in 0..*runs {
            let mut command = Command::new(&exe);
            command.args(["--scenario", scenario]);
            if low_end {
                command.arg("--low-end");
            }
            if target == BenchTarget::Linux {
                // A compositor releases a closed client's surfaces (and a
                // virtual GPU its buffers) after the process exits; a launch
                // in that window measures the release, not the application.
                std::thread::sleep(std::time::Duration::from_secs(3));
            }
            let output = checked("bench-app", command)?;
            let line = String::from_utf8_lossy(&output.stdout);
            let value: Value = serde_json::from_str(line.trim()).map_err(|error| {
                Error::Usage(format!("bench-app {scenario} printed no measurement: {error}"))
            })?;
            results.push(numbers(&value));
        }
        measured.extend(medians(&results));
    }
    measured.remove("frames");
    if matches!(target, BenchTarget::Windows | BenchTarget::Linux) {
        let size = std::fs::metadata(&exe).map_err(io("read bench-app's size"))?.len();
        #[allow(clippy::cast_precision_loss, reason = "kilobytes")]
        measured.insert("artifact_size_kb".to_owned(), size as f64 / 1024.0);
    }
    if build {
        println!("bench: clean and incremental builds");
        measured.extend(build_times(&root)?);
    }

    finish(&root, &budgets, &budget_path, &measured, (low_end, unpaced_display), check)
}

/// The Android budgets, on the attached device: `examples/hello-label`
/// built optimized for the device's ABI (debug-signed, so it installs),
/// launched cold five times — `am start -W`'s total time, and the startup
/// trace the backend logs (`rustnative startup: {…}`) — then its memory
/// (`dumpsys meminfo`'s total PSS) and the APK's size.
fn android(root: &Path) -> Result<BTreeMap<String, f64>> {
    use crate::package::android;
    let project = root.join("examples").join("hello-label");
    let config = crate::config::Config::load(&project)?;
    let toolchain = android::toolchain()?;
    let adb = |arguments: &[&str]| android::adb(&toolchain, arguments);
    let abi = adb(&["shell", "getprop", "ro.product.cpu.abi"])?.trim().to_owned();
    println!("bench: building hello-label for {abi} (optimized)");
    // Optimized, but signed with the debug key so it installs: the release
    // profile's settings on the debug build.
    // SAFETY: single-threaded here; the variables reach the Cargo child.
    unsafe {
        std::env::set_var("CARGO_PROFILE_DEV_OPT_LEVEL", "3");
        std::env::set_var("CARGO_PROFILE_DEV_DEBUG", "0");
        std::env::set_var("CARGO_PROFILE_DEV_DEBUG_ASSERTIONS", "false");
        std::env::set_var("CARGO_PROFILE_DEV_OVERFLOW_CHECKS", "false");
    }
    let apk = android::build(
        &project,
        &config,
        &android::Options { abis: vec![abi], ..android::Options::default() },
    )?;
    android::install(&toolchain, &apk)?;
    let package = android::application_id(&config);
    let component = format!("{package}/dev.rustnative.android.RnActivity");
    let mut runs = Vec::new();
    for run in 0..5 {
        println!("bench: cold start {}/5", run + 1);
        adb(&["shell", "am", "force-stop", &package])?;
        std::thread::sleep(std::time::Duration::from_secs(2));
        adb(&["logcat", "-c"])?;
        let started = adb(&["shell", "am", "start", "-W", "-n", &component])?;
        let total = started
            .lines()
            .find_map(|line| line.trim().strip_prefix("TotalTime:"))
            .and_then(|value| value.trim().parse::<f64>().ok())
            .ok_or_else(|| {
                Error::Usage(format!(
                    "am start -W reported no total time:
{started}"
                ))
            })?;
        let mut measured = BTreeMap::from([("cold_start_ms".to_owned(), total)]);
        let deadline = Instant::now() + std::time::Duration::from_secs(20);
        loop {
            let log = adb(&["logcat", "-d", "-s", "RustNative"])?;
            if let Some(json) = log.lines().find_map(|line| {
                line.split_once("rustnative startup: ").map(|(_, json)| json.to_owned())
            }) {
                let value: Value = serde_json::from_str(json.trim())
                    .map_err(|error| Error::Usage(format!("the startup trace: {error}")))?;
                measured.extend(numbers(&value));
                break;
            }
            if Instant::now() > deadline {
                return Err(Error::Usage("hello-label logged no startup trace".to_owned()));
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        runs.push(measured);
    }
    let mut measured = medians(&runs);
    std::thread::sleep(std::time::Duration::from_secs(3));
    let memory = adb(&["shell", "dumpsys", "meminfo", &package])?;
    let pss = memory
        .lines()
        .find_map(|line| {
            let line = line.trim();
            line.strip_prefix("TOTAL PSS:")
                .or_else(|| line.strip_prefix("TOTAL:"))
                .or_else(|| line.strip_prefix("TOTAL"))
        })
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|kilobytes| kilobytes.parse::<f64>().ok());
    if let Some(kilobytes) = pss {
        measured.insert("resident_memory_mb".to_owned(), kilobytes / 1024.0);
    }
    measured.insert("artifact_size_kb".to_owned(), kilobytes(&apk)?);
    adb(&["shell", "am", "force-stop", &package])?;
    Ok(measured)
}

/// Judges `measured`, writes the report, and prints the verdicts.
fn finish(
    root: &Path,
    budgets: &BudgetFile,
    budget_path: &Path,
    measured: &BTreeMap<String, f64>,
    (low_end, unpaced_display): (bool, bool),
    check: bool,
) -> Result<()> {
    let report = judge(budgets, measured, low_end, unpaced_display);
    let report_path = root.join("target/budget-report.json");
    std::fs::write(&report_path, serde_json::to_vec_pretty(&report).unwrap_or_default())
        .map_err(io(format!("write {}", report_path.display())))?;
    for (key, verdict) in &report.verdicts {
        println!(
            "{} {key:<28} {:>10.3}  (budget {}, limit {:.3})",
            match (verdict.within, verdict.advisory) {
                (true, _) => "ok  ",
                (false, true) => "INFO",
                (false, false) => "OVER",
            },
            verdict.measured,
            verdict.max,
            verdict.limit
        );
    }
    if report.unpaced_display {
        println!("bench: --unpaced-display: display-paced keys are reported, not enforced (INFO)");
    }
    for key in &report.unbudgeted {
        println!("NEW  {key:<28} {:>10.3}  (not in {})", measured[key], budget_path.display());
    }
    for key in &report.unmeasured {
        println!("MISS {key:<28} (budgeted, not measured)");
    }
    println!("bench: report in {}", report_path.display());
    if check && !report.passes() {
        return Err(Error::Usage(format!(
            "over budget or unaccounted for (see {})",
            report_path.display()
        )));
    }
    Ok(())
}

/// One `GET path` against `port`: the time to the whole response, and its
/// headers.
fn timed_get(port: u16, path: &str) -> Result<(f64, Vec<(String, String)>)> {
    use std::io::{Read, Write};
    let started = Instant::now();
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port))
        .map_err(io(format!("connect to 127.0.0.1:{port}")))?;
    write!(stream, "GET {path} HTTP/1.1\r\nhost: localhost\r\nconnection: close\r\n\r\n")
        .map_err(io("send a request"))?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response).map_err(io("read a response"))?;
    let elapsed = started.elapsed().as_secs_f64() * 1000.0;
    let head = String::from_utf8_lossy(&response).into_owned();
    if !head.starts_with("HTTP/1.1 200") {
        return Err(Error::Usage(format!(
            "bench: GET {path} was not answered: {}",
            head.lines().next().unwrap_or_default()
        )));
    }
    let headers = head
        .split("\r\n\r\n")
        .next()
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    Ok((elapsed, headers))
}

fn kilobytes(path: &Path) -> Result<f64> {
    let size = std::fs::metadata(path).map_err(io(format!("read {}", path.display())))?.len();
    #[allow(clippy::cast_precision_loss, reason = "kilobytes")]
    Ok(size as f64 / 1024.0)
}

/// The largest the process named `name` has been, in megabytes (the peak
/// working set; Windows, the reference machine).
fn peak_memory_mb(name: &str) -> Option<f64> {
    let output = Command::new("powershell")
        .args(["-NoProfile", "-Command"])
        .arg(format!(
            "(Get-Process -Name '{name}' | Measure-Object PeakWorkingSet64 -Maximum).Maximum"
        ))
        .output()
        .ok()?;
    let bytes: f64 = String::from_utf8_lossy(&output.stdout).trim().parse().ok()?;
    Some(bytes / (1024.0 * 1024.0))
}

/// The serverless budgets' measurements (`W-SL-1`, `W-ED-2`): the notes
/// application's sign-in page, as a function under the Lambda emulator and
/// as an edge module under the WAGI emulator, from a cold start and warm.
fn serverless(root: &Path) -> Result<BTreeMap<String, f64>> {
    if cfg!(debug_assertions) {
        eprintln!(
            "bench: this `rustnative` is a debug build; the edge interpreter's timings will be slow"
        );
    }
    println!("bench: building the function and the edge module (release)");
    let mut function = cargo();
    function.current_dir(root).args([
        "build",
        "--release",
        "-p",
        "web-notes",
        "--bin",
        "web-notes-lambda",
    ]);
    checked("cargo", function)?;
    let mut module = cargo();
    module.current_dir(root).args([
        "build",
        "--release",
        "-p",
        "web-notes",
        "--bin",
        "web-notes-edge",
        "--target",
        "wasm32-wasip1",
        "--no-default-features",
    ]);
    checked("cargo", module)?;
    let binary = root
        .join("target/release")
        .join(format!("web-notes-lambda{}", std::env::consts::EXE_SUFFIX));
    let module_path = root.join("target/wasm32-wasip1/release/web-notes-edge.wasm");
    let mut measured = BTreeMap::from([
        ("lambda_artifact_kb".to_owned(), kilobytes(&binary)?),
        ("edge_artifact_kb".to_owned(), kilobytes(&module_path)?),
    ]);
    let median = |mut values: Vec<f64>| {
        values.sort_by(f64::total_cmp);
        values[values.len() / 2]
    };

    // The function: the first request starts the process (a cold start).
    println!("bench: the function, cold and warm");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(io("bind a port"))?;
    let port = listener.local_addr().map_err(io("read the port"))?.port();
    let repeats = 5;
    let emulator = std::thread::spawn(move || {
        let options = crate::emulate::lambda::LambdaOptions {
            // The cold request, the warm ones, and one that closes it after
            // the memory is read.
            requests: Some(2 + repeats),
            ..crate::emulate::lambda::LambdaOptions::default()
        };
        crate::emulate::lambda::serve_on(&listener, &binary, options)
    });
    measured.insert("lambda_cold_start_ms".to_owned(), timed_get(port, "/")?.0);
    let mut warm = Vec::new();
    for _ in 0..repeats {
        warm.push(timed_get(port, "/")?.0);
    }
    if let Some(memory) = peak_memory_mb("web-notes-lambda") {
        measured.insert("lambda_memory_mb".to_owned(), memory);
    }
    timed_get(port, "/")?;
    measured.insert("lambda_warm_ms".to_owned(), median(warm));
    let _ = emulator.join();

    // The edge module: compiled once per host, instantiated per request.
    println!("bench: the edge module, cold and warm");
    let started = Instant::now();
    let host = crate::emulate::wagi::Host::new(
        &module_path,
        crate::emulate::wagi::WagiOptions::default(),
    )?;
    let request = crate::emulate::HttpRequest {
        method: "GET".into(),
        target: "/".into(),
        ..crate::emulate::HttpRequest::default()
    };
    let first = host.answer(&request, None);
    measured.insert("edge_cold_start_ms".to_owned(), started.elapsed().as_secs_f64() * 1000.0);
    let header = |outcome: &crate::emulate::wagi::Outcome, name: &str| {
        outcome
            .headers
            .iter()
            .find(|(key, _)| key == name)
            .and_then(|(_, value)| value.parse::<f64>().ok())
            .unwrap_or(f64::MAX)
    };
    if first.status != 200 {
        return Err(Error::Usage(format!("bench: the edge module answered {}", first.status)));
    }
    measured.insert("edge_memory_mb".to_owned(), header(&first, "x-rn-memory") / (1024.0 * 1024.0));
    let mut warm = Vec::new();
    let mut last = first;
    for _ in 0..5 {
        let started = Instant::now();
        last = host.answer(&request, None);
        warm.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    measured.insert("edge_warm_ms".to_owned(), median(warm));
    // A route's CPU budget, in millions of fuel units: the sign-in page,
    // warm (the first request also pays to compile what it calls).
    measured.insert("edge_fuel_sign_in_mfuel".to_owned(), header(&last, "x-rn-fuel") / 1_000_000.0);
    Ok(measured)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_web_budget_fails_an_inflated_route() {
        let text = include_str!("../../../budgets/web.toml");
        let budgets: BudgetFile = toml::from_str(text).unwrap();
        let measured = |route: f64, growth: f64| {
            BTreeMap::from([
                ("route_script_kb".to_owned(), route),
                ("unrelated_routes_script_growth_kb".to_owned(), growth),
                ("startup_ms".to_owned(), 1800.0),
                ("lcp_ms".to_owned(), 900.0),
                ("cls".to_owned(), 0.0),
                ("inp_ms".to_owned(), 30.0),
            ])
        };
        assert!(judge(&budgets, &measured(122.7, 0.0), false, false).passes());
        // A module inflated past the route's budget fails the build.
        assert!(!judge(&budgets, &measured(200.0, 0.0), false, false).passes());
        // So does a route that grows because others were added.
        assert!(!judge(&budgets, &measured(122.7, 0.5), false, false).passes());
    }

    #[test]
    fn the_serverless_budget_fails_a_route_over_its_fuel() {
        let text = include_str!("../../../budgets/serverless.toml");
        let budgets: BudgetFile = toml::from_str(text).unwrap();
        let measured = |fuel: f64| {
            let mut measured: BTreeMap<String, f64> =
                budgets.budget.iter().map(|(key, budget)| (key.clone(), budget.max)).collect();
            measured.insert("edge_fuel_sign_in_mfuel".to_owned(), fuel);
            measured
        };
        assert!(judge(&budgets, &measured(11.7), false, false).passes());
        assert!(!judge(&budgets, &measured(20.0), false, false).passes());
    }

    fn budgets() -> BudgetFile {
        toml::from_str(
            r#"
            target = "test"
            [budget]
            cold_start_ms = { max = 100, tolerance = 0.2 }
            build_time_clean_s = { max = 60, optional = true }
            layout_us_1k_nodes = { max = 500 }
            "#,
        )
        .unwrap()
    }

    #[test]
    fn a_regression_beyond_the_tolerance_fails_and_within_it_passes() {
        let measured = |cold: f64| {
            BTreeMap::from([
                ("cold_start_ms".to_owned(), cold),
                ("layout_us_1k_nodes".to_owned(), 400.0),
            ])
        };
        assert!(judge(&budgets(), &measured(119.0), false, false).passes());
        let over = judge(&budgets(), &measured(121.0), false, false);
        assert!(!over.passes());
        assert!(!over.verdicts["cold_start_ms"].within);
    }

    #[test]
    fn a_display_paced_key_is_advisory_only_without_a_pacing_display() {
        let budgets: BudgetFile = toml::from_str(
            r#"
            target = "test"
            [budget]
            frame_time_p50_ms = { max = 17.5, display_paced = true }
            cold_start_ms = { max = 100 }
            "#,
        )
        .unwrap();
        let measured = |frame: f64, cold: f64| {
            BTreeMap::from([
                ("frame_time_p50_ms".to_owned(), frame),
                ("cold_start_ms".to_owned(), cold),
            ])
        };
        // On a machine with a real display, a slow frame fails.
        assert!(!judge(&budgets, &measured(65.0, 50.0), false, false).passes());
        // Without one it is reported, not enforced...
        let unpaced = judge(&budgets, &measured(65.0, 50.0), false, true);
        assert!(unpaced.passes());
        assert!(unpaced.verdicts["frame_time_p50_ms"].advisory);
        assert!(!unpaced.verdicts["frame_time_p50_ms"].within);
        // ...and every other key still is.
        assert!(!judge(&budgets, &measured(65.0, 500.0), false, true).passes());
    }

    #[test]
    fn unbudgeted_and_unmeasured_keys_fail() {
        let measured = BTreeMap::from([
            ("cold_start_ms".to_owned(), 50.0),
            ("layout_us_1k_nodes".to_owned(), 1.0),
            ("new_metric".to_owned(), 1.0),
        ]);
        assert_eq!(judge(&budgets(), &measured, false, false).unbudgeted, ["new_metric"]);
        let missing = BTreeMap::from([("cold_start_ms".to_owned(), 50.0)]);
        // The optional build time is not required; the layout key is.
        assert_eq!(judge(&budgets(), &missing, false, false).unmeasured, ["layout_us_1k_nodes"]);
    }
}
