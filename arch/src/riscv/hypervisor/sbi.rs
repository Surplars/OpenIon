use super::VCpuContext;
use core::arch::asm;

/// SBI extension IDs
const SBI_EXT_SET_TIMER: usize = 0x00;
const SBI_EXT_CONSOLE_PUTCHAR: usize = 0x01;
const SBI_EXT_CONSOLE_GETCHAR: usize = 0x02;
const SBI_EXT_CLEAR_IPI: usize = 0x03;
const SBI_EXT_SEND_IPI: usize = 0x04;
const SBI_EXT_REMOTE_FENCE_I: usize = 0x05;
const SBI_EXT_REMOTE_SFENCE_VMA: usize = 0x06;
const SBI_EXT_REMOTE_SFENCE_VMA_ASID: usize = 0x07;
const SBI_EXT_SHUTDOWN: usize = 0x08;

/// SBI legacy extension IDs (passed in a7)
const SBI_EXT_TIME: usize = 0x54494D45;
const SBI_EXT_IPI: usize = 0x735049;
const SBI_EXT_RFENCE: usize = 0x52464E43;
const SBI_EXT_HSM: usize = 0x48534D;
const SBI_EXT_SRST: usize = 0x53525354;
const SBI_EXT_DBCN: usize = 0x4442434E;
const SBI_EXT_PMU: usize = 0x504D55;

/// SBI return codes
const SBI_SUCCESS: usize = 0;
const SBI_ERR_NOT_SUPPORTED: usize = usize::MAX; // -1 in unsigned

/// Handle an SBI ecall from the guest.
/// a6 = function ID, a7 = extension ID, a0-a5 = args.
/// Result returned in a0 (error) and a1 (value).
pub fn handle_ecall(ctx: &mut VCpuContext) {
    let eid = ctx.reg(17); // a7 = extension ID
    let fid = ctx.reg(16); // a6 = function ID
    let a0 = ctx.reg(10);  // a0
    let _a1 = ctx.reg(11);  // a1
    let _a2 = ctx.reg(12); // a2
    let _a3 = ctx.reg(13); // a3
    let _a4 = ctx.reg(14); // a4
    let _a5 = ctx.reg(15); // a5

    match eid {
        // Legacy timer extension
        SBI_EXT_TIME => {
            unsafe {
                asm!("csrw vstimecmp, {}", in(reg) a0);
            }
            ctx.set_reg(10, SBI_SUCCESS); // a0 = error
            ctx.set_reg(11, 0);           // a1 = value
        }

        // Legacy console putchar
        SBI_EXT_CONSOLE_PUTCHAR | SBI_EXT_DBCN => {
            // Forward to host console
            if let Some(console) = kernel::log::console() {
                console.putc(a0 as u8);
            }
            ctx.set_reg(10, SBI_SUCCESS);
            ctx.set_reg(11, 0);
        }

        // Legacy console getchar
        SBI_EXT_CONSOLE_GETCHAR => {
            // Try to read from host UART RX buffer
            let ch = kernel::driver::char::pop_from_rx_buf();
            match ch {
                Some(c) => {
                    ctx.set_reg(10, SBI_SUCCESS);
                    ctx.set_reg(11, c as usize);
                }
                None => {
                    ctx.set_reg(10, SBI_ERR_NOT_SUPPORTED);
                    ctx.set_reg(11, 0);
                }
            }
        }

        // Remote fence
        SBI_EXT_RFENCE | SBI_EXT_REMOTE_SFENCE_VMA | SBI_EXT_REMOTE_SFENCE_VMA_ASID => {
            // No-op for single-hart guest
            ctx.set_reg(10, SBI_SUCCESS);
            ctx.set_reg(11, 0);
        }

        // IPI
        SBI_EXT_IPI | SBI_EXT_SEND_IPI | SBI_EXT_CLEAR_IPI => {
            // No-op for single-hart guest
            ctx.set_reg(10, SBI_SUCCESS);
            ctx.set_reg(11, 0);
        }

        // HSM (Hart State Management) — start/stop
        SBI_EXT_HSM => {
            match fid {
                0 => {
                    // sbi_hart_start — we just succeed, the guest manages its own harts
                    ctx.set_reg(10, SBI_SUCCESS);
                    ctx.set_reg(11, 0);
                }
                1 => {
                    // sbi_hart_stop
                    ctx.set_reg(10, SBI_SUCCESS);
                    ctx.set_reg(11, 0);
                }
                _ => {
                    ctx.set_reg(10, SBI_ERR_NOT_SUPPORTED);
                    ctx.set_reg(11, 0);
                }
            }
        }

        // SRST (System Reset)
        SBI_EXT_SRST => {
            // Guest wants to shutdown/reboot — exit to host
            ctx.set_reg(10, SBI_SUCCESS);
            ctx.set_reg(11, 0);
        }

        // PMU
        SBI_EXT_PMU => {
            ctx.set_reg(10, SBI_ERR_NOT_SUPPORTED);
            ctx.set_reg(11, 0);
        }

        _ => {
            // Unknown extension
            ctx.set_reg(10, SBI_ERR_NOT_SUPPORTED);
            ctx.set_reg(11, 0);
        }
    }
}
