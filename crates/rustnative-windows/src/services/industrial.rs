//! Printing and serial ports on Windows (`PLAN.md` Milestone 51): the
//! spooler through GDI (`StartDocW`), and COM ports through `CreateFileW`
//! and the communications API.

use std::os::windows::io::{AsRawHandle, FromRawHandle};

use rustnative_core::ServiceError;
use rustnative_core::industrial::{
    PrintJob, PrintService, SerialPort, SerialService, SerialSettings,
};
use windows_sys::Win32::Devices::Communication::{
    COMMTIMEOUTS, DCB, GetCommState, SetCommState, SetCommTimeouts,
};
use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Graphics::Gdi::{CreateDCW, DeleteDC, TextOutW};
use windows_sys::Win32::Graphics::Printing::{
    EnumPrintersW, GetDefaultPrinterW, PRINTER_ENUM_CONNECTIONS, PRINTER_ENUM_LOCAL,
    PRINTER_INFO_4W,
};
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, OPEN_EXISTING};
use windows_sys::Win32::Storage::Xps::{DOCINFOW, EndDoc, EndPage, StartDocW, StartPage};

use crate::native::util::wide;

fn error(what: &str) -> ServiceError {
    ServiceError::new(format!("{what}: {}", std::io::Error::last_os_error()))
}

/// Windows printing.
#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsPrinting;

fn from_wide(pointer: *const u16) -> String {
    if pointer.is_null() {
        return String::new();
    }
    // SAFETY: the spooler returns NUL-terminated strings inside the buffer
    // it filled, alive for the caller's use.
    let length = (0..=usize::MAX).take_while(|&index| unsafe { *pointer.add(index) } != 0).count();
    // SAFETY: `length` elements were just read.
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(pointer, length) })
}

impl PrintService for WindowsPrinting {
    fn printers(&self) -> Vec<String> {
        let flags = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;
        let (mut needed, mut count) = (0u32, 0u32);
        // SAFETY: a size query with no buffer.
        unsafe {
            EnumPrintersW(
                flags,
                std::ptr::null(),
                4,
                std::ptr::null_mut(),
                0,
                &raw mut needed,
                &raw mut count,
            )
        };
        let mut buffer = vec![0u64; usize::try_from(needed).unwrap_or(0).div_ceil(8)];
        // SAFETY: the buffer holds `needed` bytes, aligned for the structs.
        let ok = unsafe {
            EnumPrintersW(
                flags,
                std::ptr::null(),
                4,
                buffer.as_mut_ptr().cast(),
                needed,
                &raw mut needed,
                &raw mut count,
            )
        } != 0;
        if !ok {
            return Vec::new();
        }
        let infos = buffer.as_ptr().cast::<PRINTER_INFO_4W>();
        (0..usize::try_from(count).unwrap_or(0))
            // SAFETY: the spooler wrote `count` records at the buffer's start.
            .map(|index| from_wide(unsafe { (*infos.add(index)).pPrinterName }))
            .collect()
    }

