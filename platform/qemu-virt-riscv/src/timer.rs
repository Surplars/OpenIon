// 针对 QEMU Virt 的时钟驱动
use crate::QemuVirtRiscv;
use kernel::platform::Platform;

// M-mode: 直接配置 Core Local Interruptor (CLINT)
#[cfg(feature = "m-mode")]
pub mod clint {
    const CLINT_BASE: usize = 0x200_0000;
    const CLINT_MTIME: usize = CLINT_BASE + 0xBFF8;
    const CLINT_MTIMECMP: usize = CLINT_BASE + 0x4000;
    
    pub fn init_timer() {
        let hz = super::QemuVirtRiscv::config().systick_hz;
        let clock_freq = super::QemuVirtRiscv::config().cpu_freq_hz;
        let increment = clock_freq / hz;
        
        unsafe {
            // 获取当前机器物理定时器
            let mtime_ptr = CLINT_MTIME as *const u64;
            let mtimecmp_ptr = CLINT_MTIMECMP as *mut u64;
            
            let current_time = mtime_ptr.read_volatile();
            // 在未来抛出下一个 timer 中断
            mtimecmp_ptr.write_volatile(current_time + increment as u64);
            
            // 启用 Timer 机器模式中断
            riscv::register::mie::set_mtimer();
        }
    }
    
    pub fn set_next_tick() {
        let hz = super::QemuVirtRiscv::config().systick_hz;
        let clock_freq = super::QemuVirtRiscv::config().cpu_freq_hz;
        let increment = clock_freq / hz;
        
        unsafe {
            let mtimecmp_ptr = CLINT_MTIMECMP as *mut u64;
            let current_cmp = mtimecmp_ptr.read_volatile();
            mtimecmp_ptr.write_volatile(current_cmp + increment as u64);
        }
    }
}

// S-mode: 通过 SBI 或 Sstc (SBI Timer Extension) 处理时钟
#[cfg(feature = "s-mode")]
pub mod sbi_timer {
    use kernel::platform::Platform;

    // CLINT MTIME address (readable from S-mode via PMP)
    const CLINT_MTIME: usize = 0x0200_BFF8;

    pub fn init_timer() {
        set_next_tick();
        unsafe {
            riscv::register::sie::set_stimer();
        }
    }

    pub fn set_next_tick() {
        let hz = super::QemuVirtRiscv::config().systick_hz;
        let clock_freq = super::QemuVirtRiscv::config().cpu_freq_hz;
        let increment = clock_freq / hz;

        // Read time from CLINT memory-mapped register instead of CSR
        // (RustSBI 0.4.0 blocks S-mode direct CSR reads of `time`)
        let current_time = unsafe {
            (CLINT_MTIME as *const u64).read_volatile()
        };
        sbi_rt::set_timer(current_time + increment as u64);
    }
}
