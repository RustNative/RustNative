//! Milestone 31 acceptance tests: the `rustnative` binary, run as a person runs
//! it.
//!
//! Every test here starts the real executable in a real folder and checks
//! its exit code and output. The heaviest one generates a project and
//! compiles it with Cargo, which is the only way to know the templates
//! produce something that builds.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helper functions in an integration test are test code too: a failed expectation is the test failing"
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The `rustnative` binary Cargo built for this test.
fn rustnative() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rustnative"))
}

/// This workspace's root, which generated projects depend on by path.
fn workspace() -> PathBuf {
    // `crates/rustnative` -> the workspace root.
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf()
}

/// A fresh folder for one test.
fn scratch(name: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("rustnative-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("a scratch folder");
    directory
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Creates a builder-syntax project in a scratch folder, depending on this
/// workspace.
fn new_project(name: &str) -> PathBuf {
    new_project_in(name, "builder")
}

/// Creates a project written in `syntax`.
fn new_project_in(name: &str, syntax: &str) -> PathBuf {
    let parent = scratch(name);
    let output = rustnative()
        .args(["new", name, "--syntax", syntax, "--path"])
        .arg(&parent)
        .arg("--framework-path")
        .arg(workspace())
        .output()
        .expect("rustnative runs");
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("Created"));
    parent.join(name)
}

#[test]
fn a_generated_project_compiles() {
    let project = new_project("compiles");
    // The workspace's own target folder: the framework and its dependencies
    // are already built there, so this checks the generated code rather
    // than spending minutes rebuilding the world. `--offline` keeps the
    // test from depending on the network.
    let output = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .current_dir(&project)
        .args(["check", "--offline"])
        .env("CARGO_TARGET_DIR", workspace().join("target"))
        .output()
        .expect("cargo runs");
    assert!(output.status.success(), "the generated project must compile:\n{}", stderr(&output));
}

#[test]
fn a_new_project_has_a_valid_config_and_is_found_from_inside_it() {
    let project = new_project("valid-config");
    let config = std::fs::read_to_string(project.join("rustnative.toml")).unwrap();
    assert!(config.contains("name = \"valid-config\""), "{config}");
    assert!(config.contains("id = \"com.example.validconfig\""), "{config}");

    // `rustnative check` finds the project from a subfolder, and gets as far as
    // running Cargo (which is what "found the project" looks like).
    let output = rustnative()
        .current_dir(project.join("src"))
        .args(["check", "windows"])
        .env("CARGO_TARGET_DIR", workspace().join("target"))
        .output()
        .expect("rustnative runs");
    assert!(stdout(&output).contains("check: valid-config for windows"), "{}", stdout(&output));
    assert!(output.status.success(), "{}", stderr(&output));
}

#[test]
fn every_platform_without_a_backend_is_refused_by_name() {
    let project = new_project("no-backend");
    let expected = [
        ("macos", "Milestone 33"),
        ("android", "Milestone 35"),
        ("ios", "Milestone 36"),
        ("embedded", "Milestone 37"),
    ];
    for (platform, mention) in expected {
        let output = rustnative()
            .current_dir(&project)
            .args(["build", platform])
            .output()
            .expect("rustnative runs");
        assert_eq!(output.status.code(), Some(3), "{platform}: {}", stderr(&output));
        let message = stderr(&output);
        assert!(message.contains(&format!("no backend for {platform} yet")), "{message}");
        assert!(message.contains(mention), "{message}");
    }
}

/// Linux has a backend, built on a Linux host; elsewhere the command says
/// where to build it rather than building a stub.
#[cfg(not(target_os = "linux"))]
#[test]
fn linux_is_built_on_linux_and_the_error_says_where() {
    let project = new_project("linux-elsewhere");
    let output = rustnative()
        .current_dir(&project)
        .args(["build", "linux"])
        .output()
        .expect("rustnative runs");
    assert!(!output.status.success());
    let message = stderr(&output);
    assert!(message.contains("build for Linux on Linux") && message.contains("WSL"), "{message}");
}

#[test]
fn an_unknown_platform_is_a_usage_error() {
    let project = new_project("unknown-platform");
    let output = rustnative()
        .current_dir(&project)
        .args(["build", "atari"])
        .output()
        .expect("rustnative runs");
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("atari"), "{}", stderr(&output));
}

