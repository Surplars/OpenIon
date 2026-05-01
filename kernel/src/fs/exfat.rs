//! exFAT filesystem parser (read-only for now).
//!
//! exFAT spec: clusters, FAT, directory entries.
//! This is a minimal parser for reading files from an exFAT-formatted block device.

/// Static buffer for cluster I/O (avoids stack overflow with 4KB clusters).
static mut CLUSTER_BUF: [u8; 4096] = [0u8; 4096];

/// Minimal block device interface for filesystem use.
pub trait BlockDev {
    fn read_sector(&self, sector: u64, buf: &mut [u8]) -> Result<(), ()>;
}

/// exFAT Boot Sector (offset 0 of the volume)
#[repr(C, packed)]
struct BootSector {
    jump: [u8; 3],
    oem: [u8; 8],
    _reserved1: [u8; 53],
    partition_offset: u64,
    volume_length: u64,
    fat_offset: u32,
    fat_length: u32,
    cluster_heap_offset: u32,
    cluster_count: u32,
    root_dir_first_cluster: u32,
    volume_serial: u32,
    fs_revision: u16,
    volume_flags: u16,
    bytes_per_sector_shift: u8,
    sectors_per_cluster_shift: u8,
    number_of_fats: u8,
    drive_select: u8,
    percent_in_use: u8,
    _reserved2: [u8; 7],
    boot_signature: u16,
}

/// exFAT Directory Entry types
const ENTRY_TYPE_UNUSED: u8 = 0x00;
const ENTRY_TYPE_FILE: u8 = 0x85;
const ENTRY_TYPE_STREAM: u8 = 0xC0;
const ENTRY_TYPE_NAME: u8 = 0xC1;
const ENTRY_TYPE_VOLUME_LABEL: u8 = 0x83;

/// File attribute flags
const ATTR_READ_ONLY: u16 = 0x01;
const ATTR_HIDDEN: u16 = 0x02;
const ATTR_SYSTEM: u16 = 0x04;
const ATTR_DIRECTORY: u16 = 0x10;
const ATTR_ARCHIVE: u16 = 0x20;

/// exFAT directory entry (32 bytes)
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct DirEntry {
    entry_type: u8,
    data: [u8; 31],
}

/// Parsed file/directory info from exFAT
pub struct ExfatEntry {
    pub name: [u8; 256],
    pub name_len: usize,
    pub first_cluster: u32,
    pub data_length: u64,
    pub is_dir: bool,
}

#[derive(Clone, Copy)]
pub struct ExfatFs {
    bytes_per_sector: u32,
    sectors_per_cluster: u32,
    cluster_heap_offset: u32,
    fat_offset: u32,
    root_dir_first_cluster: u32,
    cluster_count: u32,
}

impl ExfatFs {
    pub fn new() -> Self {
        Self {
            bytes_per_sector: 0,
            sectors_per_cluster: 0,
            cluster_heap_offset: 0,
            fat_offset: 0,
            root_dir_first_cluster: 0,
            cluster_count: 0,
        }
    }

    pub fn root_cluster(&self) -> u32 {
        self.root_dir_first_cluster
    }

    pub fn cluster_count(&self) -> u32 {
        self.cluster_count
    }

