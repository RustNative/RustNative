//! Printing and serial ports on Linux (`PLAN.md` Milestone 51): GTK's
//! print operation — CUPS's printers through GTK's print backends, the
//! same ones every GTK application's print dialog lists — and serial ports
//! through termios.

use std::os::fd::AsRawFd;

use gtk::prelude::*;
use rustnative_core::ServiceError;
use rustnative_core::industrial::{
    PrintJob, PrintService, SerialPort, SerialService, SerialSettings,
};

use super::on_gtk_blocking;

/// GTK printing.
#[derive(Debug, Default, Clone, Copy)]
pub struct LinuxPrinting;

/// Where each line of a page is drawn: a top margin, then one line height
/// of the default font per line.
const MARGIN_POINTS: f64 = 72.0;

impl PrintService for LinuxPrinting {
    fn printers(&self) -> Vec<String> {
        on_gtk_blocking(|| {
            let names = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let found = std::sync::Arc::clone(&names);
            // `wait`: returns once every backend (CUPS, the file printer)
            // has listed its printers.
            gtk::enumerate_printers(
                move |printer| {
                    found
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push(printer.name().to_string());
                    false
                },
                true,
            );
            Ok(std::mem::take(
                &mut *names.lock().unwrap_or_else(std::sync::PoisonError::into_inner),
            ))
        })
        .unwrap_or_default()
    }

    fn print(&self, job: &PrintJob) -> Result<(), ServiceError> {
        let job = job.clone();
        on_gtk_blocking(move || {
            let operation = gtk::PrintOperation::new();
            operation.set_job_name(&job.title);
            operation.set_n_pages(i32::try_from(job.pages.len().max(1)).unwrap_or(i32::MAX));
            if let Some(printer) = &job.printer {
                let settings = gtk::PrintSettings::new();
                settings.set_printer(printer);
                operation.set_print_settings(Some(&settings));
            }
            let pages = job.pages.clone();
            operation.connect_draw_page(move |_, context, page| {
                let cr = context.cairo_context();
                let layout = context.create_pango_layout();
                let lines = usize::try_from(page)
                    .ok()
                    .and_then(|page| pages.get(page))
                    .cloned()
                    .unwrap_or_default();
                let mut y = MARGIN_POINTS;
                for line in lines {
                    layout.set_text(&line);
                    cr.move_to(MARGIN_POINTS, y);
                    pangocairo::functions::show_layout(&cr, &layout);
                    y += f64::from(layout.pixel_size().1).max(1.0);
                }
            });
            let action = if let Some(output) = &job.output {
                // A file instead of paper: GTK writes PDF.
                operation.set_export_filename(output);
                gtk::PrintOperationAction::Export
            } else {
                gtk::PrintOperationAction::Print
            };
            match operation.run(action, None::<&gtk::Window>) {
                Ok(gtk::PrintOperationResult::Error) | Err(_) => {
                    Err(ServiceError::new(format!("printing {:?} failed", job.title)))
                }
                Ok(gtk::PrintOperationResult::Cancel) => {
                    Err(ServiceError::new("printing was cancelled"))
                }
                Ok(_) => Ok(()),
            }
        })
    }
}

/// Serial ports through termios.
#[derive(Debug, Default, Clone, Copy)]
pub struct LinuxSerial;

struct Port(std::fs::File);

impl SerialPort for Port {
    fn write(&mut self, bytes: &[u8]) -> Result<usize, ServiceError> {
        std::io::Write::write(&mut self.0, bytes)
            .map_err(|failure| ServiceError::new(failure.to_string()))
    }

    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, ServiceError> {
        std::io::Read::read(&mut self.0, buffer)
            .map_err(|failure| ServiceError::new(failure.to_string()))
    }
}

fn speed(baud: u32) -> Option<libc::speed_t> {
    Some(match baud {
        1200 => libc::B1200,
        2400 => libc::B2400,
        4800 => libc::B4800,
        9600 => libc::B9600,
        19_200 => libc::B19200,
        38_400 => libc::B38400,
        57_600 => libc::B57600,
        115_200 => libc::B115200,
        230_400 => libc::B230400,
        460_800 => libc::B460800,
        921_600 => libc::B921600,
        _ => return None,
    })
}

