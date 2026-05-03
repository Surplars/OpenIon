use super::VCpuContext;
use super::sbi;
use core::arch::asm;

unsafe extern "C" {
    /// Enters the guest. Returns scause on exit.
    pub fn guest_run(vcpu: *mut VCpuContext) -> usize;
}

/// Rust-side VM exit handler. Called from assembly after saving guest state.
/// Returns 0 to resume guest, nonzero to exit to host (value = exit reason).
#[unsafe(no_mangle)]
pub extern "C" fn handle_vm_exit(vcpu: *mut VCpuContext, scause: usize, stval: usize) -> usize {
    let ctx = unsafe { &mut *vcpu };
    ctx.vstval = stval;

    // Check if this is an interrupt (highest bit set)
    if scause & (1 << (usize::BITS - 1)) != 0 {
        let irq = scause & !(1 << (usize::BITS - 1));
        handle_interrupt(ctx, irq)
    } else {
        handle_exception(ctx, scause, stval)
    }
}

fn handle_interrupt(_ctx: &mut VCpuContext, irq: usize) -> usize {
    match irq {
        // Supervisor software interrupt — forward to guest
        1 => {
            // Set VSSIP pending in hvip
            unsafe {
                let mut hvip: usize;
                asm!("csrr {}, hvip", out(reg) hvip);
                hvip |= 1 << 2; // VSSIP
                asm!("csrw hvip, {}", in(reg) hvip);
            }
            0 // Resume guest
        }
        // Supervisor timer interrupt — forward to guest
        5 => {
            unsafe {
                let mut hvip: usize;
                asm!("csrr {}, hvip", out(reg) hvip);
                hvip |= 1 << 6; // VSTIP
                asm!("csrw hvip, {}", in(reg) hvip);
            }
            0 // Resume guest
        }
        // Supervisor external interrupt — exit to host for PLIC handling
        9 => {
            // Exit to host with this IRQ as reason
            irq
        }
        _ => 0, // Resume guest for unknown interrupts
    }
}

fn handle_exception(ctx: &mut VCpuContext, cause: usize, _stval: usize) -> usize {
    match cause {
        // Environment call from VS-mode (guest SBI call)
        8 => {
            sbi::handle_ecall(ctx);
            // Advance guest PC past the ecall instruction
            ctx.vsepc = ctx.vsepc.wrapping_add(4);
            0 // Resume guest
        }
        // Instruction address misaligned
        0 => cause,
        // Illegal instruction
        2 => cause,
        // Load page fault
        13 => cause,
        // Store page fault
        15 => cause,
        // Guest instruction page fault
        12 => cause,
        _ => cause,
    }
}
