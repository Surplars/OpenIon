use core::ptr;
use kernel::mm::addr::{PAGE_SHIFT, PAGE_SIZE, PhysAddr, VirtAddr};
use kernel::mm::{MemPerms, MmError};

const PTE_V: u64 = 1 << 0;
const PTE_R: u64 = 1 << 1;
const PTE_W: u64 = 1 << 2;
const PTE_X: u64 = 1 << 3;
const PTE_U: u64 = 1 << 4;
const PTE_G: u64 = 1 << 5;
const PTE_A: u64 = 1 << 6;
const PTE_D: u64 = 1 << 7;

const LEVELS: usize = 3; // Sv39: 3-level page table
const PT_ENTRIES: usize = 512; // 2^9 entries per page table

#[repr(C, align(4096))]
struct PageTable {
    entries: [u64; PT_ENTRIES],
}

impl PageTable {
    const fn new() -> Self {
        Self {
            entries: [0; PT_ENTRIES],
        }
    }

    fn entry(&self, idx: usize) -> u64 {
        self.entries[idx]
    }

    fn set_entry(&mut self, idx: usize, val: u64) {
        self.entries[idx] = val;
    }

    fn is_valid(&self, idx: usize) -> bool {
        self.entries[idx] & PTE_V != 0
    }

    fn is_leaf(&self, idx: usize) -> bool {
        let e = self.entries[idx];
        (e & PTE_V != 0) && (e & (PTE_R | PTE_W | PTE_X) != 0)
    }

    fn ppn(&self, idx: usize) -> usize {
        ((self.entries[idx] >> 10) & 0xFFF_FFFF_FFFF) as usize
    }

    fn set_ppn(&mut self, idx: usize, ppn: usize) {
        self.entries[idx] = (self.entries[idx] & 0x3FF) | ((ppn as u64 & 0xFFF_FFFF_FFFF) << 10);
    }
}

fn perms_to_pte_bits(perms: MemPerms) -> u64 {
    let mut bits = PTE_V | PTE_A | PTE_D | PTE_U; // User-accessible by default
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

fn pte_to_perms(pte: u64) -> MemPerms {
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

/// Sv39 page table manager.
/// Each instance owns a root page table and manages a 39-bit (512 GB) virtual address space.
pub struct Sv39PageTable {
    root: PhysAddr,
}

impl Sv39PageTable {
    /// Create a new Sv39 page table. `root` must point to a zeroed, page-aligned physical page.
    pub fn new(root: PhysAddr) -> Self {
        Self { root }
    }

    pub fn root_addr(&self) -> PhysAddr {
        self.root
    }

    fn root_table(&self) -> &mut PageTable {
        unsafe { &mut *(self.root.raw() as *mut PageTable) }
    }

    /// Map a single 4KB page: vaddr → paddr with given permissions.
    /// Allocates intermediate page tables from `alloc` as needed.
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

        let vpn = [
            (vaddr.raw() >> 12) & 0x1FF, // VPN[0]
            (vaddr.raw() >> 21) & 0x1FF, // VPN[1]
            (vaddr.raw() >> 30) & 0x1FF, // VPN[2]
        ];

        let ppn = paddr.raw() >> PAGE_SHIFT;
        let pte_bits = perms_to_pte_bits(perms);

        // Walk from root to level 1 (level-2 entry → level-1 entry → leaf)
        let mut table = self.root_table();

        for level in (1..LEVELS).rev() {
            let idx = vpn[level];
            if !table.is_valid(idx) {
                // Allocate a new intermediate page table
                let new_page = alloc().ok_or(MmError::OutOfMemory)?;
                // Zero it
                unsafe {
                    ptr::write_bytes(new_page.as_mut_ptr::<u8>(), 0, PAGE_SIZE);
                }
                let child_ppn = new_page.raw() >> PAGE_SHIFT;
                table.set_entry(idx, PTE_V | ((child_ppn as u64 & 0xFFF_FFFF_FFFF) << 10));
                table = unsafe { &mut *(new_page.as_mut_ptr()) };
            } else if table.is_leaf(idx) {
                // A leaf PTE at a higher level = superpage. For now reject (we only do 4K pages).
                return Err(MmError::AlreadyMapped);
            } else {
                let child_ppn = table.ppn(idx);
                let child_pa = PhysAddr::new(child_ppn << PAGE_SHIFT);
                table = unsafe { &mut *(child_pa.as_mut_ptr()) };
            }
        }

        // At level 0, set the leaf PTE
        let idx = vpn[0];
        if table.is_valid(idx) {
            return Err(MmError::AlreadyMapped);
        }
        table.set_entry(idx, pte_bits | (((ppn as u64) & 0xFFF_FFFF_FFFF) << 10));
        Ok(())
    }

    /// Unmap a single 4KB page. Returns the physical address it was mapped to.
    pub fn unmap_page(&mut self, vaddr: VirtAddr) -> Result<PhysAddr, MmError> {
        if !vaddr.is_aligned(PAGE_SIZE) {
            return Err(MmError::InvalidAlignment);
        }

        let vpn = [
            (vaddr.raw() >> 12) & 0x1FF,
            (vaddr.raw() >> 21) & 0x1FF,
            (vaddr.raw() >> 30) & 0x1FF,
        ];

        let mut table = self.root_table();

        for level in (1..LEVELS).rev() {
            let idx = vpn[level];
            if !table.is_valid(idx) {
                return Err(MmError::NotMapped);
            }
            let child_ppn = table.ppn(idx);
            let child_pa = PhysAddr::new(child_ppn << PAGE_SHIFT);
            table = unsafe { &mut *(child_pa.as_mut_ptr()) };
        }

        let idx = vpn[0];
        if !table.is_valid(idx) {
            return Err(MmError::NotMapped);
        }
        let ppn = table.ppn(idx);
        let paddr = PhysAddr::new(ppn << PAGE_SHIFT);
        table.set_entry(idx, 0);
        Ok(paddr)
    }

    /// Translate a virtual address to a physical address by walking the page table.
    pub fn translate(&self, vaddr: VirtAddr) -> Option<PhysAddr> {
        let vpn = [
            (vaddr.raw() >> 12) & 0x1FF,
            (vaddr.raw() >> 21) & 0x1FF,
            (vaddr.raw() >> 30) & 0x1FF,
        ];

        let mut table = self.root_table();

        for level in (1..LEVELS).rev() {
            let idx = vpn[level];
            if !table.is_valid(idx) {
                return None;
            }
            if table.is_leaf(idx) {
                // Superpage — not supported yet
                return None;
            }
            let child_ppn = table.ppn(idx);
            let child_pa = PhysAddr::new(child_ppn << PAGE_SHIFT);
            table = unsafe { &mut *(child_pa.as_mut_ptr()) };
        }

        let idx = vpn[0];
        if !table.is_valid(idx) {
            return None;
        }
        let ppn = table.ppn(idx);
        let offset = vaddr.raw() & 0xFFF;
        Some(PhysAddr::new((ppn << PAGE_SHIFT) | offset))
    }

    /// Write the satp CSR to activate this page table.
    /// Only safe when running in S-mode with Sv39.
    pub unsafe fn activate_satp(&self) {
        let ppn = self.root.raw() >> PAGE_SHIFT;
        // satp mode = Sv39 (mode 8) | PPN
        let satp_val: usize = (8usize << 60) | (ppn & 0xFFF_FFFF_FFFF);
        unsafe {
            core::arch::asm!("csrw satp, {}", in(reg) satp_val);
            core::arch::asm!("sfence.vma");
        }
    }
}
