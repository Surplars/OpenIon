#![allow(dead_code)]

use core::ptr;
use kernel::mm::addr::{PAGE_SHIFT, PAGE_SIZE, PhysAddr, VirtAddr};
use kernel::mm::{MemPerms, MmError};

const PTE_V: u32 = 1 << 0;
const PTE_R: u32 = 1 << 1;
const PTE_W: u32 = 1 << 2;
const PTE_X: u32 = 1 << 3;
const PTE_U: u32 = 1 << 4;
const PTE_G: u32 = 1 << 5;
const PTE_A: u32 = 1 << 6;
const PTE_D: u32 = 1 << 7;

const LEVELS: usize = 2;
const PT_ENTRIES: usize = 1024;
const PPN_MASK: usize = 0x003f_ffff;

#[repr(C, align(4096))]
struct PageTable {
    entries: [u32; PT_ENTRIES],
}

impl PageTable {
    fn entry(&self, idx: usize) -> u32 {
        self.entries[idx]
    }

    fn set_entry(&mut self, idx: usize, val: u32) {
        self.entries[idx] = val;
    }

    fn is_valid(&self, idx: usize) -> bool {
        self.entries[idx] & PTE_V != 0
    }

    fn is_leaf(&self, idx: usize) -> bool {
        let entry = self.entries[idx];
        (entry & PTE_V != 0) && (entry & (PTE_R | PTE_W | PTE_X) != 0)
    }

    fn ppn(&self, idx: usize) -> usize {
        ((self.entries[idx] >> 10) as usize) & PPN_MASK
    }
}

fn perms_to_pte_bits(perms: MemPerms) -> u32 {
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

fn pte_to_perms(pte: u32) -> MemPerms {
    let mut perms = MemPerms::empty();
    if pte & PTE_R != 0 {
        perms |= MemPerms::READ;
    }
    if pte & PTE_W != 0 {
        perms |= MemPerms::WRITE;
    }
    if pte & PTE_X != 0 {
        perms |= MemPerms::EXECUTE;
    }
    perms
}

/// Sv32 page table manager.
/// Each instance owns a root page table and manages a 32-bit virtual address space.
pub struct Sv32PageTable {
    root: PhysAddr,
}

impl Sv32PageTable {
    /// Create a new Sv32 page table. `root` must point to a zeroed, page-aligned physical page.
    pub fn new(root: PhysAddr) -> Self {
        Self { root }
    }

    pub fn root_addr(&self) -> PhysAddr {
        self.root
    }

    fn root_table(&self) -> &mut PageTable {
        unsafe { &mut *(self.root.raw() as *mut PageTable) }
    }

    /// Return the satp value for this root table with ASID 0 and MODE=Sv32.
    pub fn satp_value(&self) -> usize {
        let ppn = self.root.raw() >> PAGE_SHIFT;
        (1usize << 31) | (ppn & PPN_MASK)
    }

    /// Map a single 4KB page: `vaddr` to `paddr` with the given permissions.
    /// Allocates the level-0 page table from `alloc` when needed.
    pub fn map_page(
        &mut self,
        vaddr: VirtAddr,
        paddr: PhysAddr,
        perms: MemPerms,
        alloc: &mut dyn FnMut() -> Option<PhysAddr>,
    ) -> Result<(), MmError> {
        if !vaddr.is_aligned(PAGE_SIZE) || !paddr.is_aligned(PAGE_SIZE) {
            return Err(MmError::InvalidAlignment);
        }

        let vpn = [(vaddr.raw() >> 12) & 0x3ff, (vaddr.raw() >> 22) & 0x3ff];
        let ppn = paddr.raw() >> PAGE_SHIFT;
        let pte_bits = perms_to_pte_bits(perms);

        let mut table = self.root_table();
        for level in (1..LEVELS).rev() {
            let idx = vpn[level];
            if !table.is_valid(idx) {
                let new_page = alloc().ok_or(MmError::OutOfMemory)?;
                unsafe {
                    ptr::write_bytes(new_page.as_mut_ptr::<u8>(), 0, PAGE_SIZE);
                }
                let child_ppn = new_page.raw() >> PAGE_SHIFT;
                table.set_entry(idx, PTE_V | (((child_ppn & PPN_MASK) as u32) << 10));
                table = unsafe { &mut *(new_page.as_mut_ptr()) };
            } else if table.is_leaf(idx) {
                return Err(MmError::AlreadyMapped);
            } else {
                let child_pa = PhysAddr::new(table.ppn(idx) << PAGE_SHIFT);
                table = unsafe { &mut *(child_pa.as_mut_ptr()) };
            }
        }

        let idx = vpn[0];
        if table.is_valid(idx) {
            return Err(MmError::AlreadyMapped);
        }
        table.set_entry(idx, pte_bits | (((ppn & PPN_MASK) as u32) << 10));
        Ok(())
    }

    /// Unmap a single 4KB page. Returns the physical address it was mapped to.
    pub fn unmap_page(&mut self, vaddr: VirtAddr) -> Result<PhysAddr, MmError> {
        if !vaddr.is_aligned(PAGE_SIZE) {
            return Err(MmError::InvalidAlignment);
        }

        let vpn = [(vaddr.raw() >> 12) & 0x3ff, (vaddr.raw() >> 22) & 0x3ff];

        let mut table = self.root_table();
        for level in (1..LEVELS).rev() {
            let idx = vpn[level];
            if !table.is_valid(idx) {
                return Err(MmError::NotMapped);
            }
            let child_pa = PhysAddr::new(table.ppn(idx) << PAGE_SHIFT);
            table = unsafe { &mut *(child_pa.as_mut_ptr()) };
        }

        let idx = vpn[0];
        if !table.is_valid(idx) {
            return Err(MmError::NotMapped);
        }
        let paddr = PhysAddr::new(table.ppn(idx) << PAGE_SHIFT);
        table.set_entry(idx, 0);
        Ok(paddr)
    }

    /// Translate a virtual address to a physical address by walking the page table.
    pub fn translate(&self, vaddr: VirtAddr) -> Option<PhysAddr> {
        let vpn = [(vaddr.raw() >> 12) & 0x3ff, (vaddr.raw() >> 22) & 0x3ff];

        let mut table = self.root_table();
        for level in (1..LEVELS).rev() {
            let idx = vpn[level];
            if !table.is_valid(idx) {
                return None;
            }
            if table.is_leaf(idx) {
                return None;
            }
            let child_pa = PhysAddr::new(table.ppn(idx) << PAGE_SHIFT);
            table = unsafe { &mut *(child_pa.as_mut_ptr()) };
        }

        let idx = vpn[0];
        if !table.is_valid(idx) {
            return None;
        }
        let offset = vaddr.raw() & (PAGE_SIZE - 1);
        Some(PhysAddr::new((table.ppn(idx) << PAGE_SHIFT) | offset))
    }

    /// Write the satp CSR to activate this page table.
    /// Only safe when running on RV32 S-mode with Sv32.
    pub unsafe fn activate_satp(&self) {
        unsafe {
            core::arch::asm!("csrw satp, {}", in(reg) self.satp_value());
            core::arch::asm!("sfence.vma");
        }
    }
}
