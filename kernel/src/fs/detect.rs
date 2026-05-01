//! Filesystem type auto-detection.
//!
//! Reads the first sector (block 0) of a block device and attempts to
//! identify the filesystem type from known signatures.

use crate::driver::block::DynBlockDevice;
use crate::driver::block::BlockDevice;

/// Supported filesystem types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsType {
    Exfat,
    Unknown,
}

impl FsType {
    pub fn name(&self) -> &'static str {
        match self {
            FsType::Exfat => "exFAT",
            FsType::Unknown => "unknown",
        }
    }
}

/// Result of filesystem detection.
pub struct DetectResult {
    pub fs_type: FsType,
    pub sector0: [u8; 512],
    pub sector0_valid: bool,
}

/// Probe a block device and try to identify the filesystem.
/// Returns the detection result including the raw sector 0 data for debug.
pub fn detect_fs(dev: &DynBlockDevice) -> DetectResult {
    let mut sector0 = [0u8; 512];
    let sector0_valid = dev.read_block(0, &mut sector0).is_ok();

    if !sector0_valid {
        crate::kdebug!("FS detect: {} block 0 read failed", dev.name());
        return DetectResult { fs_type: FsType::Unknown, sector0, sector0_valid: false };
    }

    // Print first 64 bytes of sector 0 as hex dump for debugging
    let mut hex = [0u8; 128];
    let mut hex_len = 0;
    for i in 0..64 {
        if i % 16 == 0 && i > 0 {
            // newline in hex dump — skip, just continue
        }
        let b = sector0[i];
        if hex_len + 3 < hex.len() {
            let h = b >> 4;
            let l = b & 0xF;
            if hex_len > 0 && hex_len < hex.len() {
                hex[hex_len] = b' ';
                hex_len += 1;
            }
            hex[hex_len] = if h < 10 { b'0' + h } else { b'A' + h - 10 };
            hex_len += 1;
            hex[hex_len] = if l < 10 { b'0' + l } else { b'A' + l - 10 };
            hex_len += 1;
        }
    }
    let hex_str = core::str::from_utf8(&hex[..hex_len]).unwrap_or("?");
    crate::kdebug!("FS detect: {} sector0[0..64]: {}", dev.name(), hex_str);

    // Try exFAT detection
    if try_detect_exfat(&sector0) {
        crate::kinfo!("FS detect: {} identified as exFAT", dev.name());
        return DetectResult { fs_type: FsType::Exfat, sector0, sector0_valid: true };
    }

    crate::kdebug!("FS detect: {} not recognized (bps_shift={}, spc_shift={})",
        dev.name(), sector0[108], sector0[109]);

    DetectResult { fs_type: FsType::Unknown, sector0, sector0_valid: true }
}

/// Check if sector 0 looks like an exFAT boot sector.
fn try_detect_exfat(sector0: &[u8; 512]) -> bool {
    // exFAT boot sector layout:
    // bytes 0-2: jump instruction (typically EB 76 90)
    // bytes 3-10: OEM name "EXFAT   "
    // byte 108: bytes_per_sector_shift (must not be 0, typically 9 for 512)
    // byte 109: sectors_per_cluster_shift (must not be 0)
    // bytes 510-511: boot signature 0x55 0xAA

    // Check boot signature
    if sector0[510] != 0x55 || sector0[511] != 0xAA {
        crate::kdebug!("FS detect: exFAT boot sig mismatch: {:02X}{:02X} (expected 55AA)",
            sector0[510], sector0[511]);
        return false;
    }

    // Check OEM name field (bytes 3-10 should be "EXFAT   ")
    let oem = &sector0[3..11];
    if oem == b"EXFAT   " {
        if sector0[108] != 0 && sector0[109] != 0 {
            return true;
        }
        crate::kdebug!("FS detect: exFAT bps/spc shift is 0");
        return false;
    }

    // Also accept without OEM check: just check bps/spc shift and boot sig
    if sector0[108] != 0 && sector0[109] != 0 {
        return true;
    }

    false
}
