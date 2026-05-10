const MTIMECMP_OFFSET: usize = 0x4000;
const MTIME_OFFSET: usize = 0xbff8;

#[derive(Clone, Copy)]
pub struct Clint {
    base: usize,
}

impl Clint {
    pub const fn new(base: usize) -> Self {
        Self { base }
    }

    pub const fn base(&self) -> usize {
        self.base
    }

    pub const fn is_valid(&self) -> bool {
        self.base != 0
    }

    pub fn mtime(&self) -> u64 {
        if !self.is_valid() {
            return 0;
        }

        unsafe { ((self.base + MTIME_OFFSET) as *const u64).read_volatile() }
    }

    pub fn set_mtimecmp(&self, hartid: usize, deadline: u64) {
        if !self.is_valid() {
            return;
        }

        unsafe {
            ((self.base + MTIMECMP_OFFSET + hartid * core::mem::size_of::<u64>()) as *mut u64)
                .write_volatile(deadline);
        }
    }
}
