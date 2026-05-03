// QEMU Virt 上的 PLIC (Platform-Level Interrupt Controller)

const PLIC_BASE: usize = 0x0c00_0000;
const PLIC_PRIORITY_BASE: usize = PLIC_BASE;
const PLIC_PENDING_BASE: usize = PLIC_BASE + 0x1000;
const PLIC_ENABLE_BASE: usize = PLIC_BASE + 0x2000;
const PLIC_THRESHOLD_BASE: usize = PLIC_BASE + 0x200000;
const PLIC_CLAIM_BASE: usize = PLIC_BASE + 0x200004;

pub fn init() {
    // 简单起见，这里假设我们运行在 M-mode 或 S-mode
    // QEMU Virt 的 Context:
    // 0: Hart 0 M-mode
    // 1: Hart 0 S-mode

    #[cfg(feature = "m-mode")]
    let context = 0;

    #[cfg(feature = "s-mode")]
    let context = 1;

    unsafe {
        // 设置所有中断的优先级阈值为 0 (接收所有大于 0 的中断)
        let threshold_ptr = (PLIC_THRESHOLD_BASE + context * 0x1000) as *mut u32;
        threshold_ptr.write_volatile(0);
    }
}

pub fn enable_irq(irq: u32, priority: u32) {
    #[cfg(feature = "m-mode")]
    let context = 0;

    #[cfg(feature = "s-mode")]
    let context = 1;

    unsafe {
        // 1. 设置优先级
        let priority_ptr = (PLIC_PRIORITY_BASE + (irq as usize) * 4) as *mut u32;
        priority_ptr.write_volatile(priority);

        // 2. 使能该中断
        let enable_ptr =
            (PLIC_ENABLE_BASE + context * 0x80 + ((irq as usize) / 32) * 4) as *mut u32;
        let mut val = enable_ptr.read_volatile();
        val |= 1 << (irq % 32);
        enable_ptr.write_volatile(val);
    }
}

pub fn claim() -> u32 {
    #[cfg(feature = "m-mode")]
    let context = 0;

    #[cfg(feature = "s-mode")]
    let context = 1;

    unsafe {
        let claim_ptr = (PLIC_CLAIM_BASE + context * 0x1000) as *mut u32;
        claim_ptr.read_volatile()
    }
}

pub fn complete(irq: u32) {
    #[cfg(feature = "m-mode")]
    let context = 0;

    #[cfg(feature = "s-mode")]
    let context = 1;

    unsafe {
        let claim_ptr = (PLIC_CLAIM_BASE + context * 0x1000) as *mut u32;
        claim_ptr.write_volatile(irq);
    }
}
