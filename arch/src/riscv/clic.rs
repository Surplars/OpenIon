/// CLIC (Core Local Interrupt Controller) register definitions and basic operations.
///
/// This module provides the low-level CLIC register access layer.
/// Platform-specific configuration (init, enable, etc.) should be done
/// at the platform level using these primitives.

const CLICCFG_OFFSET: usize = 0x0000;
const CLICINT_BASE: usize = 0x1000;
const CLICINT_STRIDE: usize = 4;

// CLICINT register offsets within each interrupt entry
pub const CLICINTIP: usize = 0;
pub const CLICINTIE: usize = 1;
pub const CLICINTATTR: usize = 2;
pub const CLICINTCTL: usize = 3;

#[derive(Clone, Copy)]
pub struct Clic {
    base: usize,
}

impl Clic {
    pub const fn new(base: usize) -> Self {
        Self { base }
    }

    pub const fn base(&self) -> usize {
        self.base
    }

    pub const fn is_valid(&self) -> bool {
        self.base != 0
    }

    /// Write to CLICCFG register.
    pub fn set_config(&self, value: u8) {
        if !self.is_valid() {
            return;
        }
        unsafe {
            write_u8(self.base + CLICCFG_OFFSET, value);
        }
    }

    /// Read interrupt pending bit.
    pub fn is_pending(&self, irq: u32) -> bool {
        if !self.is_valid() || irq == 0 {
            return false;
        }
        unsafe {
            let addr = self.base + CLICINT_BASE + irq as usize * CLICINT_STRIDE + CLICINTIP;
            read_u8(addr) != 0
        }
    }

    /// Clear interrupt pending bit.
    pub fn clear_pending(&self, irq: u32) {
        if !self.is_valid() || irq == 0 {
            return;
        }
        unsafe {
            let addr = self.base + CLICINT_BASE + irq as usize * CLICINT_STRIDE + CLICINTIP;
            write_u8(addr, 0);
        }
    }

    /// Set interrupt enable bit.
    pub fn set_enable(&self, irq: u32, enabled: bool) {
        if !self.is_valid() || irq == 0 {
            return;
        }
        unsafe {
            let addr = self.base + CLICINT_BASE + irq as usize * CLICINT_STRIDE + CLICINTIE;
            write_u8(addr, if enabled { 1 } else { 0 });
        }
    }

    /// Read interrupt enable bit.
    pub fn is_enabled(&self, irq: u32) -> bool {
        if !self.is_valid() || irq == 0 {
            return false;
        }
        unsafe {
            let addr = self.base + CLICINT_BASE + irq as usize * CLICINT_STRIDE + CLICINTIE;
            read_u8(addr) != 0
        }
    }

    /// Set interrupt attributes (edge/level, etc.).
    pub fn set_attr(&self, irq: u32, attr: u8) {
        if !self.is_valid() || irq == 0 {
            return;
        }
        unsafe {
            let addr = self.base + CLICINT_BASE + irq as usize * CLICINT_STRIDE + CLICINTATTR;
            write_u8(addr, attr);
        }
    }

    /// Set interrupt priority/control level.
    pub fn set_ctl(&self, irq: u32, ctl: u8) {
        if !self.is_valid() || irq == 0 {
            return;
        }
        unsafe {
            let addr = self.base + CLICINT_BASE + irq as usize * CLICINT_STRIDE + CLICINTCTL;
            write_u8(addr, ctl);
        }
    }

    /// Initialize a single interrupt entry with default settings.
    pub fn init_irq(&self, irq: u32) {
        self.set_attr(irq, 0);
        self.set_ctl(irq, 0xff);
        self.set_enable(irq, true);
    }
}

unsafe fn read_u8(addr: usize) -> u8 {
    unsafe { (addr as *const u8).read_volatile() }
}

unsafe fn write_u8(addr: usize, value: u8) {
    unsafe { (addr as *mut u8).write_volatile(value) }
}
