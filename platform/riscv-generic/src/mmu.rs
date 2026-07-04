/// Platform MMU initialization for RISC-V.
///
/// This module provides identity-mapping initialization for both Sv32 (RV32)
/// and Sv39 (RV64) page tables using the common RiscvPageTable trait.

#[cfg(target_arch = "riscv32")]
use arch::riscv::sv32::Sv32PageTable;
#[cfg(target_arch = "riscv64")]
use arch::riscv::sv39::Sv39PageTable;

use arch::riscv::mmu::RiscvPageTable;
use kernel::mm::{MemPerms, MmError, PAGE_SIZE, PhysAddr, VirtAddr};

const DTB_FALLBACK_MAP_SIZE: usize = 64 * 1024;

#[derive(Clone, Copy)]
struct KernelSections {
    text_start: usize,
    text_end: usize,
    rodata_start: usize,
    rodata_end: usize,
}

/// Initialize identity-mapped page table based on target architecture.
pub fn init_identity_map() {
    #[cfg(target_arch = "riscv32")]
    {
        if kernel::generated_config::OPENION_RISCV_SV32_MMU {
            init_identity_map_impl::<Sv32PageTable>("Sv32");
        }
    }

    #[cfg(target_arch = "riscv64")]
    {
        if kernel::generated_config::OPENION_RISCV_SV39_MMU {
            init_identity_map_impl::<Sv39PageTable>("Sv39");
        }
    }
}

/// Generic identity map initialization using RiscvPageTable trait.
fn init_identity_map_impl<T: RiscvPageTable>(mode_name: &str) {
    if !kernel::generated_config::OPENION_RISCV_S_MODE {
        kernel::kwarn!("{} MMU requested outside S-mode; ignored", mode_name);
        return;
    }

    let Some(root) = alloc_zeroed_frame() else {
        kernel::kerror!("{} MMU: failed to allocate root page table", mode_name);
        return;
    };

    let mut table = T::new(root);
    let mut mapped_memory = false;
    let dtb = kernel::platform::dtb_addr();
    let sections = kernel_sections();

    // Map memory regions from DTB
    if dtb != 0 {
        unsafe {
            kernel::fdt::walk_nodes(dtb, |node| {
                if !node.is_available() {
                    return;
                }

                if node.device_type() == Some("memory") {
                    if let Some(reg) = node.first_reg() {
                        if map_memory_region(&mut table, reg.base, reg.size, sections) {
                            mapped_memory = true;
                        }
                    }
                }
            });
        }
    }

    // Fallback to platform config if no DTB memory found
    if !mapped_memory {
        let cfg = kernel::platform::get_config();
        map_memory_region(&mut table, cfg.memory_base, cfg.memory_size, sections);
    }

    // Map DTB region
    if dtb != 0 {
        map_identity_region(
            &mut table,
            align_down(dtb, PAGE_SIZE),
            DTB_FALLBACK_MAP_SIZE,
            MemPerms::READ | MemPerms::WRITE,
        );
    }

    // Map device regions from DTB
    if dtb != 0 {
        unsafe {
            kernel::fdt::walk_nodes(dtb, |node| {
                if !node.is_available() || node.device_type() == Some("memory") {
                    return;
                }

                if let Some(reg) = node.first_reg() {
                    map_identity_region(
                        &mut table,
                        reg.base,
                        reg.size,
                        MemPerms::READ | MemPerms::WRITE,
                    );
                }
            });
        }
    }

    unsafe {
        table.activate_satp();
    }

    kernel::mm::set_translation_enabled();

    kernel::kinfo!(
        "{} MMU enabled: identity root={:#x}, va_bits={}, levels={}",
        table.mode_name(),
        root.raw(),
        table.va_bits(),
        table.levels()
    );
}

fn kernel_sections() -> KernelSections {
    unsafe extern "C" {
        fn stext();
        fn etext();
        fn srodata();
        fn erodata();
    }

    KernelSections {
        text_start: stext as *const () as usize,
        text_end: etext as *const () as usize,
        rodata_start: srodata as *const () as usize,
        rodata_end: erodata as *const () as usize,
    }
}