/// Raw mode with `settings`, reads waiting up to half a second (as on
/// Windows).
fn configure(file: &std::fs::File, settings: SerialSettings) -> Result<(), ServiceError> {
    let error =
        |what: &str| ServiceError::new(format!("{what}: {}", std::io::Error::last_os_error()));
    let speed = speed(settings.baud).ok_or_else(|| {
        ServiceError::new(format!("{} baud is not a standard rate", settings.baud))
    })?;
    // SAFETY: plain data, filled by `tcgetattr` before use.
    let mut options: libc::termios = unsafe { std::mem::zeroed() };
    // SAFETY: an open descriptor and a termios to fill.
    if unsafe { libc::tcgetattr(file.as_raw_fd(), &raw mut options) } != 0 {
        return Err(error("not a serial port"));
    }
    // SAFETY: the structure `tcgetattr` filled.
    unsafe {
        libc::cfmakeraw(&raw mut options);
        libc::cfsetispeed(&raw mut options, speed);
        libc::cfsetospeed(&raw mut options, speed);
    }
    options.c_cflag &= !(libc::CSIZE | libc::CSTOPB | libc::PARENB | libc::PARODD);
    options.c_cflag |= libc::CLOCAL
        | libc::CREAD
        | match settings.data_bits {
            5 => libc::CS5,
            6 => libc::CS6,
            7 => libc::CS7,
            _ => libc::CS8,
        };
    if settings.two_stop_bits {
        options.c_cflag |= libc::CSTOPB;
    }
    if settings.even_parity {
        options.c_cflag |= libc::PARENB;
    }
    // A read returns what has arrived, or nothing after 0.5 s.
    options.c_cc[libc::VMIN] = 0;
    options.c_cc[libc::VTIME] = 5;
    // SAFETY: an open descriptor and a complete termios.
    if unsafe { libc::tcsetattr(file.as_raw_fd(), libc::TCSANOW, &raw const options) } != 0 {
        return Err(error("configure the port"));
    }
    Ok(())
}

impl SerialService for LinuxSerial {
    fn ports(&self) -> Vec<String> {
        // A tty with a device behind it is a real port (`ttyS0`, `ttyUSB0`,
        // `ttyACM0`); virtual consoles and pseudo-terminals have none. A
        // legacy `ttyS*` the kernel lists without hardware is filtered by
        // its UART type.
        let Ok(entries) = std::fs::read_dir("/sys/class/tty") else {
            return Vec::new();
        };
        let mut ports: Vec<String> = entries
            .flatten()
            .filter(|entry| entry.path().join("device").exists())
            .filter(|entry| {
                std::fs::read_to_string(entry.path().join("type"))
                    .map_or(true, |kind| kind.trim() != "0")
            })
            .map(|entry| format!("/dev/{}", entry.file_name().to_string_lossy()))
            .collect();
        ports.sort();
        ports
    }

    fn open(
        &self,
        port: &str,
        settings: SerialSettings,
    ) -> Result<Box<dyn SerialPort>, ServiceError> {
        use std::os::unix::fs::OpenOptionsExt as _;
        // A bare name (`ttyUSB0`) is under `/dev`.
        let path = if port.starts_with('/') { port.to_owned() } else { format!("/dev/{port}") };
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOCTTY)
            .open(&path)
            .map_err(|failure| ServiceError::new(format!("open {path}: {failure}")))?;
        configure(&file, settings)?;
        Ok(Box::new(Port(file)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_serial_port_is_configured_raw_and_carries_bytes_both_ways() {
        // A pseudo-terminal stands in for the device: the service opens the
        // follower side as it would `/dev/ttyUSB0`.
        // SAFETY: plain calls on the descriptor `posix_openpt` returns.
        let (leader, follower) = unsafe {
            let leader = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
            assert!(leader >= 0 && libc::grantpt(leader) == 0 && libc::unlockpt(leader) == 0);
            let name =
                std::ffi::CStr::from_ptr(libc::ptsname(leader)).to_string_lossy().into_owned();
            (std::os::fd::FromRawFd::from_raw_fd(leader), name)
        };
        let mut leader: std::fs::File = leader;
        let mut port = LinuxSerial
            .open(&follower, SerialSettings { baud: 115_200, ..SerialSettings::default() })
            .expect("opens");
        port.write(b"ping").expect("writes");
        let mut buffer = [0_u8; 16];
        let read = std::io::Read::read(&mut leader, &mut buffer).expect("the device reads");
        assert_eq!(&buffer[..read], b"ping", "raw: no echo or translation");
        std::io::Write::write_all(&mut leader, b"pong\n").expect("the device writes");
        let read = port.read(&mut buffer).expect("reads");
        assert_eq!(&buffer[..read], b"pong\n", "raw: no line discipline");
        let started = std::time::Instant::now();
        assert_eq!(port.read(&mut buffer).expect("an empty read"), 0, "nothing arrived");
        assert!(
            started.elapsed() >= std::time::Duration::from_millis(400),
            "waited for the timeout"
        );
        assert!(
            LinuxSerial
                .open(&follower, SerialSettings { baud: 1234, ..SerialSettings::default() })
                .is_err()
        );
        assert!(LinuxSerial.ports().iter().all(|port| port.starts_with("/dev/tty")));
    }

    #[test]
    fn a_job_prints_to_a_pdf_file() {
        crate::gtk::testing::on_gtk(|| {
            let output =
                std::env::temp_dir().join(format!("rustnative-print-{}.pdf", std::process::id()));
            let _ = std::fs::remove_file(&output);
            let job = PrintJob {
                title: "Report".into(),
                pages: vec![
                    vec!["first page".into()],
                    vec!["second page".into(), "line two".into()],
                ],
                printer: None,
                output: Some(output.clone()),
            };
            LinuxPrinting.print(&job).expect("exported");
            let pdf = std::fs::read(&output).expect("written");
            assert!(pdf.starts_with(b"%PDF"));
            // The page tree is compressed; an embedded font shows the text
            // was drawn.
            assert!(String::from_utf8_lossy(&pdf).contains("/Type /Font"), "text was drawn");
            let _ = LinuxPrinting.printers();
        });
    }
}
