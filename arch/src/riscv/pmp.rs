use kernel::mm::addr::PhysAddr;
use kernel::mm::{MemPerms, MmError};

/// Number of PMP entries supported (up to 16 on most RISC-V implementations).
const PMP_MAX_ENTRIES: usize = 16;

// PMP configuration register bits (per entry, in pmpcfg0..3)
const PMPCFG_R: u8 = 1 << 0;
const PMPCFG_W: u8 = 1 << 1;
const PMPCFG_X: u8 = 1 << 2;
const PMPCFG_A: u8 = 0b11 << 3; // Address matching mode
const PMPCFG_L: u8 = 1 << 7; // Lock bit

// Address matching modes (in the A field)
const PMPCFG_A_OFF: u8 = 0b00 << 3;
const PMPCFG_A_TOR: u8 = 0b01 << 3; // Top of Range
const PMPCFG_A_NA4: u8 = 0b10 << 3; // Naturally aligned 4-byte
const PMPCFG_A_NAPOT: u8 = 0b11 << 3; // Naturally aligned power-of-two

fn perms_to_pmpcfg(perms: MemPerms) -> u8 {
    let mut cfg = PMPCFG_A_NAPOT; // Use NAPOT by default
    if perms.contains(MemPerms::READ) {
        cfg |= PMPCFG_R;
    }
    if perms.contains(MemPerms::WRITE) {
        cfg |= PMPCFG_W;
    }
    if perms.contains(MemPerms::EXECUTE) {
        cfg |= PMPCFG_X;
    }
    cfg
}

/// Configure a PMP entry with NAPOT (naturally aligned power-of-two) addressing.
///
/// `addr` must be the base of the region, `size` must be a power of two >= 16 bytes.
/// The NAPOT encoding is: pmpaddr = (base >> 2) | ((size >> 3) - 1)
fn napot_encode(base: PhysAddr, size: usize) -> Result<usize, MmError> {
    if size < 16 || !size.is_power_of_two() {
        return Err(MmError::InvalidRegion);
    }
    if !base.is_aligned(size) {
        return Err(MmError::InvalidAlignment);
    }
    Ok((base.raw() >> 2) | ((size >> 3) - 1))
}

/// RISC-V PMP manager. Tracks which PMP entries are in use.
pub struct PmpManager {
    count: usize, // Number of PMP entries used
}

impl PmpManager {
    pub const fn new() -> Self {
        Self { count: 0 }
    }

    pub fn used_entries(&self) -> usize {
        self.count
    }

    /// Configure PMP entry `idx` with a NAPOT region.
    /// Safety: writes to PMP CSRs, which can affect memory access.
    unsafe fn write_pmp_entry(&self, idx: usize, addr_val: usize, cfg: u8) {
        // Write pmpaddr first
        unsafe {
            match idx {
                0 => core::arch::asm!("csrw pmpaddr0, {}", in(reg) addr_val),
                1 => core::arch::asm!("csrw pmpaddr1, {}", in(reg) addr_val),
                2 => core::arch::asm!("csrw pmpaddr2, {}", in(reg) addr_val),
                3 => core::arch::asm!("csrw pmpaddr3, {}", in(reg) addr_val),
                4 => core::arch::asm!("csrw pmpaddr4, {}", in(reg) addr_val),
                5 => core::arch::asm!("csrw pmpaddr5, {}", in(reg) addr_val),
                6 => core::arch::asm!("csrw pmpaddr6, {}", in(reg) addr_val),
                7 => core::arch::asm!("csrw pmpaddr7, {}", in(reg) addr_val),
                8 => core::arch::asm!("csrw pmpaddr8, {}", in(reg) addr_val),
                9 => core::arch::asm!("csrw pmpaddr9, {}", in(reg) addr_val),
                10 => core::arch::asm!("csrw pmpaddr10, {}", in(reg) addr_val),
                11 => core::arch::asm!("csrw pmpaddr11, {}", in(reg) addr_val),
                12 => core::arch::asm!("csrw pmpaddr12, {}", in(reg) addr_val),
                13 => core::arch::asm!("csrw pmpaddr13, {}", in(reg) addr_val),
                14 => core::arch::asm!("csrw pmpaddr14, {}", in(reg) addr_val),
                15 => core::arch::asm!("csrw pmpaddr15, {}", in(reg) addr_val),
                _ => {}
            }
        }

        // Write pmpcfg: on RV64, each pmpcfg register holds 8 entries (8 bits each)
        // pmpcfg0 = entries 0-7, pmpcfg2 = entries 8-15
        // (odd-numbered pmpcfg registers don't exist on RV64)
        let cfg_group = idx / 8;
        let cfg_byte = idx % 8;

        // Read current pmpcfg, modify the target byte, write back
        let current: usize;
        unsafe {
            match cfg_group {
                0 => core::arch::asm!("csrr {}, pmpcfg0", out(reg) current),
                1 => core::arch::asm!("csrr {}, pmpcfg2", out(reg) current),
                _ => return,
            }
        }

        let mask = 0xFFusize << (cfg_byte * 8);
        let new_val = (current & !mask) | ((cfg as usize) << (cfg_byte * 8));

        unsafe {
            match cfg_group {
                0 => core::arch::asm!("csrw pmpcfg0, {}", in(reg) new_val),
                1 => core::arch::asm!("csrw pmpcfg2, {}", in(reg) new_val),
                _ => {}
            }
        }
    }

    /// Add a PMP region with the given permissions.
    /// Returns `MmError` if out of PMP entries or invalid parameters.
    pub fn add_region(
        &mut self,
        base: PhysAddr,
        size: usize,
        perms: MemPerms,
    ) -> Result<(), MmError> {
        if self.count >= PMP_MAX_ENTRIES {
            return Err(MmError::OutOfMemory);
        }

        let addr_val = napot_encode(base, size)?;
        let cfg = perms_to_pmpcfg(perms);

        unsafe {
            self.write_pmp_entry(self.count, addr_val, cfg);
        }

        self.count += 1;
        Ok(())
    }

    /// Activate PMP by enabling all configured entries.
    /// Safety: must be called from M-mode or S-mode (via SBI) with correct setup.
    pub unsafe fn activate(&self) {
        // PMP entries are already written in add_region.
        // The entries take effect when the next memory access occurs.
        // We just need to ensure the address space is flushed if needed.
        unsafe {
            core::arch::asm!("sfence.vma");
        }
    }
}