    /// Find a directory entry by path relative to the root of this filesystem.
    pub fn find_entry(&self, dev: &dyn BlockDev, path: &str) -> Option<ExfatEntry> {
        let path = path.trim_matches('/');
        if path.is_empty() {
            return Some(ExfatEntry {
                name: [0u8; 256],
                name_len: 0,
                first_cluster: self.root_dir_first_cluster,
                data_length: 0,
                is_dir: true,
            });
        }

        let mut cluster = self.root_dir_first_cluster;
        let parts: [&str; 8] = {
            let mut p = [""; 8];
            let mut i = 0;
            for part in path.split('/') {
                if !part.is_empty() && i < 8 {
                    p[i] = part;
                    i += 1;
                }
            }
            p
        };

        for component in parts.iter() {
            if component.is_empty() { continue; }
            let mut found: Option<ExfatEntry> = None;
            let _ = self.list_dir(dev, cluster, &mut |entry| {
                let name_str = core::str::from_utf8(&entry.name[..entry.name_len]).unwrap_or("");
                if name_str == *component {
                    found = Some(ExfatEntry {
                        name: entry.name,
                        name_len: entry.name_len,
                        first_cluster: entry.first_cluster,
                        data_length: entry.data_length,
                        is_dir: entry.is_dir,
                    });
                }
            });
            match found {
                Some(entry) => {
                    if entry.is_dir {
                        cluster = entry.first_cluster;
                    } else {
                        return Some(entry);
                    }
                }
                None => return None,
            }
        }
        Some(ExfatEntry {
            name: [0u8; 256],
            name_len: 0,
            first_cluster: cluster,
            data_length: 0,
            is_dir: true,
        })
    }

    /// Parse the boot sector and initialize the filesystem.
    pub fn mount(&mut self, dev: &dyn BlockDev) -> Result<(), ()> {
        let mut sector0 = [0u8; 512];
        dev.read_sector(0, &mut sector0).map_err(|_| ())?;

        let bps_shift = sector0[108];
        let spc_shift = sector0[109];
        let fat_offset = u32::from_le_bytes([sector0[80], sector0[81], sector0[82], sector0[83]]);
        let fat_length = u32::from_le_bytes([sector0[84], sector0[85], sector0[86], sector0[87]]);
        let cluster_heap_offset = u32::from_le_bytes([sector0[88], sector0[89], sector0[90], sector0[91]]);
        let cluster_count = u32::from_le_bytes([sector0[92], sector0[93], sector0[94], sector0[95]]);
        let root_dir_cluster = u32::from_le_bytes([sector0[96], sector0[97], sector0[98], sector0[99]]);

        if bps_shift == 0 || spc_shift == 0 {
            return Err(());
        }

        self.bytes_per_sector = 1u32 << bps_shift;
        self.sectors_per_cluster = 1u32 << spc_shift;
        self.fat_offset = fat_offset;
        self.cluster_heap_offset = cluster_heap_offset;
        self.root_dir_first_cluster = root_dir_cluster;
        self.cluster_count = cluster_count;

        crate::kinfo!("exFAT: {} sectors/cluster, {} clusters, root={}",
            self.sectors_per_cluster, self.cluster_count, self.root_dir_first_cluster);

        Ok(())
    }

    fn cluster_to_sector(&self, cluster: u32) -> u64 {
        if cluster < 2 {
            return 0;
        }
        (self.cluster_heap_offset as u64) + ((cluster as u64) - 2) * (self.sectors_per_cluster as u64)
    }

    fn read_cluster(&self, dev: &dyn BlockDev, cluster: u32, buf: &mut [u8]) -> Result<(), ()> {
        let start_sector = self.cluster_to_sector(cluster);
        let sectors = self.sectors_per_cluster;

        for i in 0..sectors {
            let sector = start_sector + i as u64;
            let offset = i as usize * self.bytes_per_sector as usize;
            if offset + self.bytes_per_sector as usize > buf.len() { break; }
            dev.read_sector(sector, &mut buf[offset..offset + self.bytes_per_sector as usize])
                .map_err(|_| ())?;
        }
        Ok(())
    }

    fn next_cluster(&self, dev: &dyn BlockDev, cluster: u32) -> Result<u32, ()> {
        let fat_sector = self.fat_offset + cluster / (self.bytes_per_sector / 4);
        let fat_index = cluster % (self.bytes_per_sector / 4);

        let mut sector_buf = [0u8; 512];
        dev.read_sector(fat_sector as u64, &mut sector_buf).map_err(|_| ())?;

        let offset = (fat_index * 4) as usize;
        let next = u32::from_le_bytes([
            sector_buf[offset],
            sector_buf[offset + 1],
            sector_buf[offset + 2],
            sector_buf[offset + 3],
        ]);

        if next >= 0xFFFFFFF8 { Ok(0) } else { Ok(next) }
    }

