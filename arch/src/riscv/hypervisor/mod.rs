pub mod entry;
pub mod gstage;
pub mod sbi;

use core::arch::global_asm;

// Include the guest entry/exit assembly
global_asm!(include_str!("entry.S"));

// Re-export
pub use entry::{guest_run, handle_vm_exit};
pub use gstage::GStagePageTable;

use core::arch::asm;

/// RISC-V H-extension vCPU context.
///
/// Stores the full guest state: 32 GPRs + VS-mode CSRs + H-extension CSRs.
/// Layout is `#[repr(C)]` so assembly can use fixed offsets.
#[repr(C)]
pub struct VCpuContext {
    // ---- Guest general-purpose registers (x0-x31) ----
    pub x: [usize; 32],

    // ---- Guest VS-mode CSRs ----
    pub vsstatus: usize,
    pub vsepc: usize,
    pub vscause: usize,
    pub vstval: usize,
    pub vsatp: usize,

    // ---- Host HS-mode CSRs ----
    pub hstatus: usize,
    pub hedeleg: usize,
    pub hideleg: usize,
    pub hgatp: usize,

    // ---- Metadata ----
    pub id: u32,
}

impl VCpuContext {
    pub const fn new() -> Self {
        Self {
            x: [0; 32],
            vsstatus: 0,
            vsepc: 0,
            vscause: 0,
            vstval: 0,
            vsatp: 0,
            hstatus: 0,
            hedeleg: 0,
            hideleg: 0,
            hgatp: 0,
            id: 0,
        }
    }

    pub fn set_entry(&mut self, entry: usize) {
        self.vsepc = entry;
    }

    pub fn set_sp(&mut self, sp: usize) {
        self.x[2] = sp;
    }

    pub fn set_reg(&mut self, reg: usize, val: usize) {
        if reg > 0 && reg < 32 {
            self.x[reg] = val;
        }
    }

    pub fn reg(&self, reg: usize) -> usize {
        if reg == 0 { 0 } else if reg < 32 { self.x[reg] } else { 0 }
    }

    /// Configure trap delegation: which traps the guest handles directly.
    pub fn setup_delegation(&mut self) {
        self.hedeleg = (1 << 0)   // Instruction address misaligned
                     | (1 << 1)   // Instruction access fault
                     | (1 << 2)   // Illegal instruction
                     | (1 << 3)   // Breakpoint
                     | (1 << 4)   // Load address misaligned
                     | (1 << 5)   // Load access fault
                     | (1 << 6)   // Store/AMO address misaligned
                     | (1 << 7)   // Store/AMO access fault
                     | (1 << 8)   // Environment call from VS-mode
                     | (1 << 12)  // Instruction page fault
                     | (1 << 13)  // Load page fault
                     | (1 << 15); // Store/AMO page fault

        self.hideleg = 0; // All interrupts stay with host
    }

    /// Write H-extension CSRs to activate this vCPU's configuration.
    pub unsafe fn load_csrs(&self) {
        unsafe {
            asm!("csrw hstatus, {}", in(reg) self.hstatus);
            asm!("csrw hedeleg, {}", in(reg) self.hedeleg);
            asm!("csrw hideleg, {}", in(reg) self.hideleg);
            asm!("csrw hgatp, {}", in(reg) self.hgatp);
            asm!("csrw vsstatus, {}", in(reg) self.vsstatus);
            asm!("csrw vsepc, {}", in(reg) self.vsepc);
            asm!("csrw vscause, {}", in(reg) 0usize);
            asm!("csrw vstval, {}", in(reg) 0usize);
            asm!("hfence.gvma");
        }
    }

    /// Read VS-mode CSRs after guest exit.
    pub unsafe fn save_csrs(&mut self) {
        unsafe {
            asm!("csrr {}, vsstatus", out(reg) self.vsstatus);
            asm!("csrr {}, vsepc", out(reg) self.vsepc);
            asm!("csrr {}, vscause", out(reg) self.vscause);
            asm!("csrr {}, vstval", out(reg) self.vstval);
        }
    }
}
