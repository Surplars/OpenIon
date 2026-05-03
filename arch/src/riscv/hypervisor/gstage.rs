use core::ptr;
use kernel::mm::addr::{PAGE_SHIFT, PAGE_SIZE, PhysAddr};
use kernel::mm::{MemPerms, MmError};

const PTE_V: u64 = 1 << 0;
const PTE_R: u64 = 1 << 1;
const PTE_W: u64 = 1 << 2;
const PTE_X: u64 = 1 << 3;
const PTE_U: u64 = 1 << 4;
const PTE_A: u64 = 1 << 6;
const PTE_D: u64 = 1 << 7;

const GST_LEVELS: usize = 3;
const PT_ENTRIES: usize = 512;

#[repr(C, align(4096))]
struct PageTable {
    entries: [u64; PT_ENTRIES],
}

fn perms_to_pte(perms: MemPerms) -> u64 {
    let mut bits = PTE_V | PTE_A | PTE_D;
    if perms.contains(MemPerms::READ) {
        bits |= PTE_R;
    }
    if perms.contains(MemPerms::WRITE) {
        bits |= PTE_W;
    }
    if perms.contains(MemPerms::EXECUTE) {
        bits |= PTE_X;
    }
    bits
}

/// G-stage page table manager (Sv39x4).
///
/// Maps Guest Physical Addresses (GPA) → Host Physical Addresses (HPA).
/// Sv39x4 uses a 41-bit GPA space (2 TB) with 3 levels and 4KB pages.
/// The first level has 2048 entries (11 bits) instead of 512 (9 bits).
pub struct GStagePageTable {
    root: PhysAddr,
}

impl GStagePageTable {
    pub fn new(root: PhysAddr) -> Self {
        Self { root }
    }

    pub fn root_addr(&self) -> PhysAddr {
        self.root
    }

    /// Set the hgatp value for this page table (Sv39x4 mode = 8).
    pub fn hgatp_value(&self) -> usize {
        let ppn = self.root.raw() >> PAGE_SHIFT;
        // mode 8 = Sv39x4, ASID = 0
        (8usize << 60) | (ppn & 0xFFF_FFFF_FFFF)
    }

    /// Map a guest physical page to a host physical page.
    ///
    /// Sv39x4 level structure:
    ///   Level 2: VPN[2] = bits [40:30] (11 bits, 2048 entries)
    ///   Level 1: VPN[1] = bits [29:21] (9 bits, 512 entries)
    ///   Level 0: VPN[0] = bits [20:12] (9 bits, 512 entries)
    pub fn map_page(
        &mut self,
        gpa: PhysAddr,
        hpa: PhysAddr,
        perms: MemPerms,
        alloc: &mut dyn FnMut() -> Option<PhysAddr>,
    ) -> Result<(), MmError> {
        if !gpa.is_aligned(PAGE_SIZE) || !hpa.is_aligned(PAGE_SIZE) {
            return Err(MmError::InvalidAlignment);
        }

        let addr = gpa.raw();
        // Sv39x4: VPN[2] is 11 bits (bits 40:30)
        let vpn = [
            (addr >> 12) & 0x1FF, // VPN[0]
            (addr >> 21) & 0x1FF, // VPN[1]
            (addr >> 30) & 0x7FF, // VPN[2] — 11 bits for Sv39x4
        ];

        let ppn = hpa.raw() >> PAGE_SHIFT;
        let pte_bits = perms_to_pte(perms);

        let root_table = unsafe { &mut *(self.root.raw() as *mut PageTable) };

        // Walk from level 2 → level 1 → level 0
        let mut table = root_table;
        for level in (1..GST_LEVELS).rev() {
            let idx = vpn[level];
            if (table.entries[idx] & PTE_V) == 0 {
                let new_page = alloc().ok_or(MmError::OutOfMemory)?;
                unsafe {
                    ptr::write_bytes(new_page.as_mut_ptr::<u8>(), 0, PAGE_SIZE);
                }
                let child_ppn = new_page.raw() >> PAGE_SHIFT;
                table.entries[idx] = PTE_V | ((child_ppn as u64 & 0xFFF_FFFF_FFFF) << 10);
                table = unsafe { &mut *(new_page.as_mut_ptr()) };
            } else if (table.entries[idx] & (PTE_R | PTE_W | PTE_X)) != 0 {
                return Err(MmError::AlreadyMapped);
            } else {
                let child_ppn = ((table.entries[idx] >> 10) & 0xFFF_FFFF_FFFF) as usize;
                table = unsafe { &mut *((child_ppn << PAGE_SHIFT) as *mut PageTable) };
            }
        }

        let idx = vpn[0];
        if (table.entries[idx] & PTE_V) != 0 {
            return Err(MmError::AlreadyMapped);
        }
        table.entries[idx] = pte_bits | (((ppn as u64) & 0xFFF_FFFF_FFFF) << 10);
        Ok(())
    }

    /// Map a contiguous range of guest physical memory.
    pub fn map_region(
        &mut self,
        gpa_base: PhysAddr,
        hpa_base: PhysAddr,
        size: usize,
        perms: MemPerms,
        alloc: &mut dyn FnMut() -> Option<PhysAddr>,
    ) -> Result<(), MmError> {
        let pages = (size + PAGE_SIZE - 1) / PAGE_SIZE;
        for i in 0..pages {
            let gpa = gpa_base + i * PAGE_SIZE;
            let hpa = hpa_base + i * PAGE_SIZE;
            self.map_page(gpa, hpa, perms, alloc)?;
        }
        Ok(())
    }

    /// Translate a GPA to HPA by walking the G-stage page table.
    pub fn translate(&self, gpa: PhysAddr) -> Option<PhysAddr> {
        let addr = gpa.raw();
        let vpn = [
            (addr >> 12) & 0x1FF,
            (addr >> 21) & 0x1FF,
            (addr >> 30) & 0x7FF,
        ];

        let mut table = unsafe { &*(self.root.raw() as *const PageTable) };

        for level in (1..GST_LEVELS).rev() {
            let idx = vpn[level];
            if (table.entries[idx] & PTE_V) == 0 {
                return None;
            }
            if (table.entries[idx] & (PTE_R | PTE_W | PTE_X)) != 0 {
                // Superpage (not supported at intermediate levels for now)
                if level > 0 {
                    return None;
                }
            }
            let child_ppn = ((table.entries[idx] >> 10) & 0xFFF_FFFF_FFFF) as usize;
            table = unsafe { &*((child_ppn << PAGE_SHIFT) as *const PageTable) };
        }

        let idx = vpn[0];
        if (table.entries[idx] & PTE_V) == 0 {
            return None;
        }
        let ppn = ((table.entries[idx] >> 10) & 0xFFF_FFFF_FFFF) as usize;
        let offset = addr & 0xFFF;
        Some(PhysAddr::new((ppn << PAGE_SHIFT) | offset))
    }
}
