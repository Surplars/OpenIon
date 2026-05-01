pub mod addr;
pub mod frame;
pub mod slab;
pub mod tlsf;

pub use addr::{PhysAddr, VirtAddr, PAGE_SIZE, PAGE_SHIFT};

bitflags::bitflags! {
    /// Memory access permissions, architecture-agnostic.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct MemPerms: u32 {
        const READ    = 1 << 0;
        const WRITE   = 1 << 1;
        const EXECUTE = 1 << 2;
    }
}

/// Describes a contiguous region of physical memory (e.g. from device tree).
#[derive(Clone, Copy, Debug)]
pub struct MemRegion {
    pub base: PhysAddr,
    pub size: usize,
}

/// Abstraction over memory protection mechanisms.
///
/// Implementations:
/// - **Sv39/Sv48 page table** (RISC-V with MMU): full virtual memory
/// - **PMP** (RISC-V without MMU): physical address protection
/// - **MPU** (ARM Cortex-M): fixed memory regions
///
/// The kernel only deals in `PhysAddr` here. Virtual address translation
/// is an arch-specific detail behind this trait.
pub trait MemoryManager: Send + Sync {
    /// Map a physical page at `phys` with the given permissions.
    /// For MMU systems this populates page table entries;
    /// for MPU/PMP this configures protection regions.
    fn map(&mut self, phys: PhysAddr, perms: MemPerms) -> Result<(), MmError>;

    /// Unmap a previously mapped page.
    fn unmap(&mut self, phys: PhysAddr) -> Result<(), MmError>;

    /// Translate a virtual address to a physical address.
    /// Returns None for pure-physical (MPU/PMP) systems.
    fn translate(&self, vaddr: VirtAddr) -> Option<PhysAddr>;

    /// Activate this memory manager (e.g. write `satp`, PMP CSRs).
    /// Safety: changes the CPU's address translation / protection.
    unsafe fn activate(&self);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MmError {
    OutOfMemory,
    AlreadyMapped,
    NotMapped,
    InvalidAlignment,
    InvalidRegion,
}