#[test]
fn a_broken_config_names_the_field() {
    let project = new_project("broken-config");
    let config = std::fs::read_to_string(project.join("rustnative.toml")).unwrap();
    std::fs::write(
        project.join("rustnative.toml"),
        config.replace("version = \"0.1.0\"", "version = \"1\""),
    )
    .unwrap();

    let output = rustnative()
        .current_dir(&project)
        .args(["build", "windows"])
        .output()
        .expect("rustnative runs");
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("app.version"), "{}", stderr(&output));
}

#[test]
fn outside_a_project_the_error_says_how_to_make_one() {
    let empty = scratch("outside");
    let output = rustnative()
        .current_dir(&empty)
        .args(["build", "windows"])
        .output()
        .expect("rustnative runs");
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("rustnative new"), "{}", stderr(&output));
}

/// Windows-only: what `doctor` reports about the MSVC toolchain and the
/// SDK is only true on a machine that has them, and on Linux `doctor`
/// correctly reports the opposite (and exits non-zero for it).
#[cfg(windows)]
#[test]
fn doctor_reports_this_machine_as_json() {
    let output = rustnative().args(["doctor", "--json"]).output().expect("rustnative runs");
    let report: serde_json::Value =
        serde_json::from_str(&stdout(&output)).expect("valid JSON on stdout");
    let checks = report["checks"].as_array().expect("checks");
    let named = |name: &str| {
        checks
            .iter()
            .find(|check| check["name"] == name)
            .unwrap_or_else(|| panic!("{name} is checked"))
            .clone()
    };
    assert_eq!(named("rustc")["ok"], serde_json::Value::Bool(true), "rustc built this test");
    assert_eq!(named("cargo")["ok"], serde_json::Value::Bool(true));

    // This machine built `rustnative-windows`, so the MSVC toolchain and the
    // SDK are here; `doctor` must agree, and say where they are.
    for name in ["visual studio (c++ build tools)", "windows sdk", "rc", "mt"] {
        assert_eq!(named(name)["ok"], serde_json::Value::Bool(true), "{name}: {}", named(name));
        assert_ne!(named(name)["detail"].as_str().unwrap_or_default(), "");
    }

    let platforms = report["platforms"].as_array().expect("platforms");
    let windows = platforms.iter().find(|entry| entry["platform"] == "windows").expect("windows");
    assert_eq!(windows["ready"], serde_json::Value::Bool(true));
    assert!(output.status.success(), "{}", stderr(&output));
}

#[test]
fn doctor_without_json_is_a_readable_table() {
    let output = rustnative().arg("doctor").output().expect("rustnative runs");
    let text = stdout(&output);
    assert!(text.contains("Toolchains"), "{text}");
    assert!(text.contains("Platforms"), "{text}");
    assert!(text.contains("no backend yet — Milestone 33"), "{text}");
}

#[test]
fn test_passes_its_arguments_through_to_cargo() {
    let project = new_project("passthrough");
    // A filter that matches nothing still succeeds; a nonsense *flag* does
    // not. Both prove the arguments reached Cargo.
    let ok = rustnative()
        .current_dir(&project)
        .args(["test", "--offline", "no-such-test"])
        .env("CARGO_TARGET_DIR", workspace().join("target"))
        .output()
        .expect("rustnative runs");
    assert!(ok.status.success(), "{}", stderr(&ok));

    let bad = rustnative()
        .current_dir(&project)
        .args(["test", "--definitely-not-a-cargo-flag"])
        .env("CARGO_TARGET_DIR", workspace().join("target"))
        .output()
        .expect("rustnative runs");
    assert_eq!(bad.status.code(), Some(1));
    assert!(stderr(&bad).contains("cargo failed"), "{}", stderr(&bad));
}

#[test]
fn creating_over_an_existing_project_is_refused() {
    let project = new_project("twice");
    let parent: &Path = project.parent().unwrap();
    let output = rustnative()
        .args(["new", "twice", "--syntax", "builder", "--path"])
        .arg(parent)
        .arg("--framework-path")
        .arg(workspace())
        .output()
        .expect("rustnative runs");
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("already exists"), "{}", stderr(&output));
}

