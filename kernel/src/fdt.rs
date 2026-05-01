//! Minimal FDT (Flattened Device Tree) parser.
//!
//! Parses the DTB passed by QEMU/OpenSBI to discover devices.

const FDT_MAGIC: u32 = 0xD00DFEED;
const FDT_BEGIN_NODE: u32 = 0x00000001;
const FDT_END_NODE: u32 = 0x00000002;
const FDT_PROP: u32 = 0x00000003;
const FDT_NOP: u32 = 0x00000004;
const FDT_END: u32 = 0x00000009;

/// A parsed FDT header.
struct FdtHeader {
    totalsize: u32,
    off_dt_struct: u32,
    off_dt_strings: u32,
}

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
    do_parse(dtb_addr, callback)
}

/// Generic version of parse that accepts closures capturing state.
///
/// # Safety
/// `dtb_addr` must point to a valid DTB in memory.
pub unsafe fn parse_with<F: FnMut(&str, &str, &[u8], u32)>(dtb_addr: usize, mut callback: F) {
    do_parse(dtb_addr, |node_name, compatible, reg, interrupt| callback(node_name, compatible, reg, interrupt))
}

unsafe fn do_parse(dtb_addr: usize, mut callback: impl FnMut(&str, &str, &[u8], u32)) {
    let base = dtb_addr as *const u8;

    // Read header
    let magic = read_be32(base.add(0));
    if magic != FDT_MAGIC {
        crate::kdebug!("FDT: bad magic {:#x}", magic);
        return;
    }
    let totalsize = read_be32(base.add(4));
    let off_struct = read_be32(base.add(8)) as usize;
    let off_strings = read_be32(base.add(12)) as usize;

    let struct_base = base.add(off_struct);
    let strings_base = base.add(off_strings);

    crate::kdebug!("FDT: struct={:#x} strings={:#x} size={}", off_struct, off_strings, totalsize);

    // Walk the structure block
    let mut pos = struct_base;
    let end = base.add(totalsize as usize);

    let mut depth: usize = 0;
    let mut node_name = "";
    let mut prop_name = "";
    let mut prop_data: (usize, usize) = (0, 0); // (offset, len)
    let mut compatible = "";
    let mut reg_data: [u8; 32] = [0; 32];
    let mut reg_len: usize = 0;
    let mut interrupt: u32 = 0;

    while (pos as usize) < (end as usize) {
        let token = read_be32(pos);
        pos = pos.add(4);

        match token {
            FDT_BEGIN_NODE => {
                depth += 1;
                // Read node name (null-terminated, 4-byte aligned)
                let name_start = pos;
                let mut len = 0usize;
                while *pos.add(len) != 0 {
                    len += 1;
                }
                node_name = if len > 0 {
                    core::str::from_utf8_unchecked(core::slice::from_raw_parts(name_start, len))
                } else {
                    ""
                };
                // Skip name + null + alignment
                pos = pos.add((len + 4) & !3);

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
                let nameoff = read_be32(pos.add(4)) as usize;
                pos = pos.add(8);

                prop_name = core::str::from_utf8_unchecked(core::slice::from_raw_parts(
                    strings_base.add(nameoff),
                    // Find length of string
                    {
                        let mut l = 0;
                        while *strings_base.add(nameoff + l) != 0 {
                            l += 1;
                        }
                        l
                    },
                ));

                let data_ptr = pos;

                // Handle known properties
                match prop_name {
                    "compatible" => {
                        // Compatible is one or more null-terminated strings
                        let slice = core::slice::from_raw_parts(data_ptr, len);
                        // Use the first string (before the first null)
                        let end = slice.iter().position(|&b| b == 0).unwrap_or(len);
                        compatible = core::str::from_utf8_unchecked(&slice[..end]);
                    }
                    "reg" => {
                        let copy_len = len.min(32);
                        core::ptr::copy_nonoverlapping(data_ptr, reg_data.as_mut_ptr(), copy_len);
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
                pos = pos.add((len + 3) & !3);
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

/// Scan FDT for VirtIO MMIO devices and return their base addresses.
///
/// # Safety
/// `dtb_addr` must point to a valid DTB in memory.
pub unsafe fn find_virtio_mmio(dtb_addr: usize) -> VirtioDevices {
    let mut devs = VirtioDevices::new();

    let base = dtb_addr as *const u8;
    let magic = read_be32(base);
    if magic != FDT_MAGIC {
        return devs;
    }

    // Walk FDT manually to find virtio,mmio nodes
    let totalsize = read_be32(base.add(4)) as usize;
    let off_struct = read_be32(base.add(8)) as usize;
    let off_strings = read_be32(base.add(12)) as usize;
    let struct_base = base.add(off_struct);
    let strings_base = base.add(off_strings);
    let end = base.add(totalsize);

    crate::kdebug!("FDT: struct={:#x} strings={:#x} totalsize={}", off_struct, off_strings, totalsize);

    let mut pos = struct_base;
    let mut node_name_bytes: &[u8] = &[];
    let mut depth: usize = 0;
    // Per-node state
    let mut compat_str: &[u8] = &[];
    let mut reg_buf = [0u8; 32];
    let mut reg_len: usize = 0;

    while (pos as usize) < (end as usize) {
        let token = read_be32(pos);
        pos = pos.add(4);

        if depth < 3 {
            crate::kdebug!("FDT: tok={:#x} depth={} pos={:#x}", token, depth, pos as usize - 4);
        }

        match token {
            FDT_BEGIN_NODE => {
                depth += 1;
                let name_start = pos;
                let mut len = 0usize;
                while *pos.add(len) != 0 { len += 1; }

                if depth <= 3 {
                    let name = core::str::from_utf8_unchecked(core::slice::from_raw_parts(name_start, len));
                    crate::kdebug!("FDT: node '{}' depth={}", name, depth);
                }
                node_name_bytes = core::slice::from_raw_parts(name_start, len);
                pos = pos.add((len + 4) & !3);
                compat_str = &[];
                reg_len = 0;
            }
            FDT_END_NODE => {
                if !compat_str.is_empty() && reg_len >= 16 {
                    let compat = core::str::from_utf8_unchecked(compat_str);
                    if compat.contains("virtio,mmio") {
                        let addr = u64::from_be_bytes([
                            reg_buf[0], reg_buf[1], reg_buf[2], reg_buf[3],
                            reg_buf[4], reg_buf[5], reg_buf[6], reg_buf[7],
                        ]) as usize;
                        if devs.count < MAX_VIRTIO_DEVICES {
                            devs.addrs[devs.count] = addr;
                            devs.count += 1;
                        }
                    }
                }
                if depth > 0 { depth -= 1; }
            }
            FDT_PROP => {
                let len = read_be32(pos) as usize;
                let nameoff = read_be32(pos.add(4)) as usize;
                pos = pos.add(8);

                if depth <= 3 {
                    crate::kdebug!("FDT: PROP len={} nameoff={} data_start={:#x}", len, nameoff, pos as usize);
                }

                let pname = {
                    let start = strings_base.add(nameoff);
                    let mut l = 0;
                    while *start.add(l) != 0 { l += 1; }
                    core::slice::from_raw_parts(start, l)
                };

                let data = core::slice::from_raw_parts(pos, len);

                if pname == b"compatible" {
                    let end = data.iter().position(|&b| b == 0).unwrap_or(len);
                    compat_str = &data[..end];
                } else if pname == b"reg" {
                    let copy = len.min(32);
                    reg_buf[..copy].copy_from_slice(&data[..copy]);
                    reg_len = copy;
                }

                pos = pos.add((len + 3) & !3);
            }
            FDT_END => break,
            FDT_NOP => { /* skip */ }
            _ => break,
        }
    }

    devs
}

const MAX_VIRTIO_DEVICES: usize = 8;

pub struct VirtioDevices {
    pub count: usize,
    pub addrs: [usize; MAX_VIRTIO_DEVICES],
}

impl VirtioDevices {
    pub const fn new() -> Self {
        Self {
            count: 0,
            addrs: [0; MAX_VIRTIO_DEVICES],
        }
    }
}

fn read_be32(ptr: *const u8) -> u32 {
    unsafe {
        u32::from_be_bytes([
            *ptr,
            *ptr.add(1),
            *ptr.add(2),
            *ptr.add(3),
        ])
    }
}

fn read_be64_from_bytes(data: &[u8], offset: usize) -> u64 {
    if offset + 8 > data.len() {
        return 0;
    }
    u64::from_be_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
        data[offset + 4],
        data[offset + 5],
        data[offset + 6],
        data[offset + 7],
    ])
}
