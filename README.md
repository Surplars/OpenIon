# OpenIon

A bare-metal real-time operating system written in Rust, targeting QEMU-emulated RISC-V and ARM platforms. Long-term goal: RISC-V Type-1 hypervisor.

## Supported Platforms

| Platform | Architecture | QEMU Machine |
|---|---|---|
| `qemu-virt-riscv` | RISC-V 64 (rv64imac) | `qemu-system-riscv64 -machine virt` |
| `qemu-an521` | ARM Cortex-M33 | `qemu-system-arm -M mps2-an521` |

## Prerequisites

- [Rust nightly](https://rustup.rs/) (see `rust-toolchain.toml`)
- QEMU (`qemu-system-riscv64` and/or `qemu-system-arm`)

## Build & Run

```bash
# RISC-V 64
make build PLAT=qemu-virt-riscv
make run   PLAT=qemu-virt-riscv

# ARM Cortex-M33
make build PLAT=qemu-an521
make run   PLAT=qemu-an521
```

Press `Ctrl-A X` to exit QEMU.

## Project Structure

```
├── kernel/          Architecture-independent kernel core
│   └── src/         Scheduler, IRQ, memory, drivers, VFS, net, logging
├── arch/            Architecture-specific code
│   └── src/
│       ├── riscv/   RISC-V trap, context switch, IRQ
│       └── arm/     ARM Cortex-M context switch, NVIC, SysTick
├── drivers/         Device drivers (each is a separate crate)
│   ├── ns16550a/    NS16550A UART (generic, used by RISC-V)
│   ├── cmsdk_uart/  CMSDK UART (used by ARM AN521)
│   └── lan9118/     LAN9118 Ethernet (used by ARM AN521)
├── platform/        Board/SoC binaries
│   ├── qemu-virt-riscv/   RISC-V platform (PLIC, CLINT/SBI timer)
│   └── qemu-an521/        ARM platform (NVIC, SysTick)
├── app/             User tasks (shell, test tasks)
└── bootloader/      Placeholder
```

## Kernel Architecture

The kernel is fully architecture-agnostic. Platform and architecture specifics are injected through trait implementations:

- `kernel::platform::Platform` — implemented by each platform binary
- `kernel::arch::Arch` — implemented by each architecture module
- `kernel::driver::Driver` — implemented by each device driver

Boot sequence: version banner → arch init → platform early_init → core init (timer, IRQ, scheduler) → driver registration → net init → spawn root task → start first task.

## License

TBD
