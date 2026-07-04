# AGENTS.md

## Project Overview

OpenIon is a bare-metal RTOS in Rust. It uses `#![no_std]` and `#![no_main]`,
has no host test suite, currently runs on QEMU-emulated RISC-V/ARM targets,
and has early STM32F103 hardware bring-up support.

The long-term goal is a RISC-V Type-1 hypervisor. The current stable baseline is
the RTOS kernel: scheduler, shell, VFS, VirtIO block I/O, driver framework,
memory management, and clean module boundaries.

## Build And Run

```bash
make config
make menuconfig
make build PLAT=riscv-generic
make build PLAT=qemu-an521
make build PLAT=stm32f103-bluepill
make run   PLAT=riscv-generic
make run   PLAT=qemu-an521
```

- Never use `make run` in agent sessions. QEMU blocks the terminal and may not
  be interruptible from the tool session. Use `make build` only.
- `make` delegates to `xtask`, and `xtask` is launched with `HOST_TARGET`
  because `.cargo/config.toml` defaults to a bare-metal target. The default is
  `x86_64-pc-windows-msvc`; override `HOST_TARGET=...` on other hosts.
- The root `Cargo.toml` default members are `["app", "arch", "kernel"]`.
  Platform crates must be built explicitly with `make build PLAT=...` or
  `cargo build -p <platform-crate> --target <target>`.
- Requires Rust nightly through `rust-toolchain.toml`.
- No `cargo test`: crates set `test = false`, and there is no CI test harness.
- Do not treat warnings in unrelated experimental code as part of a requested
  fix unless the user asks for warning cleanup.

## Workspace Crates

| Crate | Path | Role |
|---|---|---|
| `kernel` | `kernel/` | Architecture-neutral kernel core |
| `arch` | `arch/` | ISA/CPU-specific code |
| `app` | `app/` | Root task and shell |
| `riscv-generic` | `platform/riscv-generic/` | Generic RISC-V platform binary |
| `an521` | `platform/qemu-an521/` | ARM Cortex-M33 platform binary |
| `stm32f103-bluepill` | `platform/stm32f103-bluepill/` | STM32F103 Blue Pill hardware platform binary |
| `bsp` | `bsp/` | Board support and MCU HAL bridge glue |
| `ns16550a` | `drivers/ns16550a/` | NS16550A UART driver |
| `cmsdk_uart` | `drivers/cmsdk_uart/` | CMSDK UART driver |
| `lan9118` | `drivers/lan9118/` | LAN9118 Ethernet driver |
| `virtio_blk` | `drivers/virtio_blk/` | VirtIO MMIO block driver |
| `bootloader` | `bootloader/` | Placeholder |
| `xtask` | `xtask/` | Host-side config/build orchestration |

## Configuration

OpenIon uses Ionix for checked-in kernel and platform configuration.

| Path | Role |
|---|---|
| `utils/ionix/` | Rust configuration tool with CLI and API entry points |
| `config/openion.schema.toml` | Typed schema for OpenIon config |
| `.config.toml` | Active generated/edited config created from schema defaults |
| `.config.old.toml` | Backup written before `.config.toml` changes |
| `kernel/src/generated_config.rs` | Generated `no_std` Rust constants |
| `xtask/` | Generates config and maps config values to Cargo build arguments |

- Run `make config` or
  `cargo run -p xtask --release --target <host> -- --host-target <host> config`
  after changing schema/config files. Run `make menuconfig` for the interactive
  Ionix UI.
- Build through `make build PLAT=...` when possible. It regenerates config and
  keeps target triples, platform packages, RISC-V mode features, network
  backend features, and the built-in shell feature in sync.
- Do not hand-edit `kernel/src/generated_config.rs`; edit
  `config/openion.schema.toml` or `.config.toml` and regenerate.
- Keep generated constants `no_std` compatible. Do not make core kernel,
  shell, VFS, or driver-registry paths depend on host-only Ionix code.

