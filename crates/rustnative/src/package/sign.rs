//! Signing a package with the SDK's `signtool`.
//!
//! `rustnative` never holds a password: `--password-env` names an environment
//! variable, and the value is read from it and handed to `signtool`
//! directly, so it is not in the command line a person typed, in their
//! shell history, or in this process's arguments as another process could
//! list them.
//!
//! The certificate's subject has to match the package's `Publisher`.
//! `rustnative` checks what it can see — that the publisher is an X.500 name at all
//! — and lets `signtool` make the real comparison against the certificate,
//! which it does and reports precisely; a package signed by a mismatched
//! certificate would install nowhere, so failing here is the point.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// How a package is to be signed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signing {
    /// The `.pfx` certificate file.
    pub certificate: PathBuf,
    /// The environment variable holding its password, if it has one.
    pub password_env: Option<String>,
}

/// The `signtool sign` arguments for `package`.
///
/// # Errors
///
/// [`Error::Usage`] if the certificate is missing, or the named
/// environment variable is not set.
pub fn arguments(signing: &Signing, package: &Path) -> Result<Vec<String>> {
    if !signing.certificate.is_file() {
        return Err(Error::Usage(format!(
            "{} is not a certificate file",
            signing.certificate.display()
        )));
    }
    let mut arguments = vec![
        "sign".to_owned(),
        // SHA-256: SHA-1 signatures are not accepted by Windows for MSIX.
        "/fd".to_owned(),
        "SHA256".to_owned(),
        "/a".to_owned(),
        "/f".to_owned(),
        signing.certificate.display().to_string(),
    ];
    if let Some(variable) = &signing.password_env {
        let password = std::env::var(variable)
            .map_err(|_| Error::Usage(format!("the environment variable {variable} is not set")))?;
        arguments.push("/p".to_owned());
        arguments.push(password);
    }
    arguments.push(package.display().to_string());
    Ok(arguments)
}

/// Checks what can be checked before `signtool` is called: that the
/// package declares a publisher, and that it is an X.500 name.
///
/// # Errors
///
/// [`Error::Usage`] naming what is wrong.
pub fn check_publisher(publisher: Option<&str>) -> Result<()> {
    let Some(publisher) = publisher else {
        return Err(Error::Usage(
            "app.publisher is not set in rustnative.toml, and a signed package needs one that matches \
             the certificate's subject"
                .to_owned(),
        ));
    };
    if !publisher.split(',').map(str::trim).any(|part| part.starts_with("CN=")) {
        return Err(Error::Usage(format!(
            "app.publisher must be an X.500 name with a CN, like `CN=Example Ltd`, not `{publisher}`"
        )));
    }
    Ok(())
}

/// Signs `package`.
///
/// # Errors
///
/// [`Error::ToolMissing`] if `signtool` is not installed, or
/// [`Error::ToolFailed`] with its own message — which is where a
/// certificate that does not match the publisher is reported.
pub fn sign(signtool: &Path, signing: &Signing, package: &Path) -> Result<()> {
    let arguments = arguments(signing, package)?;
    let output =
        std::process::Command::new(signtool).args(&arguments).output().map_err(|cause| {
            Error::ToolMissing {
                tool: "signtool",
                hint: "install the Windows SDK".to_owned(),
                cause: Some(cause.to_string()),
            }
        })?;
    if output.status.success() {
        return Ok(());
    }
    // `signtool`'s own message says exactly what went wrong (a wrong
    // password, a subject that does not match the publisher); passing it
    // through is more useful than anything this could say instead.
    eprintln!("{}", String::from_utf8_lossy(&output.stdout).trim());
    eprintln!("{}", String::from_utf8_lossy(&output.stderr).trim());
    Err(Error::ToolFailed { tool: "signtool", code: output.status.code() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn certificate(test: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("rustnative-sign-{test}-{}.pfx", std::process::id()));
        std::fs::write(&path, b"not a real certificate").expect("a scratch file");
        path
    }

    #[test]
    fn the_arguments_ask_for_a_sha256_signature_and_never_show_the_password() {
        let certificate = certificate("arguments");
        // SAFETY: this test reads the variable back itself, on this thread,
        // and removes it immediately.
        unsafe { std::env::set_var("RUSTNATIVE_TEST_PFX_PASSWORD", "hunter2") };
        let signing = Signing {
            certificate: certificate.clone(),
            password_env: Some("RUSTNATIVE_TEST_PFX_PASSWORD".to_owned()),
        };
        let arguments = arguments(&signing, Path::new("demo.msix")).expect("built");
        // SAFETY: as above.
        unsafe { std::env::remove_var("RUSTNATIVE_TEST_PFX_PASSWORD") };

        assert_eq!(arguments[0], "sign");
        assert!(arguments.windows(2).any(|pair| pair == ["/fd", "SHA256"]), "{arguments:?}");
        assert!(arguments.contains(&"hunter2".to_owned()), "the password reaches signtool");
        assert!(
            !arguments.contains(&"RUSTNATIVE_TEST_PFX_PASSWORD".to_owned()),
            "the variable's name is not passed, its value is"
        );
        assert_eq!(arguments.last().map(String::as_str), Some("demo.msix"));
        std::fs::remove_file(certificate).ok();
    }

    #[test]
    fn an_unset_password_variable_is_reported_before_anything_runs() {
        let certificate = certificate("unset-password");
        let signing = Signing {
            certificate: certificate.clone(),
            password_env: Some("RUSTNATIVE_TEST_NO_SUCH_VARIABLE".to_owned()),
        };
        let error = arguments(&signing, Path::new("demo.msix")).expect_err("refused");
        assert!(error.to_string().contains("RUSTNATIVE_TEST_NO_SUCH_VARIABLE"), "{error}");
        std::fs::remove_file(certificate).ok();
    }

    #[test]
    fn a_missing_certificate_is_refused() {
        let signing =
            Signing { certificate: PathBuf::from("no-such-file.pfx"), password_env: None };
        assert!(arguments(&signing, Path::new("demo.msix")).is_err());
    }

    #[test]
    fn a_publisher_must_be_an_x500_name() {
        check_publisher(Some("CN=Example Ltd, O=Example")).expect("valid");
        check_publisher(Some("O=Example, CN=Example Ltd")).expect("order does not matter");
        assert!(check_publisher(None).is_err());
        let error = check_publisher(Some("Example Ltd")).expect_err("refused");
        assert!(error.to_string().contains("CN="), "{error}");
    }
}
