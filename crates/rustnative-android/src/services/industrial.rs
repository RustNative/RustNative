//! Printing and serial ports.

use rustnative_core::ServiceError;
use rustnative_core::industrial::{
    PrintJob, PrintService, SerialPort, SerialService, SerialSettings,
};

use super::{cache_dir, checked, java};
use crate::jni_host::{Arg, Class};

/// Printing: the job laid out as a PDF (`PdfDocument`, A4), written to
/// `job.output` when it names a file, else handed to the print UI
/// (`PrintManager`), where the person picks a printer or saves a PDF.
#[derive(Debug, Default, Clone, Copy)]
pub struct AndroidPrinting;

impl PrintService for AndroidPrinting {
    /// Android does not list printers to applications: its print UI finds
    /// them through the installed print services. The list is empty, and
    /// `job.printer` is not consulted.
    fn printers(&self) -> Vec<String> {
        Vec::new()
    }

    fn print(&self, job: &PrintJob) -> Result<(), ServiceError> {
        let mut lines = Vec::new();
        let mut starts = Vec::new();
        for page in &job.pages {
            starts.push(i32::try_from(lines.len()).unwrap_or(i32::MAX));
            lines.extend(page.iter().cloned());
        }
        let path = match &job.output {
            Some(path) => path.clone(),
            None => cache_dir()?.join("rustnative-print.pdf"),
        };
        let path_text = path.to_string_lossy();
        checked(java(
            Class::Services,
            "writePdf",
            "(Ljava/lang/String;[Ljava/lang/String;[I)Ljava/lang/String;",
            &[Arg::Str(&path_text), Arg::Strs(&lines), Arg::Ints(&starts)],
        )?)?;
        if job.output.is_some() {
            return Ok(());
        }
        checked(java(
            Class::Services,
            "printPdf",
            "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
            &[Arg::Str(&job.title), Arg::Str(&path_text)],
        )?)
    }
}

const NO_SERIAL: &str = "Android gives applications no serial ports: a USB serial adapter needs the USB host API \
     and a driver for its chip, which this backend does not include";

/// Serial ports: unavailable, saying why.
#[derive(Debug, Default, Clone, Copy)]
pub struct AndroidSerial;

impl SerialService for AndroidSerial {
    fn ports(&self) -> Vec<String> {
        Vec::new()
    }

    fn open(
        &self,
        _port: &str,
        _settings: SerialSettings,
    ) -> Result<Box<dyn SerialPort>, ServiceError> {
        Err(ServiceError::new(NO_SERIAL))
    }
}
