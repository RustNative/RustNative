//! The development loop's remote half and its restart (`PLAN.md` Milestone
//! 43): `rustnative dev-agent` receives a build over the wire, refuses a
//! wrong token, and starts what it is sent; `rustnative dev --once` restarts
//! an application keeping its state.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helper functions in an integration test are test code too: a failed expectation is the test failing"
)]

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

fn rustnative() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rustnative"))
}

/// Sends `bytes` as the executable `name` to the agent at `addr`.
fn deploy(addr: &str, token: &str, name: &str, bytes: &[u8], args: &[&str]) -> Value {
    let mut stream = TcpStream::connect(addr).unwrap();
    let header = json!({ "token": token, "name": name, "size": bytes.len(), "args": args });
    writeln!(stream, "{header}").unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let answer: Value = serde_json::from_str(&line).unwrap();
    if answer["ready"] != json!(true) {
        return answer;
    }
    stream.write_all(bytes).unwrap();
    line.clear();
    reader.read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}

#[test]
fn the_agent_runs_what_a_token_holder_sends_and_nothing_else() {
    let mut agent = rustnative()
        .args(["dev-agent", "--listen", "127.0.0.1:0", "--max-deployments", "2"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut first = String::new();
    BufReader::new(agent.stdout.take().unwrap()).read_line(&mut first).unwrap();
    // "rustnative dev-agent listening on <addr> token <token>"
    let words: Vec<&str> = first.split_whitespace().collect();
    let (addr, token) = (words[4], words[6]);

    let program = std::fs::read(env!("CARGO_BIN_EXE_rustnative")).unwrap();
    let refused = deploy(addr, &"0".repeat(token.len()), "app.exe", &program, &[]);
    assert_eq!(refused["error"], json!("wrong token"));

    // A program that is not inspectable runs and exits: the agent says so.
    let ran = deploy(addr, token, "../escape/app.exe", &program, &["--version"]);
    assert!(ran["error"].is_null(), "{ran}");
    assert!(ran["pid"].as_u64().unwrap() > 0);
    assert_eq!(ran["exited"], json!(0));
    assert!(agent.wait().unwrap().success());
    // Written under the user's own folder only — never the shared temporary
    // directory, nor outside the folder — and removed once it is done.
    let folder = rustnative_core::secret::user_directory("rustnative-dev-agent");
    assert!(!folder.with_file_name("escape").exists(), "the name's directories are dropped");
    assert!(!std::env::temp_dir().join("rustnative-dev-agent").join("app.exe").exists());
    assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0, "the deployment is cleaned up");
    rustnative_core::secret::check_private_directory(&folder).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(&folder).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }
}

#[test]
#[ignore = "builds and runs a desktop application; the interactive CI pass runs it"]
fn a_restart_keeps_the_application_s_state() {
    let hello = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hello-label");
    let output =
        rustnative().current_dir(hello).args(["dev", "windows", "--once"]).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let restart: Value =
        stdout.lines().rev().find_map(|line| serde_json::from_str(line).ok()).unwrap();
    assert_eq!(restart["restored"], json!(2), "the counter's count and name: {restart}");
    assert_eq!(restart["refused"], json!(0));
}

#[test]
fn doctor_says_what_it_would_install_without_installing() {
    let output = rustnative().args(["doctor", "--install", "--dry-run"]).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("nothing to install") || text.contains("rustup component add llvm-tools"),
        "{text}"
    );
}
