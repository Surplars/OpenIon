//! Minimal Flattened Device Tree parser.
//!
//! The parser deliberately stays small: it walks the DTB structure block and
//! exposes raw node properties plus a few convenience helpers used during boot.

const FDT_MAGIC: u32 = 0xd00d_feed;
const FDT_BEGIN_NODE: u32 = 0x0000_0001;
const FDT_END_NODE: u32 = 0x0000_0002;
const FDT_PROP: u32 = 0x0000_0003;
const FDT_NOP: u32 = 0x0000_0004;
const FDT_END: u32 = 0x0000_0009;
const MAX_DEPTH: usize = 16;
const DEFAULT_ADDRESS_CELLS: u8 = 2;
const DEFAULT_SIZE_CELLS: u8 = 1;

/// Callback for each device node found in the FDT.
/// Args: node_name, compatible, reg (base_addr, size), interrupt (first interrupt cell, 0 if none)
pub type DeviceCallback = fn(node_name: &str, compatible: &str, reg: &[u8], interrupt: u32);

#[derive(Clone, Copy)]
pub struct FdtReg {
    pub base: usize,
    pub size: usize,
}

#[derive(Clone, Copy)]
pub struct FdtNode<'a> {
    pub name: &'a str,
    compatible: Option<&'a [u8]>,
    reg: Option<&'a [u8]>,
    interrupt: Option<u32>,
    timebase_frequency: Option<u32>,
    address_cells: u8,
    size_cells: u8,
    child_address_cells: u8,
    child_size_cells: u8,
}

impl<'a> FdtNode<'a> {
    pub const fn name(&self) -> &'a str {
        self.name
    }

    pub fn first_compatible(&self) -> Option<&'a str> {
        self.compatible
            .and_then(|data| first_nul_string(data))
            .and_then(|s| core::str::from_utf8(s).ok())
    }

    pub fn compatible_matches(&self, expected: &str) -> bool {
        let Some(data) = self.compatible else {
            return false;
        };

        for item in NulStringIter::new(data) {
            if item == expected.as_bytes() {
                return true;
            }
        }
        false
    }

    pub const fn reg_raw(&self) -> Option<&'a [u8]> {
        self.reg
    }

    pub fn first_reg(&self) -> Option<FdtReg> {
        let reg = self.reg?;
        let address_cells = self.address_cells as usize;
        let size_cells = self.size_cells as usize;
        let entry_cells = address_cells.checked_add(size_cells)?;
        let entry_len = entry_cells.checked_mul(core::mem::size_of::<u32>())?;
        if entry_len == 0 || reg.len() < entry_len {
            return None;
        }

        let base = read_cells(&reg[..address_cells * 4], address_cells)? as usize;
        let size_start = address_cells * 4;
        let size = if size_cells == 0 {
            0
        } else {
            read_cells(&reg[size_start..size_start + size_cells * 4], size_cells)? as usize
        };

        Some(FdtReg { base, size })
    }

    pub const fn interrupt(&self) -> Option<u32> {
        self.interrupt
    }

    pub const fn interrupt_or_zero(&self) -> u32 {
        match self.interrupt {
            Some(irq) => irq,
            None => 0,
        }
    }

    pub const fn timebase_frequency(&self) -> Option<u32> {
        self.timebase_frequency
    }

    pub const fn address_cells(&self) -> u8 {
        self.address_cells
    }

    pub const fn size_cells(&self) -> u8 {
        self.size_cells
    }
}

/// Legacy callback wrapper. New code should use [`walk_nodes`] and [`FdtNode`]
/// so it can handle multiple compatible strings and typed resources.
///
/// Parse the FDT at the given address and call `callback` for each device node
/// that has a `compatible` property.
///
/// `callback` receives (node_name, first_compatible, reg_data, interrupt).
///
/// # Safety
/// `dtb_addr` must point to a valid DTB in memory.
pub unsafe fn parse(dtb_addr: usize, callback: DeviceCallback) {
    unsafe {
        walk_nodes(dtb_addr, |node| {
            if let (Some(compatible), Some(reg)) = (node.first_compatible(), node.reg_raw()) {
                callback(node.name(), compatible, reg, node.interrupt_or_zero());
            }
        })
    }
}

/// Legacy compatibility wrapper for older FDT callers. New code should use
/// [`walk_nodes`].
///
/// # Safety
/// `dtb_addr` must point to a valid DTB in memory.
pub unsafe fn parse_with<F: FnMut(&str, &str, &[u8], u32)>(dtb_addr: usize, mut callback: F) {
    unsafe {
        walk_nodes(dtb_addr, |node| {
            if let (Some(compatible), Some(reg)) = (node.first_compatible(), node.reg_raw()) {
                callback(node.name(), compatible, reg, node.interrupt_or_zero());
            }
        })
    }
}

