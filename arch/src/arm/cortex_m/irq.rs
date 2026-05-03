#[unsafe(no_mangle)]
pub extern "C" fn nmi_handler() {
    kernel::kerror!("NMI occurred!");
    loop {}
}

#[unsafe(no_mangle)]
pub extern "C" fn memmanage_handler() {
    kernel::kerror!("Memory Management Fault occurred!");
    loop {}
}

#[unsafe(no_mangle)]
pub extern "C" fn busfault_handler() {
    kernel::kerror!("BusFault occurred!");
    loop {}
}

#[unsafe(no_mangle)]
pub extern "C" fn hardfault_handler(frame: &ExceptionFrame) {
    unsafe {
        let hfsr = *(0xE000_ED2C as *const u32);
        let cfsr = *(0xE000_ED28 as *const u32);
        let mmfar = *(0xE000_ED34 as *const u32);
        let bfar = *(0xE000_ED38 as *const u32);

        kernel::kerror!("=== HardFault Diagnostic ===");
        kernel::kerror!("HFSR: 0x{:08X}", hfsr);
        kernel::kerror!("CFSR: 0x{:08X}", cfsr);
        kernel::kerror!("MMFAR: 0x{:08X}", mmfar);
        kernel::kerror!("BFAR: 0x{:08X}", bfar);
        kernel::kerror!("PC: 0x{:08X}", frame.pc);
        kernel::kerror!("LR: 0x{:08X}", frame.lr);
    }
    loop {}
}

#[unsafe(no_mangle)]
pub extern "C" fn systick_handler() {
    kernel::timer::tick();
    kernel::sched::Scheduler::tick_update();
    if kernel::sched::Scheduler::schedule() {
        super::context::yield_cpu();
    }
}

pub fn enable() {
    unsafe { cortex_m::interrupt::enable() };
}

pub fn disable() {
    cortex_m::interrupt::disable();
}

#[repr(C)]
pub struct ExceptionFrame {
    pub r0: u32,
    pub r1: u32,
    pub r2: u32,
    pub r3: u32,
    pub r12: u32,
    pub lr: u32,
    pub pc: u32,
    pub xpsr: u32,
}
