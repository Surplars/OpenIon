/// Common MMU abstraction for RISC-V page table implementations.
///
/// This module provides a unified interface for Sv32 and Sv39 page tables,
/// allowing platform code to use either implementation through a single API.
use kernel::mm::addr::{PhysAddr, VirtAddr};
use kernel::mm::{MemPerms, MmError};

/// Result type for MMU operations.
pub type MmuResult<T> = Result<T, MmError>;

/// Common trait for RISC-V page table implementations.
///
/// Both Sv32 (RV32) and Sv39 (RV64) implement this trait, providing
/// a unified API for page table management.
pub trait RiscvPageTable: Send + Sync {
    /// Create a new page table from a zeroed, page-aligned root page.
    fn new(root: PhysAddr) -> Self
    where
        Self: Sized;

    /// Get the root page table physical address.
    fn root_addr(&self) -> PhysAddr;

    /// Get the satp value for this page table (mode + ppn).
    fn satp_value(&self) -> usize;

    /// Map a single 4KB page: vaddr → paddr with given permissions.
    ///
    /// `alloc` is called to allocate new page table pages when needed.
    fn map_page(
        &mut self,
        vaddr: VirtAddr,
        paddr: PhysAddr,
        perms: MemPerms,
        alloc: &mut dyn FnMut() -> Option<PhysAddr>,
    ) -> MmuResult<()>;

    /// Largest superpage size supported for identity mapping, in bytes.
    fn max_superpage_size(&self) -> usize {
        0
    }

    /// Map one superpage. Implementations may reject unsupported sizes.
    fn map_superpage(
        &mut self,
        _vaddr: VirtAddr,
        _paddr: PhysAddr,
        _size: usize,
        _perms: MemPerms,
        _alloc: &mut dyn FnMut() -> Option<PhysAddr>,
    ) -> MmuResult<()> {
        Err(MmError::InvalidRegion)
    }

    /// Unmap a single 4KB page. Returns the physical address it was mapped to.
    fn unmap_page(&mut self, vaddr: VirtAddr) -> MmuResult<PhysAddr>;

    /// Translate a virtual address to a physical address by walking the page table.
    fn translate(&self, vaddr: VirtAddr) -> Option<PhysAddr>;

    /// Write the satp CSR to activate this page table.
    ///
    /// # Safety
    /// Changes the CPU's address translation. Must be called with
    /// appropriate privilege and after setting up identity mappings.
    unsafe fn activate_satp(&self);

    /// Get the page table mode name (e.g., "Sv32", "Sv39").
    fn mode_name(&self) -> &'static str;

    /// Get the number of page table levels.
    fn levels(&self) -> usize;

    /// Get the virtual address space size in bits.
    fn va_bits(&self) -> usize;
}

/// Platform MMU configuration.
pub struct MmuConfig {
    /// Whether MMU is enabled for this platform.
    pub enabled: bool,
    /// Page table mode ("sv32", "sv39", or "none").
    pub mode: &'static str,
    /// Whether to create identity mappings.
    pub identity_map: bool,
}

impl MmuConfig {
    pub const fn none() -> Self {
        Self {
            enabled: false,
            mode: "none",
            identity_map: false,
        }
    }

    pub const fn sv32() -> Self {
        Self {
            enabled: true,
            mode: "sv32",
            identity_map: true,
        }
    }

    pub const fn sv39() -> Self {
        Self {
            enabled: true,
            mode: "sv39",
            identity_map: true,
        }
    }
}
