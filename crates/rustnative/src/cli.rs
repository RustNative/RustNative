//! The command line itself, and what each command does.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::doctor::Report;
use crate::error::{Error, Result};
use crate::platform::Platform;
use crate::project::{FrameworkSource, Project, create};

/// The version of the framework a generated project depends on when it is
/// not pointed at a checkout.
const FRAMEWORK_VERSION: &str = "0.1";

/// Create, build, run, and diagnose Rust Native applications.
#[derive(Debug, Parser)]
#[command(name = "rustnative", version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// What `rustnative` was asked to do.
/// `rustnative tokens`.
#[derive(Debug, clap::Subcommand)]
pub enum TokensCommand {
    /// Import a W3C Design Tokens file into the style file's `@theme` block.
    Import {
        /// The token file (`tokens.json`).
        file: PathBuf,
        /// The style file to write (default: the project's).
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create a new application.
    New {
        /// The project's name: a folder, a Cargo package, an executable.
        name: String,
        /// Where to create it (the current folder by default).
        #[arg(long, value_name = "DIR")]
        path: Option<PathBuf>,
        /// Depend on a checkout of the framework rather than on published
        /// versions — what to use while working on the framework itself.
        #[arg(long, value_name = "DIR")]
        framework_path: Option<PathBuf>,
        /// Which syntax the project is written in. There is no default:
        /// neither is the one a developer should prefer (`PLAN.md` 2.9),
        /// and both templates are the same application.
        #[arg(long, value_enum, required_unless_present = "web")]
        syntax: Option<crate::project::Syntax>,
        /// A web application: a page with an interactive island, served by
        /// the application and exportable as static files (the builder
        /// syntax).
        #[arg(long)]
        web: bool,
    },
    /// Generate a binding from an interface description (`.ril`): the C
    /// header, the C# bindings, or the Rust implementation shims.
    Bindgen {
        /// The `.ril` file.
        file: PathBuf,
        /// The language to generate.
        #[arg(long, value_enum)]
        lang: BindgenLanguage,
        /// Where to write it (standard output if omitted).
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// The translator's workflow: extract the messages the code uses, merge
    /// them into every translation, show where one is used, and lint for
    /// untranslated literals (`PLAN.md` Milestone 46).
    I18n {
        /// What to do.
        #[command(subcommand)]
        command: crate::i18n::I18nCommand,
    },
    /// Database migrations (`PLAN.md` Milestone 49).
    Db {
        /// What to do.
        #[command(subcommand)]
        command: crate::db::DbCommand,
    },
    /// Deploy the server: infrastructure descriptions, or a local
    /// deployment with revisions, traffic splitting, and rollback
    /// (`PLAN.md` Milestone 50).
    Deploy {
        /// What to do.
        #[command(subcommand)]
        command: crate::deploy::DeployCommand,
    },
    /// Sign desktop updates (`PLAN.md` Milestone 50).
    Update {
        /// What to do.
        #[command(subcommand)]
        command: crate::deploy::UpdateCommand,
    },
    /// Crash reports the application left on this machine (`PLAN.md`
    /// Milestone 51).
    Crash {
        /// What to do.
        #[command(subcommand)]
        command: crate::crash::CrashCommand,
    },
    /// Generate compliance evidence into `target/compliance/`: a software
    /// bill of materials, licenses, privacy and permission manifests,
    /// accessibility results, and requirement traceability (`PLAN.md`
    /// Milestone 51).
    Compliance,
    /// Add a capability package, after checking it supports this
    /// project's backends and framework version (`PLAN.md` Milestone 52).
    Add {
        /// A path to the package's crate, or a name from the index.
        package: String,
        /// Another index file.
        #[arg(long, value_name = "FILE")]
        index: Option<PathBuf>,
    },
    /// Search the capability-package index.
    Search {
        /// Words in a package's name, description, or contract.
        term: String,
        /// Another index file.
        #[arg(long, value_name = "FILE")]
        index: Option<PathBuf>,
    },
    /// Carry the project across breaking changes with codemods (`PLAN.md`
    /// Milestone 52, `docs/policy/stability.md`).
    Upgrade {
        /// The framework version the project was written for.
        #[arg(long)]
        from: String,
        /// Show what would change without writing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Describe the framework for tools: elements and their builder
    /// methods, utilities, capabilities, events, and services (`PLAN.md`
    /// Milestone 52).
    Describe {
        /// Print the whole description as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Design tokens (`PLAN.md` Milestone 48).
    Tokens {
        /// What to do.
        #[command(subcommand)]
        command: TokensCommand,
    },
    /// Generate a component, a screen, or a service, with its preview and
    /// its test, in the project's syntax (`PLAN.md` Milestone 43).
    Generate {
        /// What to generate.
        #[command(subcommand)]
        what: crate::generate::Generate,
    },
    /// Browse the application's previews in the preview catalogue — every
    /// preview across its configurations — or, with `--headless`, run them
    /// as golden tests (`PLAN.md` Milestone 43).
    Preview {
        /// The preview to open first.
        name: Option<String>,
        /// Run the previews as golden tests on the headless backend instead.
        #[arg(long)]
        headless: bool,
    },
    /// Measure the framework's budget scenarios for a target against
    /// `budgets/<target>.toml`, writing `target/budget-report.json`
    /// (`PLAN.md` Milestone 42). Run in the framework's repository.
    Bench {
        /// The target whose budget file to measure against.
        #[arg(long, value_enum, default_value = "windows")]
        target: crate::bench::BenchTarget,
        /// Fail on a measurement over budget, or one the budget file does
        /// not declare.
        #[arg(long)]
        check: bool,
        /// Pin the scenarios to one core: the low-end reference profile.
        #[arg(long)]
        low_end: bool,
        /// Also time a clean and an incremental build.
        #[arg(long)]
        build_times: bool,
    },
    /// Inspect a running application: its tree, components and state,
    /// layout and style explanations, trace, tasks, capabilities; the
    /// overlay; recording (`PLAN.md` Milestone 44).
    Inspect {
        /// Where the application is.
        #[command(flatten)]
        target: crate::inspect::Target,
        /// What to ask.
        #[command(subcommand)]
        question: crate::inspect::Question,
    },
    /// Print the builder form a file's markup lowers to — a `.rsx` file, or
    /// the `rsx!` calls in a `.rs` file — or, with `--classes`/`--styles`,
    /// the declarations and typed values a class string or declaration
    /// block lowers to, against the project's `app.css`.
    Expand {
        /// The file.
        #[arg(required_unless_present_any = ["classes", "styles"])]
        file: Option<PathBuf>,
        /// A class string to expand.
        #[arg(long, conflicts_with = "file")]
        classes: Option<String>,
        /// A declaration block to expand.
        #[arg(long, conflicts_with_all = ["file", "classes"])]
        styles: Option<String>,
    },
    /// Format `.rsx` files (Rust and markup together) and the `rsx!` calls
    /// in `.rs` files — every such file in the project if none is named.
    Fmt {
        /// The files to format.
        files: Vec<PathBuf>,
        /// Report files that would change, and fail, instead of writing.
        #[arg(long)]
        check: bool,
    },
    /// Serve the language server protocol for `.rsx` files, forwarding to
    /// `rust-analyzer` over the lowered files.
    Lsp {
        /// The language server to forward to (a program and its arguments).
        #[arg(long, default_value = "rust-analyzer")]
        server: String,
    },
    /// A stand-in language server the `lsp` tests forward to.
    #[command(hide = true, name = "__echo-lsp")]
    EchoLsp,
    /// Build the application.
    Build {
        /// Which platform to build for.
        platform: Platform,
        /// Build with optimizations.
        #[arg(long)]
        release: bool,
        /// Profile-guided: instrument, run a scripted startup, rebuild with
        /// the profile (needs `rustup component add llvm-tools`).
        #[arg(long, requires = "release")]
        pgo: bool,
        /// Build through `sccache`, sharing compiled crates between local
        /// and CI builds (`C64`); without it installed, builds uncached.
        #[arg(long)]
        cache: bool,
        /// For `web`: how it is deployed.
        #[arg(long, value_enum, default_value = "server")]
        mode: crate::web::WebMode,
        /// For `web --mode serverless`: where it runs.
        #[arg(long, value_enum)]
        host: Option<crate::web::WebHost>,
    },
    /// Serve a build locally as its host would (`rustnative serve static
    /// target/web/client`).
    Serve {
        /// What to serve.
        #[command(subcommand)]
        command: ServeCommand,
    },
    /// Build and run the application.
    Run {
        /// Which platform to run on.
        platform: Platform,
        /// Build with optimizations.
        #[arg(long)]
        release: bool,
    },
    /// Check the application compiles, without building it fully.
    Check {
        /// Which platform to check for.
        #[arg(default_value = "windows")]
        platform: Platform,
    },
    /// Run the application's tests.
    Test {
        /// Run the tests again on every save (`rustnative test --watch`).
        #[arg(long)]
        watch: bool,
        /// Require the browser tests to run: fail, rather than skip, when
        /// no browser is installed.
        #[arg(long)]
        browser: bool,
        /// Arguments passed through to `cargo test`, flags included
        /// (`rustnative test --offline -- --nocapture`).
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        arguments: Vec<String>,
    },
    /// The development loop: build, run, and on every save either apply a
    /// token-only `app.css` change live or rebuild and restart with the
    /// application's state kept (`PLAN.md` Milestone 43).
    Dev {
        /// Which platform to run on.
        platform: Platform,
        /// Run on another machine, through its `rustnative dev-agent`.
        #[arg(long, requires = "token")]
        remote: Option<std::net::SocketAddr>,
        /// The agent's token.
        #[arg(long, requires = "remote")]
        token: Option<String>,
        /// Restart once after starting, print how long it took as JSON, and
        /// exit — what the budget harness measures.
        #[arg(long)]
        once: bool,
    },
    /// Receive builds from `rustnative dev --remote` and run them on this
    /// machine. It runs what a holder of the printed token sends, so it
    /// listens on loopback unless given another address.
    DevAgent {
        /// The address to listen on.
        #[arg(long, default_value = "127.0.0.1:7878")]
        listen: std::net::SocketAddr,
        /// Stop after this many deployments.
        #[arg(long)]
        max_deployments: Option<usize>,
    },
    /// Build the application and package it for distribution.
    Package {
        /// Which platform to package for.
        platform: Platform,
        /// Which package to produce.
        #[arg(long, value_enum, default_value = "all")]
        format: crate::package::Format,
        /// A `.pfx` certificate to sign the MSIX with.
        #[arg(long, value_name = "CERT")]
        sign: Option<PathBuf>,
        /// The environment variable holding the certificate's password.
        #[arg(long, value_name = "VAR", requires = "sign")]
        password_env: Option<String>,
        /// Also write an `.appinstaller` file for an MSIX published under
        /// this URL (a folder URL: the package and the file go there), so
        /// App Installer updates the installed copies.
        #[arg(long, value_name = "URL")]
        appinstaller: Option<String>,
    },
    /// Report what this machine can build.
    Doctor {
        /// Print the findings as JSON.
        #[arg(long)]
        json: bool,
        /// Install the missing toolchain pieces `rustup` can install
        /// (`PLAN.md` Milestone 43, `C90`).
        #[arg(long)]
        install: bool,
        /// With `--install`, print the commands instead of running them.
        #[arg(long, requires = "install")]
        dry_run: bool,
    },
}

impl Cli {
    /// Runs the command.
    ///
    /// # Errors
    ///
    /// Whatever the command could not do; see [`Error::exit_code`].
    #[allow(clippy::too_many_lines, reason = "one arm per command, each a few lines")]
    pub fn run(self) -> Result<()> {
        let here = std::env::current_dir()
            .map_err(|cause| Error::Io { what: "find the current folder".to_owned(), cause })?;
        match self.command {
            Command::New { name, path, framework_path, syntax, web } => {
                let parent = path.unwrap_or_else(|| here.clone());
                // A relative checkout path names a folder from here, not from
                // the new project, where Cargo will read it.
                let framework = framework_path.map_or_else(
                    || FrameworkSource::Published(FRAMEWORK_VERSION.to_owned()),
                    |path| {
                        FrameworkSource::Path(if path.is_absolute() {
                            path
                        } else {
                            here.join(path)
                        })
                    },
                );
                if web {
                    if syntax == Some(crate::project::Syntax::Markup) {
                        return Err(Error::Usage(
                            "the web template is written in the builder syntax; leave out --syntax"
                                .into(),
                        ));
                    }
                    let root = crate::project::create_web(&parent, &name, &framework)?;
                    println!("Created {}", root.display());
                    println!("  cd {name}");
                    println!("  rustnative dev web");
                    return Ok(());
                }
                let syntax = syntax.unwrap_or(crate::project::Syntax::Builder);
                let root = create(&parent, &name, &framework, syntax)?;
                println!("Created {}", root.display());
                println!("  cd {name}");
                println!(
                    "  rustnative run {}",
                    if cfg!(target_os = "linux") { "linux" } else { "windows" }
                );
                Ok(())
            }
            Command::Build { platform: Platform::Web, release, mode, host, .. } => {
                crate::web::build(&Project::find(&here)?, mode, host, release).map(|_| ())
            }
            Command::Run { platform: Platform::Web, release } => {
                crate::web::run(&Project::find(&here)?, release)
            }
            Command::Dev { platform: Platform::Web, once, .. } => {
                crate::web::dev(&Project::find(&here)?, once)
            }
            Command::Package { platform: Platform::Web, .. } => {
                crate::web::package(&Project::find(&here)?).map(|_| ())
            }
            Command::Serve { command: ServeCommand::Static { folder, address, requests } } => {
                crate::web::serve_static(&folder, address, requests)
            }
            Command::Serve {
                command: ServeCommand::Lambda { binary, address, memory_mb, timeout, requests },
            } => crate::emulate::lambda::serve(
                &binary,
                address,
                crate::emulate::lambda::LambdaOptions {
                    memory_mb,
                    timeout: std::time::Duration::from_secs(timeout),
                    requests,
                },
            ),
            Command::Serve {
                command:
                    ServeCommand::Wagi {
                        module,
                        address,
                        fuel,
                        route_fuel,
                        memory_mb,
                        response_kb,
                        deadline_ms,
                        actors,
                        env,
                        allow_http,
                        requests,
                    },
            } => {
                let pairs = |list: Vec<String>| -> Vec<(String, String)> {
                    list.into_iter()
                        .filter_map(|pair| {
                            pair.split_once('=').map(|(a, b)| (a.to_owned(), b.to_owned()))
                        })
                        .collect()
                };
                let route_fuel = pairs(route_fuel)
                    .into_iter()
                    .filter_map(|(prefix, fuel)| Some((prefix, fuel.parse().ok()?)))
                    .collect();
                crate::emulate::wagi::serve(
                    &module,
                    address,
                    crate::emulate::wagi::WagiOptions {
                        fuel,
                        route_fuel,
                        memory_bytes: memory_mb * 1024 * 1024,
                        response_bytes: response_kb * 1024,
                        deadline: std::time::Duration::from_millis(deadline_ms),
                        actor_prefix: actors,
                        env: pairs(env),
                        allow_http,
                        requests,
                    },
                )
            }
            Command::Build { platform, release, pgo, cache, .. } => {
                if pgo {
                    if platform.backend().is_none() {
                        return Err(Error::NoBackend {
                            platform,
                            milestone: platform.planned_milestone(),
                        });
                    }
                    crate::pgo::build(&Project::find(&here)?)
                } else {
                    let cached = cache && sccache_installed();
                    if cache && !cached {
                        println!("build: sccache is not installed; building without the cache");
                    }
                    let wrapper: &[&str] =
                        if cached { &["--config", "build.rustc-wrapper=\"sccache\""] } else { &[] };
                    cargo_for(platform, &here, "build", release, wrapper)
                }
            }
            Command::Run { platform, release } => cargo_for(platform, &here, "run", release, &[]),
            Command::Check { platform } => cargo_for(platform, &here, "check", false, &[]),
            Command::Test { watch, browser, arguments } => {
                let project = Project::find(&here)?;
                if browser {
                    // SAFETY: set before any thread of this process starts.
                    unsafe { std::env::set_var("RUSTNATIVE_BROWSER_REQUIRED", "1") };
                }
                let mut command = vec!["test".to_owned()];
                command.extend(arguments);
                if watch {
                    return crate::dev::watch_tests(&project, &command);
                }
                crate::diagnostics::run_cargo(&project.root, &command)
            }
            Command::Dev { platform, remote, token, once } => {
                if platform.backend().is_none() {
                    return Err(Error::NoBackend {
                        platform,
                        milestone: platform.planned_milestone(),
                    });
                }
                let remote =
                    remote.zip(token).map(|(addr, token)| crate::dev::Remote { addr, token });
                crate::dev::run(&here, remote, once)
            }
            Command::DevAgent { listen, max_deployments } => {
                crate::dev::agent(listen, max_deployments)
            }
            Command::Bindgen { file, lang, out } => {
                let source = std::fs::read_to_string(&file).map_err(|cause| Error::Io {
                    what: format!("read {}", file.display()),
                    cause,
                })?;
                let idl = rustnative_interop::parse_idl(&source)
                    .map_err(|error| Error::Usage(format!("{}:{error}", file.display())))?;
                let text = match lang {
                    BindgenLanguage::C => rustnative_interop::generate::c(&idl),
                    BindgenLanguage::Csharp => rustnative_interop::generate::csharp(&idl),
                    BindgenLanguage::Rust => rustnative_interop::generate::rust(&idl),
                };
                match out {
                    Some(path) => std::fs::write(&path, text).map_err(|cause| Error::Io {
                        what: format!("write {}", path.display()),
                        cause,
                    })?,
                    None => print!("{text}"),
                }
                Ok(())
            }
            Command::Inspect { target, question } => crate::inspect::run(&target, question),
            Command::Generate { what } => crate::generate::run(&here, &what),
            Command::I18n { command } => crate::i18n::run(&here, &command),
            Command::Db { command } => crate::db::run(&here, &command),
            Command::Add { package, index } => {
                crate::packages::add(&here, &package, index.as_deref())
            }
            Command::Search { term, index } => crate::packages::search(&term, index.as_deref()),
            Command::Upgrade { from, dry_run } => {
                crate::upgrade::run(&Project::find(&here)?.root, &from, dry_run)
            }
            Command::Describe { json } => {
                crate::describe::run(json);
                Ok(())
            }
            Command::Crash { command } => crate::crash::run(&here, &command),
            Command::Compliance => crate::compliance::run(&here),
            Command::Deploy { command } => crate::deploy::run(&here, command),
            Command::Update { command } => crate::deploy::run_update(&here, command),
            Command::Tokens { command: TokensCommand::Import { file, out } } => {
                crate::tokens::import(&here, &file, out)
            }
            Command::Preview { name, headless } => {
                let project = Project::find(&here)?;
                if headless {
                    let arguments = ["test".to_owned(), "--test".to_owned(), "previews".to_owned()];
                    return crate::diagnostics::run_cargo(&project.root, &arguments);
                }
                // The application's own `main` shows the catalogue when asked.
                let status = std::process::Command::new(
                    std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()),
                )
                .current_dir(&project.root)
                .arg("run")
                .env(rustnative_core::preview::PREVIEW_VARIABLE, name.unwrap_or_default())
                .status()
                .map_err(|cause| Error::ToolMissing {
                    tool: "cargo",
                    hint: "install the Rust toolchain from https://rustup.rs".into(),
                    cause: Some(cause.to_string()),
                })?;
                if status.success() {
                    Ok(())
                } else {
                    Err(Error::ToolFailed { tool: "cargo", code: status.code() })
                }
            }
            Command::Bench { target, check, low_end, build_times } => {
                crate::bench::run(&here, target, check, low_end, build_times)
            }
            Command::Expand { file, classes, styles } => {
                let text = match (file, classes, styles) {
                    (Some(file), _, _) => crate::markup::expand_file(&file)?,
                    (None, Some(classes), _) => crate::markup::expand_style(&here, &classes, true)?,
                    (None, None, Some(styles)) => {
                        crate::markup::expand_style(&here, &styles, false)?
                    }
                    (None, None, None) => {
                        return Err(Error::Usage(
                            "name a file, `--classes`, or `--styles`".to_owned(),
                        ));
                    }
                };
                print!("{text}");
                Ok(())
            }
            Command::Fmt { files, check } => {
                let files = if files.is_empty() {
                    let project = Project::find(&here)?;
                    crate::markup::project_files(&project.root.join("src"))
                } else {
                    files
                };
                let mut unformatted = Vec::new();
                for file in &files {
                    let formatted = crate::markup::format_file(file)?;
                    let current = std::fs::read_to_string(file)
                        .map_err(|cause| Error::Io {
                            what: format!("read {}", file.display()),
                            cause,
                        })?
                        .replace("\r\n", "\n");
                    if formatted == current {
                        continue;
                    }
                    if check {
                        unformatted.push(file.display().to_string());
                    } else {
                        std::fs::write(file, formatted).map_err(|cause| Error::Io {
                            what: format!("write {}", file.display()),
                            cause,
                        })?;
                        println!("Formatted {}", file.display());
                    }
                }
                if unformatted.is_empty() {
                    Ok(())
                } else {
                    Err(Error::Usage(format!("not formatted:\n  {}", unformatted.join("\n  "))))
                }
            }
            Command::Lsp { server } => crate::lsp::serve(&server),
            Command::EchoLsp => crate::lsp::echo_server(),
            Command::Package { platform: Platform::Linux, format, .. } => {
                if !format.is_linux() {
                    return Err(Error::Usage(
                        "Linux packages are deb, tar, appimage, or all".to_owned(),
                    ));
                }
                linux_host(Platform::Linux)?;
                let project = Project::find(&here)?;
                for path in crate::package::package_linux(&project.root, &project.config, format)? {
                    println!("Packaged {}", path.display());
                }
                Ok(())
            }
            Command::Package { platform, format, sign, password_env, appinstaller } => {
                if platform.backend().is_none() {
                    return Err(Error::NoBackend {
                        platform,
                        milestone: platform.planned_milestone(),
                    });
                }
                if !format.is_windows() {
                    return Err(Error::Usage("Windows packages are zip, msix, or all".to_owned()));
                }
                let project = Project::find(&here)?;
                let signing = sign
                    .map(|certificate| crate::package::sign::Signing { certificate, password_env });
                let produced = crate::package::package(
                    &project.root,
                    &project.config,
                    format,
                    signing.as_ref(),
                )?;
                if let Some(base) = appinstaller {
                    let base = base.trim_end_matches('/');
                    let msix = produced
                        .iter()
                        .find(|path| path.extension().is_some_and(|extension| extension == "msix"));
                    let Some(msix) = msix else {
                        return Err(Error::Usage(
                            "--appinstaller needs an MSIX (--format msix or all)".to_owned(),
                        ));
                    };
                    let name = msix
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let file =
                        msix.with_file_name(format!("{}.appinstaller", project.config.app.name));
                    let text = crate::package::msix::appinstaller(
                        &project.config,
                        &format!("{base}/{}.appinstaller", project.config.app.name),
                        &format!("{base}/{name}"),
                    );
                    std::fs::write(&file, text).map_err(|cause| Error::Io {
                        what: format!("write {}", file.display()),
                        cause,
                    })?;
                    println!("Packaged {}", file.display());
                }
                for path in produced {
                    println!("Packaged {}", path.display());
                }
                Ok(())
            }
            Command::Doctor { json, install, dry_run } => {
                if install {
                    return crate::doctor::install(dry_run);
                }
                let report = Report::gather();
                if json {
                    let text = report.to_json().map_err(|cause| Error::Io {
                        what: "write the report".to_owned(),
                        cause: std::io::Error::other(cause.to_string()),
                    })?;
                    println!("{text}");
                } else {
                    print!("{}", report.to_text());
                }
                // A machine that cannot build is a failure a script should
                // see, not just read about.
                if report.is_healthy() {
                    Ok(())
                } else {
                    Err(Error::Usage(
                        "some toolchains are missing; see the report above".to_owned(),
                    ))
                }
            }
        }
    }
}

/// Runs one Cargo subcommand for `platform`, refusing the platforms that
/// have no backend.
/// Whether `sccache` runs.
fn sccache_installed() -> bool {
    std::process::Command::new("sccache")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn cargo_for(
    platform: Platform,
    here: &std::path::Path,
    subcommand: &str,
    release: bool,
    extra: &[&str],
) -> Result<()> {
    if platform.backend().is_none() {
        return Err(Error::NoBackend { platform, milestone: platform.planned_milestone() });
    }
    linux_host(platform)?;
    let project = Project::find(here)?;
    println!("{subcommand}: {} for {platform}", project.config.app.display_name);
    let mut arguments = vec![subcommand.to_owned()];
    if release {
        arguments.push("--release".to_owned());
    }
    arguments.extend(extra.iter().map(|argument| (*argument).to_owned()));
    // Structured diagnostics, so positions in lowered `.rsx` files are
    // reported in the `.rsx` file (see `diagnostics`).
    crate::diagnostics::run_cargo(&project.root, &arguments)
}

/// A Linux application is built on Linux: GTK and its libraries are the
/// host's. On Windows, WSL is a Linux host (`wsl rustnative run linux`).
fn linux_host(platform: Platform) -> Result<()> {
    if platform == Platform::Linux && !cfg!(target_os = "linux") {
        return Err(Error::Usage(
            "build for Linux on Linux (from Windows, inside WSL: `wsl rustnative run linux`)"
                .to_owned(),
        ));
    }
    Ok(())
}

/// `rustnative serve`.
#[derive(Debug, clap::Subcommand)]
pub enum ServeCommand {
    /// A static export, with the headers its `_headers` gives each file.
    Static {
        /// The export's folder.
        #[arg(default_value = "target/web/client")]
        folder: PathBuf,
        /// Where to listen.
        #[arg(long, default_value = "127.0.0.1:8080")]
        address: std::net::SocketAddr,
        /// Stop after this many requests (for tests).
        #[arg(long, hide = true)]
        requests: Option<usize>,
    },
    /// A function binary behind an emulated function runtime (AWS Lambda's
    /// runtime API), each request an API Gateway v2 event.
    Lambda {
        /// The function binary (built with `rustnative build web --mode
        /// serverless --host lambda`).
        binary: PathBuf,
        /// Where to listen.
        #[arg(long, default_value = "127.0.0.1:9000")]
        address: std::net::SocketAddr,
        /// The memory size reported to the function, in megabytes.
        #[arg(long, default_value_t = 128)]
        memory_mb: u64,
        /// How long an invocation may take, in seconds.
        #[arg(long, default_value_t = 10)]
        timeout: u64,
        /// Stop after this many requests (for tests).
        #[arg(long, hide = true)]
        requests: Option<usize>,
    },
    /// A `wasm32-wasip1` module run per request by an emulated edge host
    /// (WAGI), metered in fuel under a memory ceiling.
    Wagi {
        /// The module (built with `rustnative build web --mode serverless
        /// --host edge`).
        module: PathBuf,
        /// Where to listen.
        #[arg(long, default_value = "127.0.0.1:9100")]
        address: std::net::SocketAddr,
        /// The CPU budget per request, in fuel.
        #[arg(long, default_value_t = 2_000_000_000)]
        fuel: u64,
        /// A route's own budget, `PREFIX=FUEL` (repeatable; the longest
        /// matching prefix wins).
        #[arg(long = "route-fuel", value_name = "PREFIX=FUEL")]
        route_fuel: Vec<String>,
        /// The memory ceiling, in megabytes.
        #[arg(long, default_value_t = 128)]
        memory_mb: u64,
        /// The largest response, in kilobytes.
        #[arg(long, default_value_t = 6144)]
        response_kb: u64,
        /// How long a request may take, in milliseconds.
        #[arg(long, default_value_t = 30_000)]
        deadline_ms: u64,
        /// Requests under this prefix are for the actor its next segment
        /// names, one at a time per actor.
        #[arg(long)]
        actors: Option<String>,
        /// Configuration and secrets for the module, `NAME=VALUE`
        /// (repeatable).
        #[arg(long, value_name = "NAME=VALUE")]
        env: Vec<String>,
        /// A `host:port` the module may send HTTP requests to (repeatable).
        #[arg(long = "allow-http", value_name = "HOST:PORT")]
        allow_http: Vec<String>,
        /// Stop after this many requests (for tests).
        #[arg(long, hide = true)]
        requests: Option<usize>,
    },
}

/// The languages `rustnative bindgen` writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum BindgenLanguage {
    /// A C header.
    C,
    /// C# P/Invoke bindings.
    Csharp,
    /// The Rust implementation shims.
    Rust,
}
