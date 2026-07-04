# OpenIon

OpenIon is a small bare-metal RTOS written in Rust. It is `no_std`, `no_main`,
and currently targets QEMU-emulated RISC-V/ARM platforms plus early
STM32F103 bare-metal bring-up.

The long-term direction is a RISC-V Type-1 hypervisor, but the current stable
focus is the kernel core: scheduler, shell, VFS, block I/O, driver framework,
memory management, and platform/architecture separation.

## Supported Platforms

| Platform | Architecture | QEMU machine | Crate |
|---|---|---|---|
| `riscv-generic` | RISC-V `rv64imac`/`rv32imac` | `qemu-system-riscv{32,64} -machine virt` | `platform/riscv-generic` |
| `qemu-an521` | ARM Cortex-M33 | `qemu-system-arm -M mps2-an521` | `platform/qemu-an521` |
| `stm32f103-bluepill` | ARM Cortex-M3 | hardware target, no QEMU runner | `platform/stm32f103-bluepill` |

`riscv-generic` supports RV64 and RV32 S-mode boot on SBI firmware. It has
PLIC and CLIC interrupt paths, DTB-driven memory/timer/IRQ discovery, and
optional early Sv32/Sv39 identity maps.

## Prerequisites

- Rust nightly, selected by `rust-toolchain.toml`
- QEMU, depending on target:
  - `qemu-system-riscv64`
  - `qemu-system-arm`
- RISC-V target: `riscv64imac-unknown-none-elf`
- ARM targets: `thumbv8m.main-none-eabihf`, `thumbv7m-none-eabi`

## Build

```bash
make config
make menuconfig
make build PLAT=riscv-generic
make build PLAT=qemu-an521
make build PLAT=stm32f103-bluepill
```

For RV32 S-mode hardware, configure the RISC-V platform as RV32 S-mode and set
the kernel base to the firmware jump address. A typical OpenSBI `fw_jump` style
configuration uses:

```text
OPENION_RISCV_XLEN_32 = true
OPENION_RISCV_S_MODE = true
OPENION_RISCV_KERNEL_BASE = 0x50000000
OPENION_RISCV_SV32_MMU = true
```

Build the raw kernel image for the bootloader/OpenSBI chain:

```bash
make build PLAT=riscv-generic
rust-objcopy -O binary target/riscv32imac-unknown-none-elf/debug/riscv-generic rtos.bin
```

A board bootloader can load that raw image to the configured kernel base;
OpenSBI should be built as `fw_jump` with `FW_JUMP_ADDR` matching
`OPENION_RISCV_KERNEL_BASE`.

The Makefile delegates to `xtask`, which first generates
`kernel/src/generated_config.rs` from the Ionix schema and config files:

| Path | Role |
|---|---|
| `config/openion.schema.toml` | Typed kernel/platform configuration schema |
| `.config.toml` | Active generated/edited configuration created from schema defaults |
| `.config.old.toml` | Backup written before `.config.toml` changes |
| `kernel/src/generated_config.rs` | Generated `no_std` Rust constants used by the kernel and platforms |
| `utils/ionix/` | Configuration tool, usable from CLI or Rust API |
| `xtask/` | Host build/config orchestration |

The root `.cargo/config.toml` defaults to a bare-metal target, so `xtask` is
always launched with the host target through `HOST_TARGET`. Override it if your
host differs:

```bash
make build PLAT=riscv-generic HOST_TARGET=x86_64-unknown-linux-gnu
```

The root workspace `default-members` are only `app`, `arch`, and `kernel`.
Platform binaries should be built through `make build PLAT=...` so Ionix config
generation, Cargo features, and target triples stay in sync.

`make config` creates or refreshes `.config.toml` and regenerates
`kernel/src/generated_config.rs`. `make menuconfig` opens the Ionix TUI for the
same schema/config pair and writes `.config.old.toml` before replacing an
existing config.

## Run In QEMU

```bash
make run PLAT=riscv-generic
make run PLAT=qemu-an521
```

Use `Ctrl-A X` to exit QEMU in `-nographic` mode.

