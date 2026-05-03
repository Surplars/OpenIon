# OpenIon

OpenIon is a small bare-metal RTOS written in Rust. It is `no_std`, `no_main`,
and currently targets QEMU-emulated RISC-V and ARM platforms.

The long-term direction is a RISC-V Type-1 hypervisor, but the current stable
focus is the kernel core: scheduler, shell, VFS, block I/O, driver framework,
memory management, and platform/architecture separation.

## Supported Platforms

| Platform | Architecture | QEMU machine | Crate |
|---|---|---|---|
| `qemu-virt-riscv` | RISC-V 64 `rv64imac` | `qemu-system-riscv64 -machine virt` | `platform/qemu-virt-riscv` |
| `qemu-an521` | ARM Cortex-M33 | `qemu-system-arm -M mps2-an521` | `platform/qemu-an521` |

## Prerequisites

- Rust nightly, selected by `rust-toolchain.toml`
- QEMU, depending on target:
  - `qemu-system-riscv64`
  - `qemu-system-arm`
- RISC-V target: `riscv64imac-unknown-none-elf`
- ARM target: `thumbv8m.main-none-eabihf`

## Build

```bash
make build PLAT=qemu-virt-riscv
make build PLAT=qemu-an521
```

The root workspace `default-members` are only `app`, `arch`, and `kernel`.
Platform binaries must be built explicitly with `make build PLAT=...` or
`cargo build -p <platform-crate> --target <target>`.

## Run In QEMU

```bash
make run PLAT=qemu-virt-riscv
make run PLAT=qemu-an521
```

Use `Ctrl-A X` to exit QEMU in `-nographic` mode.

For the RISC-V platform, the Makefile attaches `sd.img` as a VirtIO block
device. If the image is missing or not exFAT-formatted, mounting it from the
shell should fail with a normal error instead of hanging.

## Shell Smoke Test

After booting `qemu-virt-riscv`, the following commands should return to the
shell prompt without hanging:

```text
ls /dev
mount
ls /sd
mount /dev/blk0 /sd
mount
ls /
ls /dev
ls /sd
cd /sd
ls
```

The shell supports basic path handling, `cd`, `ls`, file reads from RAMFS and
mounted exFAT, mount listing, block-device mounting, and tab completion across
directories.

## Project Layout

| Path | Role |
|---|---|
| `kernel/` | Architecture-neutral kernel core: scheduler, IRQ table, memory, VFS, driver framework, networking framework, logging, versioning |
| `arch/` | ISA/CPU-specific code: RISC-V traps, CSRs, context switch, SBI helpers, ARM Cortex-M context and NVIC/SysTick code |
| `platform/` | Board/SoC binaries: linker scripts, startup assembly, platform MMIO addresses, PLIC/NVIC wiring, platform timers |
| `drivers/` | Device driver crates: UART, VirtIO block, LAN9118 Ethernet |
| `app/` | Root task and shell-facing application code |
| `bootloader/` | Placeholder for future bootloader work |

## Current Kernel Features

- Cooperative scheduler with priority-aware ready queues and high-priority
  preemption points.
- Interactive shell using an IRQ producer and shell consumer UART RX path.
- RAMFS-based VFS with stable `NodeId` handles.
- Mount table snapshots to avoid printing or block I/O while holding locks.
- Read-only exFAT mounting over VirtIO block on `qemu-virt-riscv`.
- Driver registry with snapshot APIs and FDT auto-probing.
- Fixed-capacity structures on core paths for MCU compatibility.
- RISC-V S-mode boot on RustSBI/OpenSBI-style firmware by default.

## Driver Framework

Drivers implement `kernel::driver::Driver` and optionally a device-class trait:

- `kernel::driver::char::CharDevice`
- `kernel::driver::block::BlockDevice`
- `kernel::driver::net::NetDevice`

FDT-probed drivers implement `DriverFactory`. Probe inputs are represented by
`DeviceResource` rather than raw `base_addr, irq` pairs. Static probed driver
instances should use `StaticDriverPool` instead of open-coded
`UnsafeCell<MaybeUninit<T>>` pools.

The driver manager provides snapshots for iteration and IRQ dispatch so callers
do not print, call back into drivers, or perform block I/O while holding the
registry lock.

## RISC-V Notes

`qemu-virt-riscv` defaults to `s-mode`. The platform receives `hartid` and
`dtb_pa` from firmware; if no DTB address is provided, it falls back to the
legacy QEMU address used by this tree.

RISC-V CSR access, SBI calls, trap setup, and timer interrupt enables live under
`arch/src/riscv`. QEMU virt MMIO details such as PLIC and CLINT addresses remain
under `platform/qemu-virt-riscv`.

## Hypervisor Status

The RISC-V hypervisor code under `arch/src/riscv/hypervisor` is experimental.
It is not yet the primary boot path. Keep the stable kernel, shell, VFS, driver,
and scheduler paths working before expanding hypervisor functionality.

## License

MIT License. See `LICENSE`.
