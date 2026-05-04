//! Minimal FDT (Flattened Device Tree) parser.
//!
//! Parses the DTB passed by QEMU/OpenSBI to discover devices.

const FDT_MAGIC: u32 = 0xD00DFEED;
const FDT_BEGIN_NODE: u32 = 0x00000001;
const FDT_END_NODE: u32 = 0x00000002;
const FDT_PROP: u32 = 0x00000003;
const FDT_NOP: u32 = 0x00000004;
const FDT_END: u32 = 0x00000009;

/// Callback for each device node found in the FDT.
/// Args: node_name, compatible, reg (base_addr, size), interrupt (first interrupt cell, 0 if none)
pub type DeviceCallback = fn(node_name: &str, compatible: &str, reg: &[u8], interrupt: u32);

/// Parse the FDT at the given address and call `callback` for each device node
/// that has a `compatible` property.
///
/// `callback` receives (node_name, compatible, reg_data, interrupt).
///
/// # Safety
/// `dtb_addr` must point to a valid DTB in memory.
pub unsafe fn parse(dtb_addr: usize, callback: DeviceCallback) {
    unsafe { do_parse(dtb_addr, callback) }
}

/// Generic version of parse that accepts closures capturing state.
///
/// # Safety
/// `dtb_addr` must point to a valid DTB in memory.
pub unsafe fn parse_with<F: FnMut(&str, &str, &[u8], u32)>(dtb_addr: usize, mut callback: F) {
    unsafe {
        do_parse(dtb_addr, |node_name, compatible, reg, interrupt| {
            callback(node_name, compatible, reg, interrupt)
        })
    }
}

unsafe fn do_parse(dtb_addr: usize, mut callback: impl FnMut(&str, &str, &[u8], u32)) {
    let base = dtb_addr as *const u8;

    // Read header
    let magic = read_be32(unsafe { base.add(0) });
    if magic != FDT_MAGIC {
        crate::kdebug!("FDT: bad magic {:#x}", magic);
        return;
    }
    let totalsize = read_be32(unsafe { base.add(4) });
    let off_struct = read_be32(unsafe { base.add(8) }) as usize;
    let off_strings = read_be32(unsafe { base.add(12) }) as usize;

    let struct_base = unsafe { base.add(off_struct) };
    let strings_base = unsafe { base.add(off_strings) };

    // Walk the structure block
    let mut pos = struct_base;
    let end = unsafe { base.add(totalsize as usize) };

    let mut depth: usize = 0;
    let mut node_name = "";
    let mut compatible = "";
    let mut reg_data: [u8; 32] = [0; 32];
    let mut reg_len: usize = 0;
    let mut interrupt: u32 = 0;

    while (pos as usize) < (end as usize) {
        let token = read_be32(pos);
        pos = unsafe { pos.add(4) };

        match token {
            FDT_BEGIN_NODE => {
                depth += 1;
                // Read node name (null-terminated, 4-byte aligned)
                let name_start = pos;
                let mut len = 0usize;
                while unsafe { *pos.add(len) } != 0 {
                    len += 1;
                }
                node_name = if len > 0 {
                    unsafe {
                        core::str::from_utf8_unchecked(core::slice::from_raw_parts(name_start, len))
                    }
                } else {
                    ""
                };
                // Skip name + null + alignment
                pos = unsafe { pos.add((len + 4) & !3) };

                // Reset per-node state
                compatible = "";
                reg_len = 0;
                interrupt = 0;
            }
            FDT_END_NODE => {
                // If this node has compatible and reg, call the callback
                if !compatible.is_empty() && reg_len > 0 {
                    callback(node_name, compatible, &reg_data[..reg_len], interrupt);
                }
                if depth > 0 {
                    depth -= 1;
                }
            }
            FDT_PROP => {
                let len = read_be32(pos) as usize;
                let nameoff = read_be32(unsafe { pos.add(4) }) as usize;
                pos = unsafe { pos.add(8) };

                let prop_name = unsafe {
                    core::str::from_utf8_unchecked(core::slice::from_raw_parts(
                        strings_base.add(nameoff),
                        // Find length of string
                        {
                            let mut l = 0;
                            while *strings_base.add(nameoff + l) != 0 {
                                l += 1;
                            }
                            l
                        },
                    ))
                };

                let data_ptr = pos;

                // Handle known properties
                match prop_name {
                    "compatible" => {
                        // Compatible is one or more null-terminated strings
                        let slice = unsafe { core::slice::from_raw_parts(data_ptr, len) };
                        // Use the first string (before the first null)
                        let end = slice.iter().position(|&b| b == 0).unwrap_or(len);
                        compatible = unsafe { core::str::from_utf8_unchecked(&slice[..end]) };
                    }
                    "reg" => {
                        let copy_len = len.min(32);
                        unsafe {
                            core::ptr::copy_nonoverlapping(
                                data_ptr,
                                reg_data.as_mut_ptr(),
                                copy_len,
                            );
                        }
                        reg_len = copy_len;
                    }
                    "interrupts" => {
                        // Read first interrupt cell as be32
                        if len >= 4 {
                            interrupt = read_be32(data_ptr);
                        }
                    }
                    _ => {}
                }

                // Skip data + alignment
                pos = unsafe { pos.add((len + 3) & !3) };
            }
            FDT_END => {
                break;
            }
            FDT_NOP => {
                // Skip NOP padding
            }
            _ => {
                break;
            }
        }
    }
}

fn read_be32(ptr: *const u8) -> u32 {
    unsafe { u32::from_be_bytes([*ptr, *ptr.add(1), *ptr.add(2), *ptr.add(3)]) }
}
