#[cfg(target_arch = "riscv32")]
use arch::riscv::sv32::Sv32PageTable;
#[cfg(target_arch = "riscv32")]
use kernel::mm::{MemPerms, MmError, PAGE_SIZE, PhysAddr, VirtAddr};

#[cfg(target_arch = "riscv32")]
const ESP32S31_SRAM_BASE: usize = 0x2f00_0000;
#[cfg(target_arch = "riscv32")]
const ESP32S31_SRAM_SIZE: usize = 0x0008_0000;
#[cfg(target_arch = "riscv32")]
const DTB_FALLBACK_MAP_SIZE: usize = 64 * 1024;

pub fn init_sv32_identity_map() {
    if !kernel::generated_config::OPENION_RISCV_SV32_MMU {
        return;
    }

    #[cfg(not(target_arch = "riscv32"))]
    {
        kernel::kwarn!("Sv32 MMU requested on a non-RV32 target; ignored");
    }

    #[cfg(target_arch = "riscv32")]
    init_sv32_identity_map_rv32();
}

#[cfg(target_arch = "riscv32")]
fn init_sv32_identity_map_rv32() {
    if !kernel::generated_config::OPENION_RISCV_S_MODE {
        kernel::kwarn!("Sv32 MMU requested outside S-mode; ignored");
        return;
    }

    let Some(root) = alloc_zeroed_frame() else {
        kernel::kerror!("Sv32 MMU: failed to allocate root page table");
        return;
    };

    let mut table = Sv32PageTable::new(root);
    let mut mapped_memory = false;
    let dtb = kernel::platform::dtb_addr();

    if dtb != 0 {
        unsafe {
            kernel::fdt::walk_nodes(dtb, |node| {
                if !node.is_available() {
                    return;
                }

                if node.device_type() == Some("memory") {
                    if let Some(reg) = node.first_reg() {
                        if map_identity_region(
                            &mut table,
                            reg.base,
                            reg.size,
                            MemPerms::READ | MemPerms::WRITE | MemPerms::EXECUTE,
                        ) {
                            mapped_memory = true;
                        }
                    }
                }
            });
        }
    }

    if !mapped_memory {
        let cfg = kernel::platform::get_config();
        map_identity_region(
            &mut table,
            cfg.memory_base,
            cfg.memory_size,
            MemPerms::READ | MemPerms::WRITE | MemPerms::EXECUTE,
        );
    }

    if dtb != 0 {
        map_identity_region(
            &mut table,
            align_down(dtb, PAGE_SIZE),
            DTB_FALLBACK_MAP_SIZE,
            MemPerms::READ | MemPerms::WRITE,
        );
    }

    map_identity_region(
        &mut table,
        ESP32S31_SRAM_BASE,
        ESP32S31_SRAM_SIZE,
        MemPerms::READ | MemPerms::WRITE | MemPerms::EXECUTE,
    );

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

    kernel::kinfo!("Sv32 MMU enabled: identity root={:#x}", root.raw());
}

#[cfg(target_arch = "riscv32")]
fn map_identity_region(
    table: &mut Sv32PageTable,
    base: usize,
    size: usize,
    perms: MemPerms,
) -> bool {
    if size == 0 {
        return false;
    }

    let start = align_down(base, PAGE_SIZE);
    let end = align_up(base.saturating_add(size), PAGE_SIZE);
    let mut mapped_any = false;
    let mut addr = start;

    while addr < end {
        let page = PhysAddr::new(addr);
        let result = table.map_page(VirtAddr::new(addr), page, perms, &mut alloc_zeroed_frame);
        match result {
            Ok(()) | Err(MmError::AlreadyMapped) => mapped_any = true,
            Err(err) => {
                kernel::kerror!("Sv32 MMU: map {:#x} failed: {:?}", addr, err);
                return mapped_any;
            }
        }
        addr = addr.saturating_add(PAGE_SIZE);
    }

    mapped_any
}

#[cfg(target_arch = "riscv32")]
fn alloc_zeroed_frame() -> Option<PhysAddr> {
    let frame = kernel::mm::alloc_frame()?;
    unsafe {
        core::ptr::write_bytes(frame.as_mut_ptr::<u8>(), 0, PAGE_SIZE);
    }
    Some(frame)
}

#[cfg(target_arch = "riscv32")]
const fn align_down(value: usize, align: usize) -> usize {
    value & !(align - 1)
}

#[cfg(target_arch = "riscv32")]
const fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}
