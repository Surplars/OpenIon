const PLIC_BASE: usize = kernel::generated_config::OPENION_QEMU_VIRT_RISCV_PLIC_BASE;
const PLIC_PRIORITY_BASE: usize = PLIC_BASE;
const PLIC_ENABLE_BASE: usize = PLIC_BASE + 0x2000;
const PLIC_THRESHOLD_BASE: usize = PLIC_BASE + 0x200000;
const PLIC_CLAIM_BASE: usize = PLIC_BASE + 0x200004;

fn context_id() -> usize {
    #[cfg(feature = "m-mode")]
    {
        0
    }

    #[cfg(feature = "s-mode")]
    {
        1
    }
}

pub fn init() {
    let context = context_id();
    unsafe {
        let threshold_ptr = (PLIC_THRESHOLD_BASE + context * 0x1000) as *mut u32;
        threshold_ptr.write_volatile(0);
    }
}

pub fn enable_irq(irq: u32, priority: u32) {
    let context = context_id();
    unsafe {
        let priority_ptr = (PLIC_PRIORITY_BASE + (irq as usize) * 4) as *mut u32;
        priority_ptr.write_volatile(priority);

        let enable_ptr =
            (PLIC_ENABLE_BASE + context * 0x80 + ((irq as usize) / 32) * 4) as *mut u32;
        let mut val = enable_ptr.read_volatile();
        val |= 1 << (irq % 32);
        enable_ptr.write_volatile(val);
    }
}

pub fn claim() -> u32 {
    let context = context_id();
    unsafe {
        let claim_ptr = (PLIC_CLAIM_BASE + context * 0x1000) as *mut u32;
        claim_ptr.read_volatile()
    }
}

pub fn complete(irq: u32) {
    let context = context_id();
    unsafe {
        let claim_ptr = (PLIC_CLAIM_BASE + context * 0x1000) as *mut u32;
        claim_ptr.write_volatile(irq);
    }
}