    fn print(&self, job: &PrintJob) -> Result<(), ServiceError> {
        let printer = if let Some(name) = &job.printer {
            name.clone()
        } else {
            let mut size = 256u32;
            let mut name = vec![0u16; 256];
            // SAFETY: a buffer of `size` characters.
            if unsafe { GetDefaultPrinterW(name.as_mut_ptr(), &raw mut size) } == 0 {
                return Err(ServiceError::new("there is no default printer"));
            }
            from_wide(name.as_ptr())
        };
        let (driver, device) = (wide("WINSPOOL"), wide(&printer));
        // SAFETY: NUL-terminated strings; no device mode.
        let dc = unsafe {
            CreateDCW(driver.as_ptr(), device.as_ptr(), std::ptr::null(), std::ptr::null())
        };
        if dc.is_null() {
            return Err(error(&format!("open printer {printer:?}")));
        }
        let title = wide(&job.title);
        let output = job.output.as_ref().map(|path| wide(path.as_os_str()));
        let info = DOCINFOW {
            cbSize: i32::try_from(std::mem::size_of::<DOCINFOW>()).unwrap_or(0),
            lpszDocName: title.as_ptr(),
            lpszOutput: output.as_ref().map_or(std::ptr::null(), Vec::as_ptr),
            lpszDatatype: std::ptr::null(),
            fwType: 0,
        };
        let result = (|| {
            // SAFETY: a printer DC and a DOCINFOW whose strings outlive the call.
            if unsafe { StartDocW(dc, &raw const info) } <= 0 {
                return Err(error("StartDoc"));
            }
            for page in &job.pages {
                // SAFETY: an open document on this DC.
                if unsafe { StartPage(dc) } <= 0 {
                    return Err(error("StartPage"));
                }
                for (index, line) in page.iter().enumerate() {
                    let text: Vec<u16> = line.encode_utf16().collect();
                    let y =
                        i32::try_from(index).unwrap_or(0).saturating_mul(120).saturating_add(200);
                    // SAFETY: `text` is valid for its length.
                    unsafe {
                        TextOutW(dc, 200, y, text.as_ptr(), i32::try_from(text.len()).unwrap_or(0))
                    };
                }
                // SAFETY: the page started above.
                if unsafe { EndPage(dc) } <= 0 {
                    return Err(error("EndPage"));
                }
            }
            // SAFETY: the document started above.
            if unsafe { EndDoc(dc) } <= 0 {
                return Err(error("EndDoc"));
            }
            Ok(())
        })();
        // SAFETY: the DC this function created.
        unsafe { DeleteDC(dc) };
        result
    }
}

/// Windows COM ports.
#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsSerial;

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

impl SerialService for WindowsSerial {
    fn ports(&self) -> Vec<String> {
        // A port that opens is present; COM1–COM32 covers ordinary machines.
        (1..=32)
            .map(|number| format!("COM{number}"))
            .filter(|name| {
                let path = wide(format!(r"\\.\{name}"));
                // SAFETY: a NUL-terminated path; the handle is closed at once.
                let handle = unsafe {
                    CreateFileW(
                        path.as_ptr(),
                        GENERIC_READ,
                        0,
                        std::ptr::null(),
                        OPEN_EXISTING,
                        0,
                        std::ptr::null_mut(),
                    )
                };
                let present = handle != INVALID_HANDLE_VALUE
                    || std::io::Error::last_os_error().raw_os_error() == Some(5);
                if handle != INVALID_HANDLE_VALUE {
                    // SAFETY: the handle just opened.
                    unsafe { windows_sys::Win32::Foundation::CloseHandle(handle) };
                }
                present
            })
            .collect()
    }

    fn open(
        &self,
        port: &str,
        settings: SerialSettings,
    ) -> Result<Box<dyn SerialPort>, ServiceError> {
        let path = wide(format!(r"\\.\{port}"));
        // SAFETY: a NUL-terminated path.
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(error(&format!("open {port}")));
        }
        // SAFETY: a handle we own, handed to `File` once.
        let file = unsafe { std::fs::File::from_raw_handle(handle) };
        // SAFETY: zeroed is a valid DCB before GetCommState fills it.
        let mut dcb: DCB = unsafe { std::mem::zeroed() };
        dcb.DCBlength = u32::try_from(std::mem::size_of::<DCB>()).unwrap_or(0);
        let raw = file.as_raw_handle();
        // SAFETY: a comm handle and a DCB of the right size.
        if unsafe { GetCommState(raw, &raw mut dcb) } == 0 {
            return Err(error("GetCommState"));
        }
        dcb.BaudRate = settings.baud;
        dcb.ByteSize = settings.data_bits;
        dcb.StopBits = if settings.two_stop_bits { 2 } else { 0 };
        dcb.Parity = if settings.even_parity { 2 } else { 0 };
        let timeouts = COMMTIMEOUTS {
            ReadIntervalTimeout: 50,
            ReadTotalTimeoutMultiplier: 0,
            ReadTotalTimeoutConstant: 500,
            WriteTotalTimeoutMultiplier: 0,
            WriteTotalTimeoutConstant: 500,
        };
        // SAFETY: as above.
        let configured = unsafe { SetCommState(raw, &raw const dcb) } != 0;
        // SAFETY: a comm handle and a timeouts struct.
        if !configured || unsafe { SetCommTimeouts(raw, &raw const timeouts) } == 0 {
            return Err(error("configure the port"));
        }
        Ok(Box::new(Port(file)))
    }
}