## Module Boundaries

Keep these boundaries strict:

| Layer | Allowed responsibilities |
|---|---|
| `kernel/` | Platform-neutral and architecture-neutral kernel logic, traits, registries, scheduler, VFS, memory, logging, networking abstractions |
| `arch/` | ISA and CPU details: assembly, CSR access, trap vectors, context switching, SBI helpers, ARM NVIC helpers |
| `platform/` | Board/SoC binaries: startup assembly, linker scripts, platform memory and interrupt wiring, board driver instances |
| `bsp/` | Board support and MCU HAL bridge glue: clocks, pinmux, vendor HAL adapters, MCU-specific console/IRQ setup |
| `drivers/` | Reusable device protocol implementation behind kernel driver traits |
| `app/` | Root task and user-facing kernel apps such as the shell |

Do not put RISC-V CSR access or inline assembly in `kernel/` or platform code
when an `arch/src/riscv` helper is appropriate. Do not put board MMIO policy
into `arch/`; RISC-V platform discovery belongs in `platform/riscv-generic`.

## Boot Flow

1. Platform startup assembly sets the initial stack and calls the Rust entry.
2. Platform entry clears BSS, stores platform boot data such as `hartid` and
   DTB address, sets an early trap vector if needed, then calls
   `kernel::boot::<Platform, Arch>()`.
3. `kernel::boot` initializes architecture hooks, runs `Platform::early_init`,
   prints the version banner, writes `PlatformConfig`, initializes memory and
   core kernel services, auto-probes FDT drivers, registers platform drivers,
   initializes VFS and device files, initializes networking, installs the kernel
   root process, schedules the first task, and enters `Arch::start_first_task()`.

## RISC-V Mode Notes

`riscv-generic` defaults to `s-mode`, running on SBI firmware. The `arch` and
platform crates also expose `m-mode` features, but the default linker layout is
for the SBI jump target.

- S-mode uses supervisor CSRs, SBI timer setup, and `sstatus::set_sie()`.
- M-mode uses machine CSRs, CLINT timer compare, and `mstatus::set_mie()`.
- RISC-V CSR and SBI access should stay under `arch/src/riscv`.
- RISC-V PLIC, CLINT, timer, and memory discovery should be DTB-driven under
  `platform/riscv-generic`.

## Interrupt Architecture

### RISC-V `riscv-generic`

- External interrupts go through the platform PLIC.
- `arch/src/riscv/trap.rs` detects timer, external, and yield traps.
- The platform sets `kernel::arch::EXTERNAL_IRQ_HANDLER` during `early_init`.
- The external IRQ handler claims PLIC, dispatches through
  `DriverManager::dispatch_irq`, drains UART RX when needed, then completes PLIC.
- Timer interrupts call `kernel::platform::schedule_next_timer_tick()` so the
  platform decides the next board-specific deadline.

### ARM `qemu-an521`

- External interrupts go through NVIC.
- Startup assembly vector entries call Rust handlers.
- Handlers call `kernel::irq::handle_irq(N)`, which dispatches the registered
  IRQ table entry.

### STM32F103 `stm32f103-bluepill`

- Targets real STM32F103 Blue Pill-style hardware with target triple
  `thumbv7m-none-eabi`; there is no QEMU runner for this platform.
- Uses the `kernel/mcu_profile` Cargo feature to shrink fixed-capacity kernel
  storage and disables heavyweight optional paths through `xtask` constraints.
- `platform/stm32f103-bluepill` owns startup assembly, linker policy, and the
  platform trait implementation. `bsp::arm::stm32f103` owns clocks, USART1
  console setup, SysTick, NVIC wiring, and C HAL bridge exports.
- Do not add STM32 family register wrappers under `drivers/`. Keep MCU-specific
  HAL glue in `bsp/`, `platform/`, or the user project; reusable protocols may
  live under `drivers/`.

