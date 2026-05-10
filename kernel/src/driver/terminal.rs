use super::{DriverErr, DriverResult, GenericDeviceConfig, char::CharDevice};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalSize {
    pub columns: u16,
    pub rows: u16,
}

impl TerminalSize {
    pub const fn new(columns: u16, rows: u16) -> Self {
        Self { columns, rows }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalMode {
    Raw,
    Cooked,
}

pub trait TerminalDevice: CharDevice {
    fn size(&self) -> TerminalSize {
        TerminalSize::new(80, 25)
    }

    fn mode(&self) -> TerminalMode {
        TerminalMode::Raw
    }

    fn set_mode(&self, _mode: TerminalMode) -> DriverResult<()> {
        Err(DriverErr::NotSupported)
    }

    fn write_str(&self, s: &str) -> DriverResult<usize> {
        self.write_buffer(s.as_bytes())
    }

    fn write_line(&self, s: &str) -> DriverResult<usize> {
        let mut written = self.write_str(s)?;
        if self.write_byte(b'\r').is_ok() {
            written += 1;
        }
        if self.write_byte(b'\n').is_ok() {
            written += 1;
        }
        Ok(written)
    }

    fn clear_screen(&self) -> DriverResult<usize> {
        self.write_str("\x1b[2J\x1b[H")
    }
}

pub type DynTerminalDevice = dyn TerminalDevice<Config = GenericDeviceConfig, Error = DriverErr>;
