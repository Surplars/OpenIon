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

/// Parsed exFAT boot sector fields used by the read-only driver.
struct BootSector {
    oem: [u8; 8],
    volume_length: u64,
    fat_offset: u32,
    fat_length: u32,
    cluster_heap_offset: u32,
    cluster_count: u32,
    root_dir_first_cluster: u32,
    bytes_per_sector_shift: u8,
    sectors_per_cluster_shift: u8,
    number_of_fats: u8,
    boot_signature: u16,
}

impl BootSector {
    fn parse(sector: &[u8; 512]) -> Self {
        Self {
            oem: [
                sector[3], sector[4], sector[5], sector[6], sector[7], sector[8], sector[9],
                sector[10],
            ],
            volume_length: le_u64(sector, 72),
            fat_offset: le_u32(sector, 80),
            fat_length: le_u32(sector, 84),
            cluster_heap_offset: le_u32(sector, 88),
            cluster_count: le_u32(sector, 92),
            root_dir_first_cluster: le_u32(sector, 96),
            bytes_per_sector_shift: sector[108],
            sectors_per_cluster_shift: sector[109],
            number_of_fats: sector[110],
            boot_signature: le_u16(sector, 510),
        }
    }
}

/// exFAT Directory Entry types
const ENTRY_TYPE_UNUSED: u8 = 0x00;
const ENTRY_TYPE_FILE: u8 = 0x85;
const ENTRY_TYPE_STREAM: u8 = 0xC0;
const ENTRY_TYPE_NAME: u8 = 0xC1;
const ENTRY_TYPE_VOLUME_LABEL: u8 = 0x83;

/// Stream extension flags.
const STREAM_FLAG_NO_FAT_CHAIN: u8 = 1 << 1;

/// File attribute flags
const ATTR_READ_ONLY: u16 = 0x01;
const ATTR_HIDDEN: u16 = 0x02;
const ATTR_SYSTEM: u16 = 0x04;
const ATTR_DIRECTORY: u16 = 0x10;
const ATTR_ARCHIVE: u16 = 0x20;

/// exFAT directory entry (32 bytes)
#[derive(Clone, Copy)]
struct DirEntry {
    entry_type: u8,
    data: [u8; 31],
}

impl DirEntry {
    fn parse(buf: &[u8], idx: usize) -> Option<Self> {
        let start = idx.checked_mul(32)?;
        let end = start.checked_add(32)?;
        if end > buf.len() {
            return None;
        }

        let mut data = [0u8; 31];
        data.copy_from_slice(&buf[start + 1..end]);
        Some(Self {
            entry_type: buf[start],
            data,
        })
    }

    fn u16_at(&self, offset: usize) -> u16 {
        let i = offset.saturating_sub(1);
        u16::from_le_bytes([self.data[i], self.data[i + 1]])
    }

    fn u32_at(&self, offset: usize) -> u32 {
        let i = offset.saturating_sub(1);
        u32::from_le_bytes([
            self.data[i],
            self.data[i + 1],
            self.data[i + 2],
            self.data[i + 3],
        ])
    }

    fn u64_at(&self, offset: usize) -> u64 {
        let i = offset.saturating_sub(1);
        u64::from_le_bytes([
            self.data[i],
            self.data[i + 1],
            self.data[i + 2],
            self.data[i + 3],
            self.data[i + 4],
            self.data[i + 5],
            self.data[i + 6],
            self.data[i + 7],
        ])
    }
}

/// Parsed file/directory info from exFAT
pub struct ExfatEntry {
    pub name: [u8; 256],
    pub name_len: usize,
    pub first_cluster: u32,
    pub data_length: u64,
    pub is_dir: bool,
    pub attributes: u16,
    pub no_fat_chain: bool,
}

impl ExfatEntry {
    pub fn is_read_only(&self) -> bool {
        self.attributes & ATTR_READ_ONLY != 0
    }

    pub fn is_hidden(&self) -> bool {
        self.attributes & ATTR_HIDDEN != 0
    }

    pub fn is_system(&self) -> bool {
        self.attributes & ATTR_SYSTEM != 0
    }