## Driver Framework Rules

- Every driver lives in its own crate under `drivers/`.
- Drivers implement `kernel::driver::Driver`.
- Character, terminal, GPIO, block, and network drivers additionally implement
  the matching device class trait.
- FDT-probed drivers implement `DriverFactory`.
- `DriverFactory::probe()` accepts `DeviceResource`; do not add new probe APIs
  that pass raw `base_addr, irq` pairs.
- Static probed driver instances should use `StaticDriverPool`.
- Use `DriverManager::drivers_snapshot()` or `for_each_driver()` before calling
  back into drivers. Do not hold the registry lock while printing, doing block
  I/O, or invoking arbitrary callbacks.

## FDT Policy

`kernel/src/fdt.rs` is a minimal generic FDT parser. It should not contain
driver-specific scanners such as "find all VirtIO devices". Device matching
belongs in driver factories and `DriverManager::auto_probe_fdt()`.

Do not reintroduce per-node boot spam such as `FDT node: compatible=...`.
Boot logs should only show concise DTB/probe summaries unless the user asks for
temporary diagnostics.

## Shell, VFS, And Block I/O Baseline

The RISC-V interactive path is expected to stay responsive. These commands must
return to the prompt:

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
```

Important current behavior:

- UART RX uses an IRQ producer and shell consumer path.
- Shell idle uses `core::hint::spin_loop()`; do not replace it with scheduler
  delay unless input wakeups are redesigned.
- VFS exposes stable `NodeId` handles instead of shell-visible raw vnode
  pointers.
- RAMFS owns built-in paths such as `/dev`.
- Mount table iteration uses snapshots.
- Empty directories should print nothing, not `(empty)`.
- Mount listing format is `<device> on <mount-point> type <fs>`.
- VirtIO block reads are internally serialized and should return errors instead
  of spinning forever.

## Scheduler Baseline

The scheduler has priority-aware ready queues, wait queues, high-priority
preemption points, and optional SMP-oriented per-CPU storage. Be careful around
idle behavior:

- The idle task should spin, not repeatedly yield into a trap storm.
- Shell no-input polling should spin, not sleep on a timer-dependent delay.
- Timer and external IRQ paths may request preemption, but should keep IRQ work
  short and avoid console output while holding locks.

## Memory

The kernel uses fixed-capacity structures on core paths for MCU compatibility.
`heapless` is available for fixed-capacity collections, and `rlsf` is used for
heap allocation where appropriate. Do not make shared kernel, shell, VFS, or
driver-registry paths depend on RISC-V-only heap behavior.

## Networking

The kernel supports `use_ionnet` by default and has optional `use_smoltcp`
support. Network tasks in `app` may be disabled or experimental. Do not treat
the network stack as a stable acceptance path unless the user asks for it.

## Hypervisor Status

RISC-V hypervisor code under `arch/src/riscv/hypervisor` is experimental and is
not the main boot path. Prefer preserving the stable RTOS path before expanding
hypervisor functionality. Keep hypervisor ISA details under `arch`, and keep
board/device emulation policy out of `kernel`.

## Coding Conventions

- Prefer existing local patterns over introducing new abstractions.
- Keep edits scoped to the requested area.
- Use `rg` for search.
- Use `apply_patch` for manual edits.
- Do not revert unrelated dirty worktree changes.
- Avoid long lock hold times. Do not print, probe devices, or do block I/O while
  holding global registry or mount locks.
- Keep core code `no_std` compatible.
- Prefer ASCII in new docs and comments unless the surrounding file already uses
  non-ASCII intentionally.

## Verification

For a broad build check:

```bash
make build PLAT=riscv-generic
make build PLAT=qemu-an521
make build PLAT=stm32f103-bluepill
```

Do not run QEMU from an agent session. If runtime validation is needed, provide
the manual shell smoke test commands for the user to run locally.
