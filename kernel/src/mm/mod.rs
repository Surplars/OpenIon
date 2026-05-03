pub mod addr;
pub mod frame;
pub mod slab;
pub mod tlsf;

pub use addr::{PAGE_SHIFT, PAGE_SIZE, PhysAddr, VirtAddr};
pub use frame::FrameStats;
pub use tlsf::{GlobalTlsfAlloc, HeapStats};

use crate::sync::Mutex;
use core::sync::atomic::{AtomicBool, Ordering};

const KERNEL_HEAP_SIZE: usize = 64 * 1024;

#[repr(align(16))]
struct HeapStorage([u8; KERNEL_HEAP_SIZE]);

#[global_allocator]
static GLOBAL_ALLOCATOR: GlobalTlsfAlloc = GlobalTlsfAlloc::new();

static FRAME_ALLOCATOR: Mutex<frame::FrameAllocator> = Mutex::new(frame::FrameAllocator::new());
static INITIALIZED: AtomicBool = AtomicBool::new(false);
static mut KERNEL_HEAP: HeapStorage = HeapStorage([0; KERNEL_HEAP_SIZE]);

#[derive(Clone, Copy, Debug)]
pub struct MmStats {
    pub initialized: bool,
    pub heap: HeapStats,
    pub frames: FrameStats,
    pub heap_algorithm: &'static str,
    pub object_pool_algorithm: &'static str,
    pub frame_algorithm: &'static str,
}

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

pub fn init(config: &crate::platform::PlatformConfig) {
    if INITIALIZED.swap(true, Ordering::AcqRel) {
        return;
    }

    unsafe {
        let heap = &mut (*core::ptr::addr_of_mut!(KERNEL_HEAP)).0;
        GLOBAL_ALLOCATOR.init(heap);
    }

    let frame_start = align_up(config.kernel_end, PAGE_SIZE);
    let mem_end = config.memory_base.saturating_add(config.memory_size);
    if frame_start < mem_end {
        let frame_size = mem_end - frame_start;
        let mut frames = FRAME_ALLOCATOR.lock();
        unsafe {
            frames.init(PhysAddr::new(frame_start), frame_size);
        }
    }

    crate::kinfo!(
        "MM initialized: heap={} bytes, frame_base={:#x}",
        KERNEL_HEAP_SIZE,
        frame_start
    );
}

pub fn stats() -> MmStats {
    let frames = FRAME_ALLOCATOR.lock().stats();
    MmStats {
        initialized: INITIALIZED.load(Ordering::Acquire),
        heap: GLOBAL_ALLOCATOR.stats(),
        frames,
        heap_algorithm: "TLSF",
        object_pool_algorithm: "Slab",
        frame_algorithm: "Bitmap",
    }
}

pub fn alloc_frame() -> Option<PhysAddr> {
    FRAME_ALLOCATOR.lock().alloc()
}

pub fn free_frame(addr: PhysAddr) {
    FRAME_ALLOCATOR.lock().free(addr);
}

const fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}
