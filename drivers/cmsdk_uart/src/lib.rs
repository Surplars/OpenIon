#![no_std]

use bitflags::bitflags;
use kernel::driver::char::CharDevice;
use kernel::driver::{GenericDeviceConfig, Driver, DriverErr, DriverFactory, DriverResult, DeviceConfig};
use kernel::driver::manager::AnyDriver;
use kernel::kinfo;
use volatile_register::RW;

#[repr(C)]
struct UartRegisters {
    data: RW<u32>,
    state: RW<u32>,
    ctrl: RW<u32>,
    intstatus: RW<u32>,
    bauddiv: RW<u32>,
}

bitflags! {
    struct UartStatus: u32 {
        const TX_BF = 1 << 0;
        const RX_BF = 1 << 1;
        const TX_OR = 1 << 2;
        const RX_OR = 1 << 3;
    }
}

bitflags! {
    struct UartCtrl: u32 {
        const TXEN = 1 << 0;
        const RXEN = 1 << 1;
        const TXIEN = 1 << 2;
        const RXIEN = 1 << 3;
        const TXOEN = 1 << 4;
        const RXOEN = 1 << 5;
        const HSTM = 1 << 6;
    }
}

pub struct CmsdkUart {
    base_addr: usize,
    irq_num: u32,
}

impl CmsdkUart {
    pub const fn new(base_addr: usize, irq_num: u32) -> Self {
        Self {
            base_addr,
            irq_num,
        }
    }
}

impl Driver for CmsdkUart {
    type Config = GenericDeviceConfig;
    type Error = DriverErr;

    fn get_config(&self) -> Self::Config {
        GenericDeviceConfig::new(self.base_addr, self.irq_num)
    }

    fn name(&self) -> &'static str {
        "CMSDK UART"
    }

    fn init(&self) -> DriverResult<()> {
        let uart_regs = self.get_config().base_address() as *const UartRegisters;

        unsafe {
            (*uart_regs).ctrl.write(
                UartCtrl::TXEN.bits() | UartCtrl::RXEN.bits() | UartCtrl::RXIEN.bits()
            );
        }

        kinfo!("CMSDK UART initialized with RX interrupt enabled");
        kernel::irq::add_irq_handler(self.irq_num as usize, || {
            kernel::driver::manager::DriverManager::dispatch_irq(0);
        });

        DriverResult::Ok(())
    }

    fn handle_irq(&self, irq_id: u32) -> bool {
        if irq_id == self.irq_num {
            let regs = self.base_addr as *const UartRegisters;
            
            // Read all available bytes
            while UartStatus::from_bits_truncate(unsafe { (*regs).state.read() })
                .contains(UartStatus::RX_BF) 
            {
                let byte = unsafe { (*regs).data.read() } as u8;
                // Add to global rx buffer
                kernel::driver::char::push_to_rx_buf(byte);
            }
            
            unsafe {
                let rx_mask = 1 << 1; // RX interrupt clear mask (RXIQ)
                (*regs).intstatus.write(rx_mask);
            }
            true
        } else {
            false
        }
    }
}

impl CharDevice for CmsdkUart {
    fn write_byte(&self, byte: u8) -> DriverResult<()> {
        let regs = self.base_addr as *const UartRegisters;

        while UartStatus::from_bits_truncate(unsafe { (*regs).state.read() })
            .contains(UartStatus::TX_BF)
        {}

        unsafe {
            (*(self.base_addr as *mut UartRegisters))
                .data
                .write(byte as u32);
        }

        DriverResult::Ok(())
    }

    fn read_byte(&self) -> DriverResult<u8> {
        let regs = self.base_addr as *const UartRegisters;

        if !UartStatus::from_bits_truncate(unsafe { (*regs).state.read() })
            .contains(UartStatus::RX_BF)
        {
            return DriverResult::Err(DriverErr::Busy);
        }

        let byte = unsafe { (*regs).data.read() } as u8;

        DriverResult::Ok(byte)
    }
}

/// FDT-compatible factory for CMSDK UART.
/// Matches compatible = "arm,cmsdk-uart" and creates a driver instance.
/// Also supports manual registration on MCU platforms without FDT.
pub struct CmsdkUartFactory;

use core::cell::UnsafeCell;
use core::mem::MaybeUninit;

struct UartSlot(UnsafeCell<MaybeUninit<CmsdkUart>>);
unsafe impl Sync for UartSlot {}

const MAX_CMSDK_UART: usize = 4;
static UART_POOL: [UartSlot; MAX_CMSDK_UART] = [
    UartSlot(UnsafeCell::new(MaybeUninit::uninit())),
    UartSlot(UnsafeCell::new(MaybeUninit::uninit())),
    UartSlot(UnsafeCell::new(MaybeUninit::uninit())),
    UartSlot(UnsafeCell::new(MaybeUninit::uninit())),
];
static UART_POOL_IDX: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

impl DriverFactory for CmsdkUartFactory {
    fn compatible(&self) -> &[&str] {
        &["arm,cmsdk-uart"]
    }

    fn probe(&self, base_addr: usize, irq: u32) -> Option<&'static dyn AnyDriver> {
        let idx = UART_POOL_IDX.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        if idx >= MAX_CMSDK_UART {
            return None;
        }
        let slot = &UART_POOL[idx];
        let driver = CmsdkUart::new(base_addr, irq);
        unsafe {
            (*slot.0.get()).write(driver);
            Some(&*(*slot.0.get()).as_ptr())
        }
    }
}
