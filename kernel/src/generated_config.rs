//! Auto-generated configuration file.
//! Do not edit manually - use ionix TUI instead.

#![allow(unused)]

/// Kernel scheduler tick frequency in Hz. Platforms decide how this maps to hardware timers.
pub const OPENION_SYSTICK_HZ: u32 = 1000;

/// Maximum number of scheduler task control blocks.
pub const OPENION_TASK_CAP: usize = 32;

/// Enable symmetric multi-processing support. When disabled, the scheduler uses a single global run queue.
pub const OPENION_SMP: bool = false;

/// Maximum number of CPUs supported. Per-CPU data structures are allocated for this many CPUs.
pub const OPENION_SMP_MAX_CPUS: usize = 4;

/// Root task stack capacity measured in machine words.
pub const OPENION_ROOT_STACK_WORDS: usize = 1024;

/// Idle task stack capacity measured in machine words.
pub const OPENION_IDLE_STACK_WORDS: usize = 256;

/// Enable the cooperative Rust Future executor running on top of the RTOS scheduler.
pub const OPENION_ASYNC_RT: bool = true;

/// Fixed number of async task slots managed by the kernel executor.
pub const OPENION_ASYNC_TASK_SLOTS: usize = 8;

/// Static kernel heap size in bytes.
pub const OPENION_KERNEL_HEAP_SIZE: usize = 65536;

/// Maximum number of RAMFS vnode objects.
pub const OPENION_RAMFS_NODE_CAP: usize = 64;

/// Maximum inline bytes stored by one RAMFS file node.
pub const OPENION_RAMFS_FILE_MAX_SIZE: usize = 4096;

/// Enable the built-in interactive shell task.
pub const OPENION_BUILTIN_SHELL: bool = true;

/// Stack size in bytes for the built-in shell task.
pub const OPENION_SHELL_STACK_SIZE: usize = 32768;

/// Enable generic flattened device tree parsing support.
pub const OPENION_FDT: bool = true;

/// Scan FDT and instantiate matching driver factories during boot.
pub const OPENION_FDT_AUTO_PROBE: bool = true;

/// Size of the generic IRQ handler table.
pub const OPENION_EXTERNAL_IRQ_COUNT: usize = 64;

/// Build RISC-V code for a 64-bit hart and Sv39-capable address space.
pub const OPENION_RISCV_XLEN_64: bool = true;

/// Build RISC-V code for a 32-bit hart and Sv32-capable address space.
pub const OPENION_RISCV_XLEN_32: bool = false;

/// Build RISC-V code for Supervisor mode on SBI firmware.
pub const OPENION_RISCV_S_MODE: bool = true;

/// Build RISC-V code for Machine mode. Experimental for this tree.
pub const OPENION_RISCV_M_MODE: bool = false;

/// Physical load/link address for riscv-generic kernels. OpenSBI or the loader must jump here.
pub const OPENION_RISCV_KERNEL_BASE: usize = 2149580800;

/// Enable an early Sv32 identity map for RV32 S-mode kernels.
pub const OPENION_RISCV_SV32_MMU: bool = false;

/// Enable an early Sv39 identity map for RV64 S-mode kernels.
pub const OPENION_RISCV_SV39_MMU: bool = true;

/// Enable the RISC-V M extension. Disabling this builds closer to RV64IA/RV64IAC and may expose compiler/runtime assumptions.
pub const OPENION_RISCV_EXT_M: bool = true;

/// Enable the RISC-V A extension for native atomic instructions.
pub const OPENION_RISCV_EXT_A: bool = true;

/// Enable the compressed instruction set extension. Disable this to build RISC-V code with IMA only.
pub const OPENION_RISCV_EXT_C: bool = true;

/// Enable the RISC-V F extension for compiler-generated single-precision floating-point instructions. Kernel FPU context handling is not part of the stable path.
pub const OPENION_RISCV_EXT_F: bool = false;

/// Enable the RISC-V D extension. This requires F and should remain off unless FPU state handling is being validated.
pub const OPENION_RISCV_EXT_D: bool = false;

/// Enable the LLVM/Rust B bundle, covering the common Zba, Zbb, and Zbs bit-manipulation extensions.
pub const OPENION_RISCV_EXT_B: bool = true;

/// Enable the RISC-V V extension for vector code generation. Leave off unless vector state save/restore is being worked on.
pub const OPENION_RISCV_EXT_V: bool = false;

/// Enable the separated Zicsr extension for CSR instructions.
pub const OPENION_RISCV_EXT_ZICSR: bool = true;

/// Enable the separated Zifencei extension for fence.i.
pub const OPENION_RISCV_EXT_ZIFENCEI: bool = true;

/// Compile experimental RISC-V H-extension hypervisor support. This does not make it the default boot path.
pub const OPENION_RISCV_HYPERVISOR: bool = false;

/// Enable the NS16550A UART driver crate for platforms that instantiate it.
pub const OPENION_DRIVER_NS16550A: bool = true;

/// Enable the CMSDK UART driver crate for ARM MPS2-style platforms.
pub const OPENION_DRIVER_CMSDK_UART: bool = false;

/// Allow legacy VirtIO MMIO devices. Modern VERSION_1 devices remain preferred.
pub const OPENION_VIRTIO_MMIO_LEGACY: bool = false;

/// Enable the VirtIO MMIO block driver.
pub const OPENION_DRIVER_VIRTIO_BLK: bool = true;

/// Maximum polling iterations for one synchronous VirtIO block request before returning an error.
pub const OPENION_VIRTIO_BLK_POLL_LIMIT: usize = 2000000;

/// Enable the VirtIO MMIO GPU driver and expose it as a framebuffer device.
pub const OPENION_DRIVER_VIRTIO_GPU: bool = true;

/// Maximum polling iterations for one synchronous VirtIO GPU control request before returning an error.
pub const OPENION_VIRTIO_GPU_POLL_LIMIT: usize = 2000000;

/// Enable the VirtIO MMIO entropy driver for platforms that expose virtio-rng devices.
pub const OPENION_DRIVER_VIRTIO_RNG: bool = true;

/// Enable the LAN9118 Ethernet driver.
pub const OPENION_DRIVER_LAN9118: bool = false;

/// Kernel network backend. Supported: ionnet, smoltcp.
pub const OPENION_NET_BACKEND: &str = "ionnet";

