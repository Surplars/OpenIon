use kernel::driver::manager::AnyDriver;
use kernel::log::FunctionConsole;

pub const CPU_FREQ_HZ: u32 = 72_000_000;
pub const APB2_FREQ_HZ: u32 = 72_000_000;
pub const MEMORY_BASE: usize = 0x2000_0000;
pub const MEMORY_SIZE: usize = 20 * 1024;

const RCC_BASE: usize = 0x4002_1000;
const FLASH_BASE: usize = 0x4002_2000;
const GPIOA_BASE: usize = 0x4001_0800;
const USART1_BASE: usize = 0x4001_3800;

const FLASH_ACR: usize = FLASH_BASE;
const RCC_CR: usize = RCC_BASE;
const RCC_CFGR: usize = RCC_BASE + 0x04;
const RCC_APB2ENR: usize = RCC_BASE + 0x18;
const GPIOA_CRH: usize = GPIOA_BASE + 0x04;
const USART1_SR: usize = USART1_BASE;
const USART1_DR: usize = USART1_BASE + 0x04;
const USART1_BRR: usize = USART1_BASE + 0x08;
const USART1_CR1: usize = USART1_BASE + 0x0c;

const RCC_CR_HSION: u32 = 1 << 0;
const RCC_CR_HSIRDY: u32 = 1 << 1;
const RCC_CR_HSEON: u32 = 1 << 16;
const RCC_CR_HSERDY: u32 = 1 << 17;
const RCC_CR_PLLON: u32 = 1 << 24;
const RCC_CR_PLLRDY: u32 = 1 << 25;

const RCC_APB2ENR_AFIOEN: u32 = 1 << 0;
const RCC_APB2ENR_IOPAEN: u32 = 1 << 2;
const RCC_APB2ENR_USART1EN: u32 = 1 << 14;

const USART_SR_RXNE: u32 = 1 << 5;
const USART_SR_TXE: u32 = 1 << 7;
const USART_CR1_RE: u32 = 1 << 2;
const USART_CR1_TE: u32 = 1 << 3;
const USART_CR1_RXNEIE: u32 = 1 << 5;
const USART_CR1_UE: u32 = 1 << 13;

pub const USART1_IRQ: u32 = 37;
const USART_BAUD: u32 = 115_200;

static USART1_CONSOLE: FunctionConsole = FunctionConsole {
    putc_fn: usart1_putc,
};
static PLATFORM_DRIVERS: [&'static dyn AnyDriver; 0] = [];

pub fn early_clock_init() {
    write_reg(RCC_CR, read_reg(RCC_CR) | RCC_CR_HSION);
    while (read_reg(RCC_CR) & RCC_CR_HSIRDY) == 0 {}

    write_reg(FLASH_ACR, 0x12);
    write_reg(RCC_CR, read_reg(RCC_CR) | RCC_CR_HSEON);

    let mut hse_ready = false;
    for _ in 0..100_000 {
        if (read_reg(RCC_CR) & RCC_CR_HSERDY) != 0 {
            hse_ready = true;
            break;
        }
    }

    if hse_ready {
        write_reg(RCC_CFGR, (0b100 << 8) | (1 << 16) | (0b0111 << 18));
        write_reg(RCC_CR, read_reg(RCC_CR) | RCC_CR_PLLON);
        while (read_reg(RCC_CR) & RCC_CR_PLLRDY) == 0 {}
        write_reg(RCC_CFGR, read_reg(RCC_CFGR) | 0b10);
        while ((read_reg(RCC_CFGR) >> 2) & 0b11) != 0b10 {}
    }
}

pub fn init_console() {
    enable_usart1_pins();
    init_usart1_regs();
    kernel::driver::char::set_rx_poll_fn(usart1_read_byte);
    crate::install_console(&USART1_CONSOLE);
}

pub fn drivers() -> &'static [&'static dyn AnyDriver] {
    &PLATFORM_DRIVERS
}

#[cfg(target_arch = "arm")]
pub fn init_irqs() {
    kernel::irq::add_irq_handler(USART1_IRQ as usize, usart1_irq_handler);
    arch::arm::cortex_m::nvic::enable_irq(USART1_IRQ as u16);
}

#[cfg(not(target_arch = "arm"))]
pub fn init_irqs() {}

#[cfg(target_arch = "arm")]
pub fn init_timer() {
    let config = kernel::platform::get_config();
    arch::arm::cortex_m::systick::init(config.cpu_freq_hz, config.systick_hz);
}

#[cfg(not(target_arch = "arm"))]
pub fn init_timer() {}

pub fn usart1_irq() -> u32 {
    USART1_IRQ
}

fn usart1_irq_handler() {
    while let Some(byte) = usart1_read_byte() {
        kernel::driver::char::push_to_rx_buf(byte);
    }
}

fn enable_usart1_pins() {
    write_reg(
        RCC_APB2ENR,
        read_reg(RCC_APB2ENR) | RCC_APB2ENR_AFIOEN | RCC_APB2ENR_IOPAEN | RCC_APB2ENR_USART1EN,
    );

    let mut crh = read_reg(GPIOA_CRH);
    crh &= !((0b1111 << 4) | (0b1111 << 8));
    crh |= (0b1011 << 4) | (0b0100 << 8);
    write_reg(GPIOA_CRH, crh);
}

fn init_usart1_regs() {
    write_reg(USART1_CR1, 0);
    write_reg(USART1_BRR, (APB2_FREQ_HZ + (USART_BAUD / 2)) / USART_BAUD);
    write_reg(
        USART1_CR1,
        USART_CR1_UE | USART_CR1_TE | USART_CR1_RE | USART_CR1_RXNEIE,
    );
}

fn usart1_putc(ch: u8) {
    while (read_reg(USART1_SR) & USART_SR_TXE) == 0 {}
    write_reg(USART1_DR, ch as u32);
}

fn usart1_read_byte() -> Option<u8> {
    if (read_reg(USART1_SR) & USART_SR_RXNE) == 0 {
        return None;
    }
    Some(read_reg(USART1_DR) as u8)
}

#[inline(always)]
fn read_reg(addr: usize) -> u32 {
    unsafe { core::ptr::read_volatile(addr as *const u32) }
}

#[inline(always)]
fn write_reg(addr: usize, val: u32) {
    unsafe { core::ptr::write_volatile(addr as *mut u32, val) }
}

pub mod hal_bridge {
    use super::*;

    #[unsafe(no_mangle)]
    pub extern "C" fn openion_stm32f103_clock_hz() -> u32 {
        CPU_FREQ_HZ
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn openion_stm32f103_usart1_base() -> usize {
        USART1_BASE
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn openion_stm32f103_usart1_irq() -> u32 {
        USART1_IRQ
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn openion_kernel_tick_ms() -> u32 {
        kernel::timer::ticks()
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn openion_uart_rx_push(byte: u8) -> bool {
        kernel::driver::char::push_to_rx_buf(byte);
        true
    }
}