/// Windows-only: the executable's name, and the linker that produces it.
#[cfg(windows)]
#[test]
fn building_a_generated_project_produces_an_executable() {
    let project = new_project("builds");
    let target = workspace().join("target");
    let output = rustnative()
        .current_dir(&project)
        .args(["build", "windows"])
        .env("CARGO_TARGET_DIR", &target)
        .output()
        .expect("rustnative runs");
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("build: builds for windows"), "{}", stdout(&output));
    assert!(
        target.join("debug").join("builds.exe").is_file(),
        "an executable at {}",
        target.join("debug").join("builds.exe").display()
    );
}

#[test]
fn new_requires_a_syntax_and_offers_both() {
    let parent = scratch("no-syntax");
    let output = rustnative()
        .args(["new", "undecided", "--path"])
        .arg(&parent)
        .output()
        .expect("rustnative runs");
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    let message = stderr(&output);
    assert!(message.contains("--syntax"), "{message}");
}

#[test]
fn a_generated_markup_project_compiles() {
    let project = new_project_in("markup-compiles", "markup");
    assert!(project.join("src/app.rsx").is_file());
    let output = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .current_dir(&project)
        .args(["check", "--offline"])
        .env("CARGO_TARGET_DIR", workspace().join("target"))
        .output()
        .expect("cargo runs");
    assert!(output.status.success(), "the markup project must compile:\n{}", stderr(&output));
}

#[test]
fn a_diagnostic_in_an_rsx_file_is_reported_where_it_was_written() {
    let project = new_project_in("rsx-diagnostic", "markup");
    let app = project.join("src/app.rsx");
    let source = std::fs::read_to_string(&app).unwrap();
    let broken = source.replace(
        r#"<Button key="click" text="Click me" />"#,
        r#"<Button key="click" text={42_u32} />"#,
    );
    assert_ne!(broken, source, "the template has the button this test edits");
    std::fs::write(&app, &broken).unwrap();
    let line = broken.lines().position(|line| line.contains("42_u32")).unwrap() + 1;
    let column = broken.lines().nth(line - 1).unwrap().find("42_u32").unwrap() + 1;

    let output = rustnative()
        .current_dir(&project)
        .args(["check", "windows"])
        .env("CARGO_TARGET_DIR", workspace().join("target"))
        .output()
        .expect("rustnative runs");
    assert!(!output.status.success(), "the error fails the check");
    let message = stderr(&output);
    let expected = format!("app.rsx:{line}:{column}");
    assert!(message.contains(&expected), "reported at {expected}:\n{message}");
    assert!(message.contains("text={42_u32}"), "the quoted line is the .rsx line:\n{message}");
    assert!(
        !message.contains("::rustnative_core::rsx!"),
        "the lowering does not show through:\n{message}"
    );
}

#[test]
fn expand_and_fmt_work_on_a_markup_project() {
    let project = new_project_in("rsx-tools", "markup");
    let output = rustnative()
        .current_dir(&project)
        .args(["expand", "src/app.rsx"])
        .output()
        .expect("rustnative runs");
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("column_with_layout"), "{}", stdout(&output));

    let output = rustnative()
        .current_dir(&project)
        .args(["fmt", "--check"])
        .output()
        .expect("rustnative runs");
    assert!(output.status.success(), "the template is formatted: {}", stderr(&output));
}

