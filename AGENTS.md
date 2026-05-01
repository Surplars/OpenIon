# AGENTS.md

## Project Overview

Bare-metal RTOS ("OpenIon") in Rust. `#![no_std]`, `#![no_main]`, no host tests. Runs on QEMU-emulated targets only. Long-term goal: RISC-V Type-1 hypervisor.

## Build & Run

```bash
make build PLAT=qemu-virt-riscv    # build RISC-V 64 platform
make build PLAT=qemu-an521         # build ARM Cortex-M platform
make run   PLAT=qemu-virt-riscv    # build + launch in QEMU (blocks terminal)
make run   PLAT=qemu-an521
```

- **Never use `make run` in agent sessions** — QEMU blocks the terminal and cannot be interrupted. Always use `make build`.
- `default-members` in root `Cargo.toml` is `["app", "arch", "kernel"]` — platform crates must be built explicitly via `-p` or `make`.
- Active default target is set in `.cargo/config.toml` (comment/uncomment `target = ...`). The Makefile overrides this with `--target`.
- Requires **Rust nightly** (`rust-toolchain.toml`).
- No `cargo test` — all crates set `test = false`. No CI workflows exist.

## Directory Layout

| Directory | Role |
|---|---|
| `kernel/` | Core kernel — scheduler, IRQ, memory, VFS framework, net stack framework, driver framework, logging, version. **Fully architecture-agnostic.** |
| `arch/` | Architecture-specific code. `riscv/` and `arm/cortex_m/` selected by `#[cfg(target_arch)]`. Implements kernel arch traits. |
| `drivers/` | Device drivers. May be generic (e.g. `ns16550a`) or platform-specific (e.g. `cmsdk_uart`). Each driver is a separate crate. |
| `platform/` | Board/SoC binaries. Each contains `startup.s`, `main.rs`, linker script, and platform-specific peripherals (PLIC, timer, etc.). |
| `app/` | User tasks (shell, test tasks). Contains `root_task()` — the first task the kernel spawns. |
| `bootloader/` | Placeholder, not yet used. |

## Workspace Crates

| Crate | Path |
|---|---|
| `kernel` | `kernel/` |
| `arch` | `arch/` |
| `app` | `app/` |
| `qemu-virt-riscv` | `platform/qemu-virt-riscv/` — RISC-V 64. Entry: `rust_main()` |
| `an521` | `platform/qemu-an521/` — ARM Cortex-M33. Entry: `platform_init()` |
| `ns16550a` | `drivers/ns16550a/` — NS16550A UART driver (used by qemu-virt-riscv) |
| `cmsdk_uart` | `drivers/cmsdk_uart/` — CMSDK UART driver (used by an521) |
| `lan9118` | `drivers/lan9118/` — LAN9118 Ethernet driver (used by an521) |
| `virtio_blk` | `drivers/virtio_blk/` — VirtIO block device driver (used by qemu-virt-riscv) |
| `bootloader` | `bootloader/` — placeholder |

## Boot Flow

1. Platform `startup.s` sets up stack, calls Rust entry.
2. Entry calls `kernel::boot::<Platform, Arch>(root_task)`.
3. Kernel boot: **version banner** → `arch::init` → `Platform::early_init` (UART, PLIC/NVIC, console, timer, IRQ handler registration) → core init (timer, IRQ table, scheduler) → driver registration via `DriverManager` → net init → spawn `root_task` → `Arch::start_first_task()`.

## Feature Flags: s-mode vs m-mode (RISC-V)

The `arch` and `qemu-virt-riscv` crates have `s-mode` (default) and `m-mode` features that control RISC-V privilege level:

- **s-mode**: Kernel runs in Supervisor mode on top of OpenSBI/RustSBI. Uses `sie::set_sext()` for IRQs, SBI timer, `sstatus::set_sie()`. Entry point is `rust_main(hartid, dtb_pa)`.
- **m-mode**: Kernel runs in Machine mode directly on hardware. Uses `mie::set_mext()` for IRQs, CLINT timer, `mstatus::set_mie()`.

The default is `s-mode`. To build for m-mode pass `--features m-mode --no-default-features`. The linker script base address (`0x80200000`) is set for OpenSBI jump target; m-mode would need a different linker script.

## Interrupt Architecture

### RISC-V (qemu-virt-riscv)
- External interrupts go through **PLIC** (Platform-Level Interrupt Controller).
- Trap handler (`arch/src/riscv/trap.rs`) detects `SupervisorExternal` / `MachineExternal` cause and calls `EXTERNAL_IRQ_HANDLER` function pointer.
- Platform sets `EXTERNAL_IRQ_HANDLER` in `early_init` to do PLIC claim → `DriverManager::dispatch_irq` → PLIC complete.
- CPU external interrupt enable: `sie::set_sext()` (S-mode) or `mie::set_mext()` (M-mode).
- QEMU Virt NS16550A UART is at `0x1000_0000`, IRQ 10.
- DTB address is hardcoded at `0x8006_8000` in `main.rs` (set via `kernel::platform::set_dtb_addr()`).

### ARM (qemu-an521)
- External interrupts go through **NVIC**.
- Vector table in `startup.s` maps IRQ numbers to handler symbols (e.g. `uart0_rx_handler` at IRQ 0 position).
- Handlers call `kernel::irq::handle_irq(N)` which dispatches via registered IRQ table.
- AN521 UART RX = IRQ 0, LAN9118 Ethernet = IRQ 48.

## Key Kernel Statics

- `kernel::arch::EXTERNAL_IRQ_HANDLER` — function pointer set by each platform in `early_init()`. The RISC-V trap handler calls this to dispatch external interrupts. ARM platforms do not use it (they call `kernel::irq::handle_irq(N)` directly from NVIC handlers).
- `kernel::arch::DISABLE_IRQ_FN` / `ENABLE_IRQ_FN` — set by `arch::init::<A>()`, used by `kernel::arch::disable_irq()` / `enable_irq()` with nesting counter.

## Driver Conventions

- Each driver lives in its own crate under `drivers/`.
- Drivers implement `kernel::driver::Driver` + optionally `kernel::driver::char::CharDevice` or `kernel::driver::net::NetDevice`.
- Platform `main.rs` creates `static` driver instances and registers them via `Platform::drivers()`.
- Console output is set up in `Platform::early_init()` via `kernel::log::set_console()`.
- The `EXTERNAL_IRQ_HANDLER` pointer (RISC-V only) is set in `Platform::early_init()` and must call `DriverManager::dispatch_irq(irq)`.
- Driver registration flow: `DriverManager::register_driver()` → `drv.auto_init()` (runs polymorphic native init).

## Memory Allocator

The kernel uses `rlsf` (Rust Linked-list-based Size-class Free-list) v0.2.2 for heap allocation and `heapless` v0.9.2 for fixed-capacity collections. Both are `no_std`-compatible.

## Networking

- Kernel supports two backends via features: `use_smoltcp` and `use_ionnet` (default).
- Net tasks in `app` are currently commented out.

## Conventions

- Comments and log messages mix Chinese and English.
- Log macros: `kinfo!`, `kdebug!`, `kerror!`, `kwarn!`, `kpln!`, `kp!` — defined in `kernel::log`.
- Version: `kernel::version` module, banner printed as first line of `boot()`.
- Edition 2024 across all crates.
- VFS is initialized early in boot (after driver init, before net init). File system operations go through `kernel::fs`.