/// Walk all nodes and expose the node properties used by early platform code
/// and FDT driver probing.
///
/// # Safety
/// `dtb_addr` must point to a valid DTB in memory that remains valid while the
/// callback is executing.
pub unsafe fn walk_nodes<F: FnMut(FdtNode<'static>)>(dtb_addr: usize, mut callback: F) {
    let base = dtb_addr as *const u8;

    let magic = unsafe { read_be32_ptr(base.add(0)) };
    if magic != FDT_MAGIC {
        crate::kdebug!("FDT: bad magic {:#x}", magic);
        return;
    }

    let totalsize = unsafe { read_be32_ptr(base.add(4)) } as usize;
    let off_struct = unsafe { read_be32_ptr(base.add(8)) } as usize;
    let off_strings = unsafe { read_be32_ptr(base.add(12)) } as usize;

    let mut pos = unsafe { base.add(off_struct) };
    let end = unsafe { base.add(totalsize) };
    let strings_base = unsafe { base.add(off_strings) };

    let mut depth: usize = 0;
    let mut nodes: [Option<FdtNode<'static>>; MAX_DEPTH] = [None; MAX_DEPTH];

    while (pos as usize) < end as usize {
        let token = unsafe { read_be32_ptr(pos) };
        pos = unsafe { pos.add(4) };

        match token {
            FDT_BEGIN_NODE => {
                let name_start = pos;
                let mut len = 0usize;
                while unsafe { *pos.add(len) } != 0 {
                    len += 1;
                }
                let name = if len > 0 {
                    unsafe {
                        core::str::from_utf8_unchecked(core::slice::from_raw_parts(name_start, len))
                    }
                } else {
                    ""
                };
                pos = unsafe { pos.add((len + 4) & !3) };

                if depth < MAX_DEPTH {
                    let (address_cells, size_cells) = if depth == 0 {
                        (DEFAULT_ADDRESS_CELLS, DEFAULT_SIZE_CELLS)
                    } else if let Some(parent) = nodes[depth - 1] {
                        (parent.child_address_cells, parent.child_size_cells)
                    } else {
                        (DEFAULT_ADDRESS_CELLS, DEFAULT_SIZE_CELLS)
                    };

                    nodes[depth] = Some(FdtNode {
                        name,
                        compatible: None,
                        reg: None,
                        interrupt: None,
                        timebase_frequency: None,
                        address_cells,
                        size_cells,
                        child_address_cells: DEFAULT_ADDRESS_CELLS,
                        child_size_cells: DEFAULT_SIZE_CELLS,
                    });
                }
                depth += 1;
            }
            FDT_END_NODE => {
                if depth > 0 {
                    depth -= 1;
                    if depth < MAX_DEPTH {
                        if let Some(node) = nodes[depth] {
                            callback(node);
                            nodes[depth] = None;
                        }
                    }
                }
            }
            FDT_PROP => {
                if depth == 0 {
                    let len = unsafe { read_be32_ptr(pos) } as usize;
                    pos = unsafe { pos.add(8 + ((len + 3) & !3)) };
                    continue;
                }

                let node_slot = depth - 1;
                if node_slot >= MAX_DEPTH {
                    let len = unsafe { read_be32_ptr(pos) } as usize;
                    pos = unsafe { pos.add(8 + ((len + 3) & !3)) };
                    continue;
                }

                let Some(mut node) = nodes[node_slot] else {
                    let len = unsafe { read_be32_ptr(pos) } as usize;
                    pos = unsafe { pos.add(8 + ((len + 3) & !3)) };
                    continue;
                };

                let len = unsafe { read_be32_ptr(pos) } as usize;
                let nameoff = unsafe { read_be32_ptr(pos.add(4)) } as usize;
                pos = unsafe { pos.add(8) };

                let prop_name = unsafe { fdt_string(strings_base.add(nameoff)) };
                let data = unsafe { core::slice::from_raw_parts(pos, len) };

                match prop_name {
                    "compatible" => node.compatible = Some(data),
                    "reg" => node.reg = Some(data),
                    "interrupts" => {
                        if len >= 4 {
                            node.interrupt = Some(read_be32_slice(&data[0..4]));
                        }
                    }
                    "timebase-frequency" => {
                        if len >= 4 {
                            node.timebase_frequency = Some(read_be32_slice(&data[0..4]));
                        }
                    }
                    "#address-cells" => {
                        if len >= 4 {
                            node.child_address_cells =
                                read_be32_slice(&data[0..4]).min(u8::MAX as u32) as u8;
                        }
                    }
                    "#size-cells" => {
                        if len >= 4 {
                            node.child_size_cells =
                                read_be32_slice(&data[0..4]).min(u8::MAX as u32) as u8;
                        }
                    }
                    _ => {}
                }

                nodes[node_slot] = Some(node);
                pos = unsafe { pos.add((len + 3) & !3) };
            }
            FDT_END => break,
            FDT_NOP => {}
            _ => break,
        }
    }
}

pub fn read_be32_slice(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

pub fn read_be64_slice(bytes: &[u8]) -> u64 {
    u64::from_be_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ])
}

fn read_cells(bytes: &[u8], cells: usize) -> Option<u64> {
    if cells == 0 {
        return Some(0);
    }
    if cells > 2 || bytes.len() < cells * 4 {
        return None;
    }

    let mut value = 0u64;
    for i in 0..cells {
        value = (value << 32) | read_be32_slice(&bytes[i * 4..i * 4 + 4]) as u64;
    }
    Some(value)
}

unsafe fn fdt_string(ptr: *const u8) -> &'static str {
    let mut len = 0usize;
    while unsafe { *ptr.add(len) } != 0 {
        len += 1;
    }
    unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(ptr, len)) }
}

unsafe fn read_be32_ptr(ptr: *const u8) -> u32 {
    unsafe { u32::from_be_bytes([*ptr, *ptr.add(1), *ptr.add(2), *ptr.add(3)]) }
}

fn first_nul_string(data: &[u8]) -> Option<&[u8]> {
    let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
    if end == 0 { None } else { Some(&data[..end]) }
}

struct NulStringIter<'a> {
    data: &'a [u8],
}

impl<'a> NulStringIter<'a> {
    const fn new(data: &'a [u8]) -> Self {
        Self { data }
    }
}

impl<'a> Iterator for NulStringIter<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.data.is_empty() {
                return None;
            }

            let end = self
                .data
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(self.data.len());
            let item = &self.data[..end];
            self.data = if end < self.data.len() {
                &self.data[end + 1..]
            } else {
                &[]
            };

            if !item.is_empty() {
                return Some(item);
            }
        }
    }
}