fn kernel_memory_perms(addr: usize, sections: KernelSections) -> MemPerms {
    if addr >= align_down(sections.text_start, PAGE_SIZE)
        && addr < align_up(sections.text_end, PAGE_SIZE)
    {
        MemPerms::READ | MemPerms::EXECUTE
    } else if addr >= align_down(sections.rodata_start, PAGE_SIZE)
        && addr < align_up(sections.rodata_end, PAGE_SIZE)
    {
        MemPerms::READ
    } else {
        MemPerms::READ | MemPerms::WRITE
    }
}

fn map_memory_region<T: RiscvPageTable>(
    table: &mut T,
    base: usize,
    size: usize,
    sections: KernelSections,
) -> bool {
    map_identity_region_with(table, base, size, true, |addr| {
        if is_stack_guard_page(addr) {
            None
        } else {
            Some(kernel_memory_perms(addr, sections))
        }
    })
}

/// Map a region of physical memory as identity-mapped.
fn map_identity_region<T: RiscvPageTable>(
    table: &mut T,
    base: usize,
    size: usize,
    perms: MemPerms,
) -> bool {
    map_identity_region_with(table, base, size, false, |_| Some(perms))
}

fn map_identity_region_with<T, F>(
    table: &mut T,
    base: usize,
    size: usize,
    use_superpages: bool,
    mut perms_for: F,
) -> bool
where
    T: RiscvPageTable,
    F: FnMut(usize) -> Option<MemPerms>,
{
    if size == 0 {
        return false;
    }

    let start = align_down(base, PAGE_SIZE);
    let end = align_up(base.saturating_add(size), PAGE_SIZE);
    let superpage_size = if use_superpages {
        table.max_superpage_size()
    } else {
        0
    };
    let mut mapped_any = false;
    let mut addr = start;

    while addr < end {
        if superpage_size != 0
            && is_aligned(addr, superpage_size)
            && addr.saturating_add(superpage_size) <= end
        {
            if let Some(perms) = uniform_region_perms(addr, superpage_size, &mut perms_for) {
                match table.map_superpage(
                    VirtAddr::new(addr),
                    PhysAddr::new(addr),
                    superpage_size,
                    perms,
                    &mut alloc_zeroed_frame,
                ) {
                    Ok(()) => {
                        mapped_any = true;
                        addr = addr.saturating_add(superpage_size);
                        continue;
                    }
                    Err(_) => {}
                }
            }
        }

        let Some(perms) = perms_for(addr) else {
            addr = addr.saturating_add(PAGE_SIZE);
            continue;
        };
        let page = PhysAddr::new(addr);
        let result = table.map_page(VirtAddr::new(addr), page, perms, &mut alloc_zeroed_frame);
        match result {
            Ok(()) | Err(MmError::AlreadyMapped) => mapped_any = true,
            Err(err) => {
                kernel::kerror!("MMU: map {:#x} failed: {:?}", addr, err);
                return mapped_any;
            }
        }
        addr = addr.saturating_add(PAGE_SIZE);
    }

    mapped_any
}

fn uniform_region_perms(
    start: usize,
    size: usize,
    perms_for: &mut impl FnMut(usize) -> Option<MemPerms>,
) -> Option<MemPerms> {
    let first = perms_for(start)?;
    let mut addr = start.saturating_add(PAGE_SIZE);
    let end = start.saturating_add(size);

    while addr < end {
        if perms_for(addr)? != first {
            return None;
        }
        addr = addr.saturating_add(PAGE_SIZE);
    }

    Some(first)
}

fn is_stack_guard_page(addr: usize) -> bool {
    let mut guarded = false;
    kernel::sched::for_each_stack_guard(|guard| {
        guarded |= addr == align_down(guard, PAGE_SIZE);
    });
    guarded
}

fn alloc_zeroed_frame() -> Option<PhysAddr> {
    let frame = kernel::mm::alloc_frame()?;
    unsafe {
        core::ptr::write_bytes(frame.as_mut_ptr::<u8>(), 0, PAGE_SIZE);
    }
    Some(frame)
}

const fn align_down(value: usize, align: usize) -> usize {
    value & !(align - 1)
}

const fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

const fn is_aligned(value: usize, align: usize) -> bool {
    value & (align - 1) == 0
}
