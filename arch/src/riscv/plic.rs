const PRIORITY_OFFSET: usize = 0x0;
const ENABLE_OFFSET: usize = 0x2000;
const THRESHOLD_OFFSET: usize = 0x200000;
const CLAIM_OFFSET: usize = 0x200004;

#[derive(Clone, Copy)]
pub struct Plic {
    base: usize,
}

impl Plic {
    pub const fn new(base: usize) -> Self {
        Self { base }
    }

    pub const fn base(&self) -> usize {
        self.base
    }

    pub const fn is_valid(&self) -> bool {
        self.base != 0
    }

    pub fn init_context(&self, context: usize) {
        if !self.is_valid() {
            return;
        }

        unsafe {
            let threshold_ptr = (self.base + THRESHOLD_OFFSET + context * 0x1000) as *mut u32;
            threshold_ptr.write_volatile(0);
        }
    }

    pub fn enable_irq(&self, context: usize, irq: u32, priority: u32) {
        if !self.is_valid() || irq == 0 {
            return;
        }

        unsafe {
            let priority_ptr = (self.base + PRIORITY_OFFSET + (irq as usize) * 4) as *mut u32;
            priority_ptr.write_volatile(priority);

            let enable_ptr =
                (self.base + ENABLE_OFFSET + context * 0x80 + ((irq as usize) / 32) * 4)
                    as *mut u32;
            let mut val = enable_ptr.read_volatile();
            val |= 1 << (irq % 32);
            enable_ptr.write_volatile(val);
        }
    }

    pub fn claim(&self, context: usize) -> u32 {
        if !self.is_valid() {
            return 0;
        }

        unsafe {
            let claim_ptr = (self.base + CLAIM_OFFSET + context * 0x1000) as *mut u32;
            claim_ptr.read_volatile()
        }
    }

    pub fn complete(&self, context: usize, irq: u32) {
        if !self.is_valid() || irq == 0 {
            return;
        }

        unsafe {
            let claim_ptr = (self.base + CLAIM_OFFSET + context * 0x1000) as *mut u32;
            claim_ptr.write_volatile(irq);
        }
    }
}
