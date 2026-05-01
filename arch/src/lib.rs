#![no_std]

#[cfg(target_arch = "arm")]
pub mod arm;

#[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
pub mod riscv;

