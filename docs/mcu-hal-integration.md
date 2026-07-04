# MCU HAL integration

OpenIon keeps `drivers/` for reusable device protocols and platform-neutral driver implementations. MCU-specific peripheral ownership belongs in a BSP, platform crate, or user crate because pinmux, clocks, reset domains, DMA channels, and vendor HAL policy are board-specific.

For STM32F103 and similar MCU projects, use this split:

- OpenIon owns startup, kernel scheduling, IRQ dispatch, SysTick, and kernel-facing traits.
- The platform or BSP owns clock setup, memory layout, vector entries, and early console setup.
- A user BSP or HAL adapter owns STM32Cube HAL, `stm32f1xx-hal`, or direct PAC integration.
- HAL adapters register with the kernel through stable APIs instead of adding MCU-only crates under `drivers/`.

The current STM32F103 Blue Pill path follows that model. `platform/stm32f103-bluepill` provides startup and linker policy. `bsp::arm::stm32f103` initializes USART1, installs the early console, registers the USART1 IRQ handler, and exposes a small C ABI bridge for C HAL code.

Kernel-facing registration points:

```rust
kernel::irq::add_irq_handler(irq, handler);
kernel::driver::manager::DriverManager::register_driver(driver);
kernel::driver::char::set_rx_poll_fn(poll_fn);
kernel::driver::char::push_to_rx_buf(byte);
```

C HAL adapters can call the exported bridge functions from the STM32F103 BSP:

```c
uint32_t openion_stm32f103_clock_hz(void);
uintptr_t openion_stm32f103_usart1_base(void);
uint32_t openion_stm32f103_usart1_irq(void);
uint32_t openion_kernel_tick_ms(void);
bool openion_uart_rx_push(uint8_t byte);
```

Do not add a new `drivers/stm32...` crate only to wrap one MCU family's registers. Add reusable protocol drivers under `drivers/` only when the implementation is meaningful across boards or discoverable platform instances. Board-specific HAL glue should stay with the BSP or user project.

## STM32F103 project integration

The in-tree STM32F103 support is intentionally a thin kernel-facing layer:

- `platform/stm32f103-bluepill/` provides the Rust platform binary, startup
  assembly, linker script, and `Platform` implementation.
- `bsp/src/arm/stm32f103.rs` owns board clocks, USART1 console setup, SysTick,
  IRQ hookup, and the exported C ABI bridge.
- A CubeMX project should keep generated HAL code in the user project. Add a
  small C adapter that calls the OpenIon bridge functions instead of copying
  HAL drivers into OpenIon `drivers/`.

Build the kernel-side target with:

```bash
make build PLAT=stm32f103-bluepill
```

Use the resulting ELF or binary according to the board flashing flow. Runtime
sensor, display, storage, and network HAL code should be linked from the user
project or a board-specific BSP adapter, then registered through kernel APIs
when it needs to appear as an OpenIon device.