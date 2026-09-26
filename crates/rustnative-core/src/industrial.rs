//! Industrial service contracts (`PLAN.md` Milestone 51): printing and
//! serial ports, which line-of-business and device applications need and
//! which each backend answers from its host's own facilities (Windows:
//! the print spooler and the COM ports).

use std::path::PathBuf;

use crate::services::ServiceError;

/// A document to print: pages of lines of text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintJob {
    /// Its title, as the spooler shows it.
    pub title: String,
    /// The pages, each a list of lines.
    pub pages: Vec<Vec<String>>,
    /// The printer (none: the default).
    pub printer: Option<String>,
    /// Print to this file instead of paper, when the printer supports it
    /// (Windows: a document writer).
    pub output: Option<PathBuf>,
}

/// Printing.
pub trait PrintService: Send + Sync {
    /// The installed printers.
    fn printers(&self) -> Vec<String>;

    /// Prints `job`.
    ///
    /// # Errors
    ///
    /// The printer refused or does not exist.
    fn print(&self, job: &PrintJob) -> Result<(), ServiceError>;
}

/// Serial-port settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SerialSettings {
    /// Bits per second.
    pub baud: u32,
    /// Data bits (5–8).
    pub data_bits: u8,
    /// Two stop bits instead of one.
    pub two_stop_bits: bool,
    /// Even parity (none otherwise).
    pub even_parity: bool,
}

impl Default for SerialSettings {
    fn default() -> Self {
        Self { baud: 9600, data_bits: 8, two_stop_bits: false, even_parity: false }
    }
}

/// An open serial port.
pub trait SerialPort: Send {
    /// Writes `bytes`.
    ///
    /// # Errors
    ///
    /// The port failed.
    fn write(&mut self, bytes: &[u8]) -> Result<usize, ServiceError>;

    /// Reads what has arrived, waiting up to the port's timeout.
    ///
    /// # Errors
    ///
    /// The port failed.
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, ServiceError>;
}

/// Serial ports.
pub trait SerialService: Send + Sync {
    /// The ports present (`COM3`).
    fn ports(&self) -> Vec<String>;

    /// Opens `port` with `settings`.
    ///
    /// # Errors
    ///
    /// The port does not exist or is in use.
    fn open(
        &self,
        port: &str,
        settings: SerialSettings,
    ) -> Result<Box<dyn SerialPort>, ServiceError>;
}
