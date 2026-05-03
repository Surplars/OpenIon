//! Minimal ELF64 parser and loader.

/// ELF magic bytes
const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];

/// ELF64 header
#[repr(C, packed)]
struct Elf64Ehdr {
    e_ident: [u8; 16], // Magic + class + data + version + OS/ABI
    e_type: u16,       // ET_EXEC=2, ET_DYN=3
    e_machine: u16,    // EM_RISCV=243, EM_AARCH64=183
    e_version: u32,
    e_entry: u64, // Entry point address
    e_phoff: u64, // Program header offset
    e_shoff: u64, // Section header offset
    e_flags: u32,
    e_ehsize: u16,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
}

/// ELF64 program header
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct Elf64Phdr {
    p_type: u32,   // PT_LOAD=1
    p_flags: u32,  // PF_R=4, PF_W=2, PF_X=1
    p_offset: u64, // Offset in file
    p_vaddr: u64,  // Virtual address
    p_paddr: u64,  // Physical address
    p_filesz: u64, // Size in file
    p_memsz: u64,  // Size in memory
    p_align: u64,  // Alignment
}

const PT_LOAD: u32 = 1;
const ET_EXEC: u16 = 2;

/// Result of loading an ELF binary
pub struct ElfLoadResult {
    pub entry_point: usize,
    pub load_base: usize,
    pub load_size: usize,
}

/// Parse and validate an ELF header from a byte buffer.
pub fn validate_elf(data: &[u8]) -> Result<(), &'static str> {
    if data.len() < core::mem::size_of::<Elf64Ehdr>() {
        return Err("Too small for ELF header");
    }
    if data[..4] != ELF_MAGIC {
        return Err("Bad ELF magic");
    }
    // Check class: 2 = 64-bit
    if data[4] != 2 {
        return Err("Not ELF64");
    }
    // Check data encoding: 1 = little-endian
    if data[5] != 1 {
        return Err("Not little-endian");
    }
    Ok(())
}

/// Load ELF segments into memory at their specified physical addresses.
///
/// # Safety
/// This writes directly to the addresses specified in the ELF program headers.
/// The caller must ensure the memory regions are available and properly mapped.
pub unsafe fn load_elf(data: &[u8]) -> Result<ElfLoadResult, &'static str> {
    validate_elf(data)?;

    let ehdr = unsafe { &*(data.as_ptr() as *const Elf64Ehdr) };

    if ehdr.e_type != ET_EXEC {
        return Err("Not an executable ELF");
    }

    let entry = ehdr.e_entry as usize;
    let phoff = ehdr.e_phoff as usize;
    let phnum = ehdr.e_phnum as usize;
    let phentsize = ehdr.e_phentsize as usize;

    let mut load_base = usize::MAX;
    let mut load_end = 0usize;

    for i in 0..phnum {
        let offset = phoff + i * phentsize;
        if offset + core::mem::size_of::<Elf64Phdr>() > data.len() {
            return Err("Program header out of bounds");
        }
        let phdr = unsafe { &*(data.as_ptr().add(offset) as *const Elf64Phdr) };

        if phdr.p_type != PT_LOAD {
            continue;
        }

        let file_offset = phdr.p_offset as usize;
        let mem_addr = phdr.p_paddr as usize;
        let file_size = phdr.p_filesz as usize;
        let mem_size = phdr.p_memsz as usize;

        if file_offset + file_size > data.len() {
            return Err("Segment data out of bounds");
        }

        // Copy segment data to target address
        unsafe {
            let dst = mem_addr as *mut u8;
            core::ptr::copy_nonoverlapping(data.as_ptr().add(file_offset), dst, file_size);
            // Zero BSS (mem_size > file_size)
            if mem_size > file_size {
                core::ptr::write_bytes(dst.add(file_size), 0, mem_size - file_size);
            }
        }

        if mem_addr < load_base {
            load_base = mem_addr;
        }
        if mem_addr + mem_size > load_end {
            load_end = mem_addr + mem_size;
        }
    }

    if load_base >= load_end {
        return Err("No LOAD segments found");
    }

    Ok(ElfLoadResult {
        entry_point: entry,
        load_base,
        load_size: load_end - load_base,
    })
}