    pub fn is_archive(&self) -> bool {
        self.attributes & ATTR_ARCHIVE != 0
    }
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
                attributes: ATTR_DIRECTORY,
                no_fat_chain: false,
            });
        }

        let mut current = ExfatEntry {
            name: [0u8; 256],
            name_len: 0,
            first_cluster: self.root_dir_first_cluster,
            data_length: 0,
            is_dir: true,
            attributes: ATTR_DIRECTORY,
            no_fat_chain: false,
        };
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
            if component.is_empty() {
                continue;
            }
            let mut found: Option<ExfatEntry> = None;
            let _ = self.list_entry_dir(dev, &current, &mut |entry| {
                let safe_name_len = entry.name_len.min(255);
                let name_str = core::str::from_utf8(&entry.name[..safe_name_len]).unwrap_or("");
                if name_str == *component {
                    found = Some(ExfatEntry {
                        name: entry.name,
                        name_len: entry.name_len,
                        first_cluster: entry.first_cluster,
                        data_length: entry.data_length,
                        is_dir: entry.is_dir,
                        attributes: entry.attributes,
                        no_fat_chain: entry.no_fat_chain,
                    });
                }
            });
            match found {
                Some(entry) => {
                    if entry.is_dir {
                        current = entry;
                    } else {
                        return Some(entry);
                    }
                }
                None => return None,
            }
        }
        Some(current)
    }

    /// Parse the boot sector and initialize the filesystem.
    pub fn mount(&mut self, dev: &dyn BlockDev) -> Result<(), ()> {
        let mut sector0 = [0u8; 512];
        dev.read_sector(0, &mut sector0).map_err(|_| ())?;
        let boot = BootSector::parse(&sector0);

        if boot.oem != *b"EXFAT   " {
            return Err(());
        }

        if boot.boot_signature != 0xAA55 {
            return Err(());
        }

        if !(9..=12).contains(&boot.bytes_per_sector_shift) || boot.sectors_per_cluster_shift > 25 {
            return Err(());
        }

        let bytes_per_sector = 1u32
            .checked_shl(boot.bytes_per_sector_shift as u32)
            .ok_or(())?;
        let sectors_per_cluster = 1u32
            .checked_shl(boot.sectors_per_cluster_shift as u32)
            .ok_or(())?;
        if bytes_per_sector != 512
            || sectors_per_cluster == 0
            || bytes_per_sector.saturating_mul(sectors_per_cluster) as usize > 4096
        {
            return Err(());
        }

        if boot.volume_length == 0
            || boot.number_of_fats == 0
            || boot.fat_offset == 0
            || boot.fat_length == 0
            || boot.cluster_heap_offset == 0
            || boot.cluster_count == 0
            || boot.root_dir_first_cluster < 2
            || boot.root_dir_first_cluster >= boot.cluster_count + 2
        {
            return Err(());
        }

        self.bytes_per_sector = bytes_per_sector;
        self.sectors_per_cluster = sectors_per_cluster;
        self.fat_offset = boot.fat_offset;
        self.cluster_heap_offset = boot.cluster_heap_offset;
        self.root_dir_first_cluster = boot.root_dir_first_cluster;
        self.cluster_count = boot.cluster_count;

        crate::kinfo!(
            "exFAT: {} sectors/cluster, {} clusters, root={}",
            self.sectors_per_cluster,
            self.cluster_count,
            self.root_dir_first_cluster
        );

        Ok(())
    }

    fn cluster_to_sector(&self, cluster: u32) -> u64 {
        if cluster < 2 {
            return 0;
        }
        (self.cluster_heap_offset as u64)
            + ((cluster as u64) - 2) * (self.sectors_per_cluster as u64)
    }

    fn read_cluster(&self, dev: &dyn BlockDev, cluster: u32, buf: &mut [u8]) -> Result<(), ()> {
        let start_sector = self.cluster_to_sector(cluster);
        let sectors = self.sectors_per_cluster;

        for i in 0..sectors {
            let sector = start_sector + i as u64;
            let offset = i as usize * self.bytes_per_sector as usize;
            if offset + self.bytes_per_sector as usize > buf.len() {
                break;
            }
            dev.read_sector(
                sector,
                &mut buf[offset..offset + self.bytes_per_sector as usize],
            )
            .map_err(|_| ())?;
        }
        Ok(())
    }

    fn next_cluster(&self, dev: &dyn BlockDev, cluster: u32) -> Result<u32, ()> {
        let fat_sector = self.fat_offset + cluster / (self.bytes_per_sector / 4);
        let fat_index = cluster % (self.bytes_per_sector / 4);

        let mut sector_buf = [0u8; 512];
        dev.read_sector(fat_sector as u64, &mut sector_buf)
            .map_err(|_| ())?;

        let offset = (fat_index * 4) as usize;
        let next = u32::from_le_bytes([
            sector_buf[offset],
            sector_buf[offset + 1],
            sector_buf[offset + 2],
            sector_buf[offset + 3],
        ]);

        if next >= 0xFFFFFFF8 { Ok(0) } else { Ok(next) }
    }

    fn next_cluster_for_entry(
        &self,
        dev: &dyn BlockDev,
        entry: &ExfatEntry,
        cluster: u32,
    ) -> Result<u32, ()> {
        if entry.no_fat_chain {
            let bytes_per_cluster = (self.bytes_per_sector * self.sectors_per_cluster) as u64;
            let clusters = entry.data_length.div_ceil(bytes_per_cluster).max(1);
            let offset = cluster.saturating_sub(entry.first_cluster) as u64;
            if offset + 1 >= clusters {
                Ok(0)
            } else {
                Ok(cluster + 1)
            }
        } else {
            self.next_cluster(dev, cluster)
        }
    }

    /// List directory entries in a given cluster chain
    pub fn list_dir(
        &self,
        dev: &dyn BlockDev,
        start_cluster: u32,
        callback: &mut dyn FnMut(&ExfatEntry),
    ) -> Result<(), ()> {
        let entry = ExfatEntry {
            name: [0u8; 256],
            name_len: 0,
            first_cluster: start_cluster,
            data_length: 0,
            is_dir: true,
            attributes: ATTR_DIRECTORY,
            no_fat_chain: false,
        };
        self.list_entry_dir(dev, &entry, callback)
    }

    /// List directory entries using the directory stream metadata.
    pub fn list_entry_dir(
        &self,
        dev: &dyn BlockDev,
        dir: &ExfatEntry,
        callback: &mut dyn FnMut(&ExfatEntry),
    ) -> Result<(), ()> {
        let bytes_per_cluster = (self.bytes_per_sector * self.sectors_per_cluster) as usize;
        let mut cluster = dir.first_cluster;

        while cluster != 0 {
            let buf = unsafe { &mut CLUSTER_BUF[..bytes_per_cluster.min(4096)] };
            self.read_cluster(dev, cluster, buf)?;

            let num_entries = bytes_per_cluster / 32;
            let mut i = 0;
            while i < num_entries {
                let Some(file_entry) = DirEntry::parse(buf, i) else {
                    break;
                };
                if file_entry.entry_type == ENTRY_TYPE_UNUSED {
                    break;
                }
                if file_entry.entry_type == ENTRY_TYPE_VOLUME_LABEL {
                    i += 1;
                    continue;
                }
                if file_entry.entry_type == ENTRY_TYPE_FILE {
                    let file_attrs = file_entry.u16_at(4);
                    let is_dir = file_attrs & ATTR_DIRECTORY != 0;

                    if i + 1 < num_entries {
                        let Some(stream) = DirEntry::parse(buf, i + 1) else {
                            break;
                        };
                        if stream.entry_type != ENTRY_TYPE_STREAM {
                            i += 1;
                            continue;
                        }

                        let flags = stream.data[0];
                        let name_len = stream.data[2] as usize;
                        let first_cluster = stream.u32_at(20);
                        let data_length = stream.u64_at(24);

                        let mut name = [0u8; 256];
                        let mut name_pos = 0;
                        let mut j = i + 2;
                        while j < num_entries && name_pos < name_len {
                            let Some(name_entry) = DirEntry::parse(buf, j) else {
                                break;
                            };
                            if name_entry.entry_type != ENTRY_TYPE_NAME {
                                break;
                            }
                            for k in 0..15 {
                                let off = 2 + k * 2;
                                if off <= 30 {
                                    let ch = name_entry.u16_at(off);
                                    if ch != 0 && name_pos < name_len {
                                        if ch < 128 {
                                            name[name_pos] = ch as u8;
                                        } else {
                                            name[name_pos] = b'?';
                                        }
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
                            attributes: file_attrs,
                            no_fat_chain: flags & STREAM_FLAG_NO_FAT_CHAIN != 0,
                        };
                        callback(&entry);
                        i = j;
                        continue;
                    }
                }
                i += 1;
            }
            cluster = self.next_cluster_for_entry(dev, dir, cluster)?;
        }
        Ok(())
    }

    /// Read file data from a cluster chain into a buffer
    pub fn read_file(
        &self,
        dev: &dyn BlockDev,
        start_cluster: u32,
        data_length: u64,
        no_fat_chain: bool,
        buf: &mut [u8],
    ) -> Result<usize, ()> {
        let bytes_per_cluster = (self.bytes_per_sector * self.sectors_per_cluster) as usize;
        let mut cluster = start_cluster;
        let mut offset = 0;
        let target_len = (data_length as usize).min(buf.len());
        let chain_entry = ExfatEntry {
            name: [0u8; 256],
            name_len: 0,
            first_cluster: start_cluster,
            data_length,
            is_dir: false,
            attributes: 0,
            no_fat_chain,
        };

        while cluster != 0 && offset < target_len {
            let read_len = bytes_per_cluster.min(target_len - offset);
            let cl_buf = unsafe { &mut CLUSTER_BUF[..bytes_per_cluster.min(4096)] };
            self.read_cluster(dev, cluster, cl_buf)?;
            buf[offset..offset + read_len].copy_from_slice(&cl_buf[..read_len]);
            offset += read_len;
            cluster = self.next_cluster_for_entry(dev, &chain_entry, cluster)?;
        }
        Ok(offset)
    }
}

fn le_u16(buf: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([buf[offset], buf[offset + 1]])
}

fn le_u32(buf: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        buf[offset],
        buf[offset + 1],
        buf[offset + 2],
        buf[offset + 3],
    ])
}

fn le_u64(buf: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes([
        buf[offset],
        buf[offset + 1],
        buf[offset + 2],
        buf[offset + 3],
        buf[offset + 4],
        buf[offset + 5],
        buf[offset + 6],
        buf[offset + 7],
    ])
}
