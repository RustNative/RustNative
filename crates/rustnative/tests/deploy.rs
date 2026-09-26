//! `rustnative deploy` and `rustnative update` (`PLAN.md` Milestone 50):
//! infrastructure descriptions from the project, and update manifests the
//! installed application's updater accepts.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helper functions in an integration test are test code too: a failed expectation is the test failing"
)]

use std::process::{Command, Output};

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
        let updater = Updater::new(project.join("install"), &public, "1.0.0").unwrap();
        let Ok(Decision::Update(taken)) = updater.check(signed.as_bytes()) else {
            panic!("the updater accepts what the CLI signed: {signed}")
        };
        assert_eq!(updater.stage(&taken, b"tampered"), Err(UpdateError::BadDigest));
        assert!(
            updater.stage(&taken, b"the package").is_ok(),
            "the digest the CLI wrote is the package's"
        );
    }
    #[cfg(not(windows))]
    let _ = signed;
    let _ = std::fs::remove_dir_all(project);
}
