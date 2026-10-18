//! `rustnative deploy` and `rustnative update` (`PLAN.md` Milestone 50):
//! infrastructure descriptions from the project, and update manifests the
//! installed application's updater accepts.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helper functions in an integration test are test code too: a failed expectation is the test failing"
)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

fn project(name: &str) -> std::path::PathBuf {
    let project = std::env::temp_dir().join(format!("rustnative-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&project);
    std::fs::create_dir_all(&project).unwrap();
    project
}

fn run(project: &std::path::Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rustnative"))
        .current_dir(project)
        .args(arguments)
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).into_owned()
}

const APP: &str = "[app]\nname = \"notes\"\nid = \"dev.rustnative.notes\"\ndisplay-name = \"Notes\"\nversion = \"1.0.0\"\n";

#[test]
fn export_writes_every_description() {
    let project = project("deploy");
    std::fs::write(
        project.join("rustnative.toml"),
        format!("{APP}\n[resources.data]\nkind = \"directory\"\n"),
    )
    .unwrap();
    stdout(&run(&project, &["deploy", "export", "all", "--port", "9000"]));
    let dockerfile = std::fs::read_to_string(project.join("deploy/Dockerfile")).unwrap();
    assert!(
        dockerfile.contains("COPY target/release/notes /app/notes")
            && dockerfile.contains("EXPOSE 9000")
    );
    let compose = std::fs::read_to_string(project.join("deploy/compose.yaml")).unwrap();
    assert!(compose.contains("RUSTNATIVE_RESOURCE_DATA"), "{compose}");
    assert!(
        project.join("deploy/kubernetes.yaml").is_file()
            && project.join("deploy/notes.service").is_file()
    );
    assert!(
        !run(&project, &["deploy", "export", "helm"]).status.success(),
        "an unknown target is refused"
    );
    let _ = std::fs::remove_dir_all(project);
}

#[test]
fn a_manifest_is_signed_only_with_the_trusted_key() {
    let project = project("update");
    std::fs::write(project.join("rustnative.toml"), APP).unwrap();
    let printed = stdout(&run(&project, &["update", "keygen", "--out", "publisher.key"]));
    let public =
        printed.split("public-key = \"").nth(1).unwrap().split('"').next().unwrap().to_owned();
    std::fs::write(project.join("package.zip"), b"the package").unwrap();
    let manifest = [
        "update",
        "manifest",
        "--version",
        "1.1.0",
        "--url",
        "https://example.com/notes-1.1.0.zip",
    ];
    let with = |key: &str| {
        let mut arguments = manifest.to_vec();
        arguments.extend(["--package", "package.zip", "--rollout", "100", "--key", key]);
        run(&project, &arguments)
    };

    assert!(
        !with("publisher.key").status.success(),
        "no [update] public-key: nothing is trusted yet"
    );
    std::fs::write(
        project.join("rustnative.toml"),
        format!("{APP}\n[update]\npublic-key = \"{public}\"\n"),
    )
    .unwrap();
    let signed = stdout(&with("publisher.key"));

    stdout(&run(&project, &["update", "keygen", "--out", "other.key"]));
    assert!(!with("other.key").status.success(), "a key the application does not trust is refused");

    #[cfg(windows)]
    {
        use rustnative_windows::update::{Decision, UpdateError, Updater};
        let updater =
            Updater::new(project.join("install"), "dev.rustnative.notes", &public, "1.0.0")
                .unwrap();
        let Ok(Decision::Update(taken)) = updater.check(signed.as_bytes()) else {
            panic!("the updater accepts what the CLI signed: {signed}")
        };
        assert_eq!(updater.stage(&taken, b"tampered"), Err(UpdateError::BadDigest));
        assert!(
            updater.stage(&taken, b"the package").is_ok(),
            "the digest the CLI wrote is the package's"
        );
        let other =
            Updater::new(project.join("other"), "dev.rustnative.other", &public, "1.0.0").unwrap();
        assert!(
            matches!(other.check(signed.as_bytes()), Err(UpdateError::WrongApplication { .. })),
            "another application signed with the same key refuses it"
        );
    }
    #[cfg(not(windows))]
    let _ = signed;
    let _ = std::fs::remove_dir_all(project);
}

struct Process(Child);

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// The status line of a raw request to the control port.
fn status_of(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response.lines().next().unwrap_or_default().to_owned()
}

/// `deploy local`'s other commands reach the control API with the token
/// `start` left for this user; nothing without it does.
#[test]
fn the_local_control_api_answers_only_this_user_s_cli() {
    let runtime = project("deploy-run");
    let (proxy, control) = (free_port(), free_port());
    let _deployment = Process(
        Command::new(env!("CARGO_BIN_EXE_rustnative"))
            .args(["deploy", "local", "start"])
            .args(["--port", &proxy.to_string(), "--control", &control.to_string()])
            .env("XDG_RUNTIME_DIR", &runtime)
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let token_file = runtime.join("rustnative").join(format!("deploy-{control}.token"));
    let started = Instant::now();
    while !token_file.is_file() || TcpStream::connect(("127.0.0.1", control)).is_err() {
        assert!(started.elapsed() < Duration::from_secs(60), "the deployment did not start");
        std::thread::sleep(Duration::from_millis(50));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let preview = runtime.join("rustnative").join(format!("deploy-{control}-preview.token"));
        for file in [&token_file, &preview] {
            let mode = std::fs::metadata(file).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "only this user reads {}", file.display());
        }
    }
    let cli = |arguments: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_rustnative"))
            .args(["deploy", "local"])
            .args(arguments)
            .args(["--control", &control.to_string()])
            .env("XDG_RUNTIME_DIR", &runtime)
            .output()
            .unwrap()
    };
    stdout(&cli(&["add", "r1", "127.0.0.1:9001"]));
    stdout(&cli(&["add", "r2", "127.0.0.1:9002"]));
    stdout(&cli(&["promote", "r2", "--percent", "100"]));
    let status = stdout(&cli(&["status"]));
    assert!(status.contains(r#""weights":{"r2":100}"#), "{status}");

    let body = r#"{"revision":"r1","percent":100}"#;
    let forged = status_of(
        control,
        &format!(
            "POST /promote HTTP/1.1\r\nHost: 127.0.0.1:{control}\r\nContent-Type: text/plain;charset=UTF-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    );
    assert!(forged.contains(" 401 "), "{forged}");
    let elsewhere =
        std::env::temp_dir().join(format!("rustnative-elsewhere-{}", std::process::id()));
    let other_user = Command::new(env!("CARGO_BIN_EXE_rustnative"))
        .args(["deploy", "local", "rollback", "--control", &control.to_string()])
        .env("XDG_RUNTIME_DIR", &elsewhere)
        .output()
        .unwrap();
    assert!(!other_user.status.success(), "no token, no control");
    assert!(stdout(&cli(&["status"])).contains(r#""weights":{"r2":100}"#));
    let _ = std::fs::remove_dir_all(runtime);
}