    /// List directory entries in a given cluster chain
    pub fn list_dir(
        &self,
        dev: &dyn BlockDev,
        start_cluster: u32,
        callback: &mut dyn FnMut(&ExfatEntry),
    ) -> Result<(), ()> {
        let bytes_per_cluster = (self.bytes_per_sector * self.sectors_per_cluster) as usize;
        let mut cluster = start_cluster;

        while cluster != 0 {
            let buf = unsafe { &mut CLUSTER_BUF[..bytes_per_cluster.min(4096)] };
            self.read_cluster(dev, cluster, buf)?;

            let num_entries = bytes_per_cluster / 32;
            let mut i = 0;
            while i < num_entries {
                let entry_type = buf[i * 32];
                if entry_type == ENTRY_TYPE_UNUSED { break; }
                if entry_type == ENTRY_TYPE_FILE {
                    let file_attrs = u16::from_le_bytes([buf[i * 32 + 4], buf[i * 32 + 5]]);
                    let is_dir = file_attrs & ATTR_DIRECTORY != 0;

                    if i + 1 < num_entries && buf[(i + 1) * 32] == ENTRY_TYPE_STREAM {
                        let stream = &buf[(i + 1) * 32..(i + 2) * 32];
                        let name_len = stream[3] as usize;
                        let first_cluster = u32::from_le_bytes([
                            stream[20], stream[21], stream[22], stream[23],
                        ]);
                        let data_length = u64::from_le_bytes([
                            stream[24], stream[25], stream[26], stream[27],
                            stream[28], stream[29], stream[30], stream[31],
                        ]);

                        let mut name = [0u8; 256];
                        let mut name_pos = 0;
                        let mut j = i + 2;
                        while j < num_entries && buf[j * 32] == ENTRY_TYPE_NAME && name_pos < name_len {
                            let name_entry = &buf[j * 32..(j + 1) * 32];
                            for k in 0..15 {
                                let off = 2 + k * 2;
                                if off + 1 < 32 {
                                    let ch = u16::from_le_bytes([name_entry[off], name_entry[off + 1]]);
                                    if ch != 0 && name_pos < name_len {
                                        if ch < 128 { name[name_pos] = ch as u8; }
                                        else { name[name_pos] = b'?'; }
                                        name_pos += 1;
                                    }
                                }
                            }
                            j += 1;
                        }

                        let entry = ExfatEntry {
                            name,
                            name_len: name_pos.min(name_len),
                            first_cluster,
                            data_length,
                            is_dir,
                        };
                        callback(&entry);
                        i = j;
                        continue;
                    }
                }
                i += 1;
            }
            cluster = self.next_cluster(dev, cluster)?;
        }
        Ok(())
    }

    /// Read file data from a cluster chain into a buffer
    pub fn read_file(
        &self,
        dev: &dyn BlockDev,
        start_cluster: u32,
        buf: &mut [u8],
    ) -> Result<usize, ()> {
        let bytes_per_cluster = (self.bytes_per_sector * self.sectors_per_cluster) as usize;
        let mut cluster = start_cluster;
        let mut offset = 0;

        while cluster != 0 && offset < buf.len() {
            let read_len = bytes_per_cluster.min(buf.len() - offset);
            let cl_buf = unsafe { &mut CLUSTER_BUF[..read_len.min(4096)] };
            self.read_cluster(dev, cluster, cl_buf)?;
            buf[offset..offset + read_len].copy_from_slice(&cl_buf[..read_len]);
            offset += read_len;
            cluster = self.next_cluster(dev, cluster)?;
        }
        Ok(offset)
    }
}