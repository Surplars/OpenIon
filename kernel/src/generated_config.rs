//! Auto-generated configuration file.
//! Do not edit manually - use ionix TUI instead.

#![allow(unused)]

/// Platform crate to build. Supported: qemu-virt-riscv, qemu-an521.
pub const OPENION_PLATFORM: &str = "qemu-virt-riscv";

/// Rust target triple passed to cargo.
pub const OPENION_TARGET: &str = "riscv64imac-unknown-none-elf";

/// Build the RISC-V platform in Supervisor mode on SBI firmware.
pub const OPENION_RISCV_S_MODE: bool = true;

/// Build the RISC-V platform in Machine mode. Experimental for this tree.
pub const OPENION_RISCV_M_MODE: bool = false;

/// Kernel network backend. Supported: ionnet, smoltcp.
pub const OPENION_NET_BACKEND: &str = "ionnet";

/// Kernel tick frequency in Hz.
pub const OPENION_SYSTICK_HZ: u32 = 1000;

/// Size of the generic IRQ handler table.
pub const OPENION_EXTERNAL_IRQ_COUNT: usize = 64;

/// Enable the built-in interactive shell.
pub const OPENION_BUILTIN_SHELL: bool = true;

/// Scan FDT and instantiate matching driver factories during boot.
pub const OPENION_FDT_AUTO_PROBE: bool = true;

/// RISC-V QEMU virt timer frequency in Hz.
pub const OPENION_QEMU_VIRT_RISCV_CPU_HZ: u32 = 10000000;

/// RISC-V QEMU virt RAM base.
pub const OPENION_QEMU_VIRT_RISCV_MEMORY_BASE: usize = 0x8000_0000;

/// RISC-V QEMU virt RAM size in bytes.
pub const OPENION_QEMU_VIRT_RISCV_MEMORY_SIZE: usize = 134217728;

/// Fallback DTB physical address if firmware does not pass one.
pub const OPENION_QEMU_VIRT_RISCV_DTB_ADDR: usize = 0x8006_8000;

/// NS16550A UART0 MMIO base.
pub const OPENION_QEMU_VIRT_RISCV_UART0_BASE: usize = 0x1000_0000;

/// NS16550A UART0 PLIC IRQ.
pub const OPENION_QEMU_VIRT_RISCV_UART0_IRQ: u32 = 10;

/// VirtIO block PLIC IRQ for the default QEMU device.
pub const OPENION_QEMU_VIRT_RISCV_VIRTIO_BLK_IRQ: u32 = 1;

/// QEMU virt PLIC MMIO base.
pub const OPENION_QEMU_VIRT_RISCV_PLIC_BASE: usize = 0x0c00_0000;

/// QEMU virt CLINT MMIO base.
pub const OPENION_QEMU_VIRT_RISCV_CLINT_BASE: usize = 0x0200_0000;

/// AN521 CPU clock in Hz.
pub const OPENION_QEMU_AN521_CPU_HZ: u32 = 25000000;

/// AN521 RAM base.
pub const OPENION_QEMU_AN521_MEMORY_BASE: usize = 0x8000_0000;

/// AN521 RAM size in bytes.
pub const OPENION_QEMU_AN521_MEMORY_SIZE: usize = 16777216;

/// CMSDK UART MMIO base.
pub const OPENION_QEMU_AN521_UART_BASE: usize = 0x4020_0000;

/// CMSDK UART IRQ number.
pub const OPENION_QEMU_AN521_UART_IRQ: u32 = 0;

/// LAN9118 MMIO base.
pub const OPENION_QEMU_AN521_LAN9118_BASE: usize = 0x4200_0000;

/// LAN9118 IRQ number.
pub const OPENION_QEMU_AN521_LAN9118_IRQ: u32 = 48;