For the RISC-V platform, the Makefile attaches `sd.img` as a VirtIO block
device. If the image is missing or not exFAT-formatted, mounting it from the
shell should fail with a normal error instead of hanging.

## Shell Smoke Test

After booting `riscv-generic`, the following commands should return to the
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
| `bsp/` | Board support code and MCU HAL bridge glue that is too board-specific for reusable driver crates |
| `drivers/` | Reusable device driver crates: UART protocols, VirtIO block/GPU/RNG, LAN9118 Ethernet |
| `app/` | Root task and shell-facing application code |
| `bootloader/` | Placeholder for future bootloader work |

## Current Kernel Features

- Cooperative scheduler with priority-aware ready queues and high-priority
  preemption points, wait queues, and optional SMP-oriented per-CPU storage.
- Interactive shell using an IRQ producer and shell consumer UART RX path.
- Ionix-generated configuration for platform constants, scheduler tick rate,
  IRQ table size, FDT auto-probing, built-in shell selection, RISC-V mode,
  optional RISC-V ISA extension toggles, optional Sv32/Sv39 MMU setup, SMP
  limits, and network backend feature selection.
- RAMFS-based VFS with stable `NodeId` handles.
- Mount table snapshots to avoid printing or block I/O while holding locks.
- Read-only exFAT mounting over VirtIO block on `riscv-generic`.
- Driver registry with snapshot APIs, class registration, factory-based probing,
  and FDT auto-probing.
- Fixed-capacity structures on core paths for MCU compatibility.
- RISC-V S-mode boot on RustSBI/OpenSBI-style firmware by default.

## Driver Framework

Drivers implement `kernel::driver::Driver` and optionally a device-class trait:

- `kernel::driver::char::CharDevice`
- `kernel::driver::terminal::TerminalDevice`
- `kernel::driver::gpio::GpioController`
- `kernel::driver::block::BlockDevice`
- `kernel::driver::net::NetDevice`

FDT-probed drivers implement `DriverFactory`. Probe inputs are represented by
`DeviceResource` rather than raw `base_addr, irq` pairs. Static probed driver
instances should use `StaticDriverPool` instead of open-coded
`UnsafeCell<MaybeUninit<T>>` pools.

The driver manager provides snapshots for iteration and IRQ dispatch so callers
do not print, call back into drivers, or perform block I/O while holding the
registry lock.

## MCU And STM32 Notes

MCU-specific peripherals are not placed in `drivers/` unless the implementation
is reusable across boards. Pinmux, clocks, reset policy, DMA, vendor HAL
selection, and C HAL adapters belong in `bsp/`, `platform/`, or the user
project.

The current STM32 path is `stm32f103-bluepill`. It is a buildable hardware
bring-up target using `thumbv7m-none-eabi`, a Cortex-M startup file, a small
linker script, and `bsp::arm::stm32f103` for clock setup, USART1 console,
SysTick, IRQ wiring, and C HAL bridge functions. See
`docs/mcu-hal-integration.md` for the intended STM32Cube HAL integration model.

## RISC-V Notes

`riscv-generic` defaults to `s-mode`. The boot HART policy is intentionally
hart0-first so firmware setups that choose a different boot HART do not start
multiple kernel instances. The platform receives `hartid` and `dtb_pa` from
firmware; if no DTB address is provided, it falls back to the DTB address
configured in `.config.toml`.

RISC-V CSR access, SBI calls, trap setup, and timer interrupt enables live under
`arch/src/riscv`. QEMU virt MMIO details such as PLIC and CLINT addresses remain
under `platform/riscv-generic`.

The schema also exposes the RISC-V compressed-instruction toggle, optional
SMP, and optional early Sv32/Sv39 identity maps. CLIC support is kept under the
RISC-V architecture/platform boundary; board-specific device policy stays in
`platform/riscv-generic`.

## Hypervisor Status

The RISC-V hypervisor code under `arch/src/riscv/hypervisor` is experimental.
It is not yet the primary boot path. Keep the stable kernel, shell, VFS, driver,
and scheduler paths working before expanding hypervisor functionality.

## License

MIT License. See `LICENSE`.