/// The editor proxy, end to end: a `.rsx` document opened through
/// `rustnative lsp` reaches the language server as its lowered file, and
/// the server's diagnostic comes back at the `.rsx` position.
#[test]
fn the_lsp_proxy_maps_documents_and_diagnostics() {
    use std::io::{BufRead, BufReader, Read, Write};
    let project = new_project_in("rsx-lsp", "markup");
    let app = project.join("src/app.rsx");
    let text = std::fs::read_to_string(&app)
        .unwrap()
        .replace(r#"<Column key="root">"#, r#"<Column key="root" gap=4>"#);
    let server = format!("{} __echo-lsp", env!("CARGO_BIN_EXE_rustnative"));
    let mut child = rustnative()
        .args(["lsp", "--server", &server])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("rustnative lsp starts");
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let send = |input: &mut std::process::ChildStdin, value: serde_json::Value| {
        let body = value.to_string();
        write!(input, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        input.flush().unwrap();
    };
    let receive = |output: &mut BufReader<std::process::ChildStdout>| -> serde_json::Value {
        let mut length = 0;
        loop {
            let mut header = String::new();
            output.read_line(&mut header).unwrap();
            let header = header.trim();
            if header.is_empty() {
                break;
            }
            if let Some(value) = header.strip_prefix("Content-Length:") {
                length = value.trim().parse().unwrap();
            }
        }
        let mut body = vec![0; length];
        output.read_exact(&mut body).unwrap();
        serde_json::from_slice(&body).unwrap()
    };
    let uri = format!("file:///{}", app.display().to_string().replace('\\', "/"));
    send(
        &mut input,
        serde_json::json!({ "jsonrpc": "2.0", "method": "textDocument/didOpen", "params": { "textDocument": { "uri": uri, "languageId": "rsx", "version": 1, "text": text } } }),
    );
    let diagnostic = receive(&mut output);
    assert_eq!(diagnostic["params"]["uri"], serde_json::json!(uri), "{diagnostic}");
    let line = text.lines().position(|line| line.contains("gap=4")).unwrap();
    let column = text.lines().nth(line).unwrap().find("gap").unwrap();
    assert_eq!(
        diagnostic["params"]["diagnostics"][0]["range"]["start"],
        serde_json::json!({ "line": line, "character": column })
    );
    send(&mut input, serde_json::json!({ "jsonrpc": "2.0", "method": "exit" }));
    drop(input);
    let _ = child.wait();
}

#[test]
fn expand_prints_what_a_class_string_lowers_to() {
    let output = rustnative()
        .args(["expand", "--classes", "p-4 hover:bg-blue-500/50"])
        .output()
        .expect("rustnative runs");
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("padding-top: calc(var(--spacing) * 4)  /* = 1rem */"), "{text}");
    assert!(
        text.contains(
            "hover:background-color: color-mix(in oklab, var(--color-blue-500) 50%, transparent)"
        ),
        "{text}"
    );

    let output = rustnative()
        .args(["expand", "--classes", "bg-bleu-500"])
        .output()
        .expect("rustnative runs");
    assert!(!output.status.success());
    assert!(stderr(&output).contains("did you mean `bg-blue-500`"), "{}", stderr(&output));
}

/// A generated project styles through `app.css`; a mistake in it fails the
/// build at the file, line, and column (`PLAN.md` Milestone 58).
#[test]
fn a_mistake_in_app_css_fails_the_build_where_it_was_written() {
    let project = new_project("css-diagnostic");
    let css = project.join("app.css");
    let source = std::fs::read_to_string(&css).unwrap();
    assert!(source.contains("@utility headline"), "the template has a project utility");
    std::fs::write(&css, format!("{source}\n.card {{ color: red; }}\n")).unwrap();
    let line = source.lines().count() + 2;

    let output = rustnative()
        .current_dir(&project)
        .args(["check", "windows"])
        .env("CARGO_TARGET_DIR", workspace().join("target"))
        .output()
        .expect("rustnative runs");
    assert!(!output.status.success(), "the mistake fails the check");
    let message = format!("{}{}", stderr(&output), stdout(&output));
    assert!(
        message.contains(&format!("app.css:{line}:1")),
        "reported at app.css:{line}:1:\n{message}"
    );
    assert!(message.contains("no selectors"), "{message}");
}

#[test]
fn bindgen_writes_each_language_from_one_description() {
    let description = workspace().join("examples/adoption-library/counter.ril");
    for (language, expected) in [
        ("c", "int32_t counter_Counter_increment(counter_Counter self, uint32_t* out);"),
        ("csharp", "public uint Increment()"),
        ("rust", "macro_rules! export_counter"),
    ] {
        let output = rustnative()
            .args(["bindgen"])
            .arg(&description)
            .args(["--lang", language])
            .output()
            .expect("rustnative runs");
        assert!(output.status.success(), "{}", stderr(&output));
        assert!(stdout(&output).contains(expected), "{language}: {}", stdout(&output));
    }
    let broken = scratch("bindgen").join("broken.ril");
    std::fs::write(&broken, "library x;\nservice S {\n    fn f(&self) -> float;\n}\n").unwrap();
    let output = rustnative()
        .args(["bindgen"])
        .arg(&broken)
        .args(["--lang", "c"])
        .output()
        .expect("rustnative runs");
    assert!(!output.status.success());
    assert!(stderr(&output).contains("broken.ril:3:"), "{}", stderr(&output));
}
