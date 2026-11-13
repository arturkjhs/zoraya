//! A virtio transport's registers for a driver, by mapping or by call
//! (ADR-0091, ADR-0295). A driver opens the device handle it was given:
//! when the kernel maps its page the registers are volatile accesses; when
//! the handle is a transport device (the page is shared with another
//! driver, ADR-0221 item 8) every access is a system call that the kernel
//! bounds to this one transport.

use zoraya_abi::{Handle, Status};
use zoraya_rt::{device_map, device_read, device_read_width, device_write, device_write_width};
use zoraya_virtio::Registers;

/// The registers of one transport.
pub enum Window {
    /// The address of the transport's block in this process's mapping.
    Mapped(u64),
    /// A transport device: register access by call.
    Call(Handle),
}

impl Window {
    /// Opens range `index` of `device`. A page device is mapped; a transport
    /// device (the kernel answers `device_map` with `AccessDenied`) is used
    /// by call, and its one range is 0.
    pub fn open(device: Handle, index: u32) -> Result<Window, Status> {
        match device_map(device, index) {
            Ok((window, offset)) => Ok(Window::Mapped(window + offset)),
            Err(Status::AccessDenied) if index == 0 => Ok(Window::Call(device)),
            Err(e) => Err(e),
        }
    }
}

impl Registers for Window {
    fn read(&self, offset: usize) -> u32 {
        match self {
            // SAFETY: a device mapping of this process, and `offset` a
            // register of the transport the kernel checked.
            Window::Mapped(at) => unsafe { ((at + offset as u64) as *const u32).read_volatile() },
            // A refused read gives 0, which no transport's magic is, so the
            // driver reports a device it cannot use.
            Window::Call(device) => device_read(*device, offset as u64).unwrap_or(0),
        }
    }

    fn write(&mut self, offset: usize, value: u32) {
        match self {
            // SAFETY: as in `read`.
            Window::Mapped(at) => unsafe {
                ((*at + offset as u64) as *mut u32).write_volatile(value);
            },
            Window::Call(device) => {
                let _ = device_write(*device, offset as u64, value);
            }
        }
    }

    // The configuration has byte fields: one byte access each, mapped or
    // by call (virtio 1.2, 4.2.2.2; ADR-0295 item 2).
    fn read8(&self, offset: usize) -> u8 {
        match self {
            // SAFETY: as in `read`; a byte access needs no alignment.
            Window::Mapped(at) => unsafe { ((at + offset as u64) as *const u8).read_volatile() },
            // A refused read gives 0, as in `read`.
            Window::Call(device) => device_read_width(*device, offset as u64, 1).unwrap_or(0) as u8,
        }
    }

    fn write8(&mut self, offset: usize, value: u8) {
        match self {
            // SAFETY: as in `read8`.
            Window::Mapped(at) => unsafe {
                ((*at + offset as u64) as *mut u8).write_volatile(value);
            },
            Window::Call(device) => {
                let _ = device_write_width(*device, offset as u64, u32::from(value), 1);
            }
        }
    }
}
