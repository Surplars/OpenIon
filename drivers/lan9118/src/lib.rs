#![no_std]

pub mod register;

use kernel::driver::net::{MacAddress, NetDevice};
use kernel::driver::{DeviceConfig, Driver, DriverErr, DriverFactory, DriverResult, GenericDeviceConfig};
use kernel::driver::manager::AnyDriver;
use register::Lan9118Registers;

pub struct Lan9118 {
    config: GenericDeviceConfig,
}

impl Lan9118 {
    pub const fn new(config: GenericDeviceConfig) -> Self {
        Self { config }
    }

    fn registers(&self) -> &Lan9118Registers {
        unsafe { &*(self.config.base_address() as *const Lan9118Registers) }
    }

    fn mac_csr_wait(&self) {
        let regs = self.registers();
        while regs.mac_csr_cmd.read() & (1 << 31) != 0 {}
    }

    fn mac_write(&self, reg: u8, val: u32) {
        let regs = self.registers();
        self.mac_csr_wait();
        unsafe { regs.mac_csr_data.write(val) };
        unsafe { regs.mac_csr_cmd.write((1 << 31) | reg as u32) };
        self.mac_csr_wait();
    }

    fn mac_read(&self, reg: u8) -> u32 {
        let regs = self.registers();
        self.mac_csr_wait();
        unsafe { regs.mac_csr_cmd.write((1 << 31) | (1 << 30) | reg as u32) };
        self.mac_csr_wait();
        regs.mac_csr_data.read()
    }
}

impl Driver for Lan9118 {
    type Config = GenericDeviceConfig;
    type Error = kernel::driver::DriverErr;

    fn get_config(&self) -> Self::Config {
        self.config
    }

    fn name(&self) -> &'static str {
        "LAN9118 Ethernet Controller"
    }

    fn init(&self) -> DriverResult<()> {
        let regs = self.registers();

        let id_rev = regs.id_rev.read();
        if (id_rev >> 16) != 0x0118 {
            return Err(kernel::driver::DriverErr::InitFailed);
        }

        // Soft reset
        unsafe {
            regs.hw_cfg.write(0x00000001); // SRST
        }
        while regs.hw_cfg.read() & 0x00000001 != 0 {}

        // TX FIFO size to 5 (5KB TX, 11KB RX)
        unsafe { regs.hw_cfg.write(0x0005_0000) };

        // Wait for EEPROM logic to be ready
        while regs.byte_test.read() != 0x87654321 {}
        while regs.hw_cfg.read() & (1 << 27) != 0 {} // Wait for EEPROM load to finish

        // Enable MAC TX/RX
        let mac_cr = self.mac_read(1); // 1 = MAC_CR
        self.mac_write(1, mac_cr | (1 << 3) | (1 << 2)); // TXEN and RXEN

        // Enable TX/RX datapath
        unsafe {
            regs.tx_cfg.write(1 << 1); // TX_ON
            regs.rx_cfg.write(0); // 0 offset (no alignment padding for now)
        }

        Ok(())
    }

    fn handle_irq(&self, _irq_id: u32) -> bool {
        // Clear interrupts if we handle them
        false
    }
}

impl NetDevice for Lan9118 {
    fn mac_address(&self) -> MacAddress {
        let high = self.mac_read(2); // ADDRH
        let low = self.mac_read(3); // ADDRL
        [
            (low & 0xFF) as u8,
            ((low >> 8) & 0xFF) as u8,
            ((low >> 16) & 0xFF) as u8,
            ((low >> 24) & 0xFF) as u8,
            (high & 0xFF) as u8,
            ((high >> 8) & 0xFF) as u8,
        ]
    }

    fn has_rx_data(&self) -> bool {
        let regs = self.registers();
        let rx_inf = regs.rx_fifo_inf.read();
        let rx_used = (rx_inf >> 16) & 0xFF;
        rx_used > 0
    }

    fn transmit(&self, buf: &[u8]) -> DriverResult<()> {
        let regs = self.registers();
        let len = buf.len();

        let words = (len + 3) / 4;
        let required_space = (words + 2) as u32 * 4; // Data + 2 command words (in bytes)

        // Wait for enough space in TX FIFO
        while (regs.tx_fifo_inf.read() & 0xFFFF) < required_space {}

        // Command A: First segment(13) | Last segment(12) | buffer len
        let cmd_a = (1 << 13) | (1 << 12) | (len as u32 & 0x7FF);
        // Command B: packet len
        let cmd_b = len as u32 & 0x7FF;

        unsafe {
            regs.tx_data_fifo.write(cmd_a);
            regs.tx_data_fifo.write(cmd_b);

            let mut chunks = buf.chunks_exact(4);
            for chunk in &mut chunks {
                let val = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                regs.tx_data_fifo.write(val);
            }

            let rem = chunks.remainder();
            if !rem.is_empty() {
                let mut last = [0u8; 4];
                last[..rem.len()].copy_from_slice(rem);
                let val = u32::from_le_bytes(last);
                regs.tx_data_fifo.write(val);
            }
        }

        Ok(())
    }

    fn receive(&self, buf: &mut [u8]) -> DriverResult<usize> {
        let regs = self.registers();
        let rx_inf = regs.rx_fifo_inf.read();
        let rx_used = (rx_inf >> 16) & 0xFF;

        if rx_used == 0 {
            return Ok(0);
        }

        let status = regs.rx_status_fifo.read();
        let pkt_len = ((status >> 16) & 0x3FFF) as usize;
        let error = (status & (1 << 15)) != 0;

        // Calculate dwords to read. The length includes trailing padding bytes for alignment?
        // Actually length is just length. Dwords = (length + 3) / 4.
        // The hardware padding makes it a multiple of 4 bytes.
        let pad_len = (pkt_len + 3) & !3;
        let words = pad_len / 4;

        let mut read_len = 0;

        for _ in 0..words {
            let val = regs.rx_data_fifo.read();
            if error {
                continue; // Drain FIFO if error
            }
            let bytes = val.to_le_bytes();
            for i in 0..4 {
                if read_len < pkt_len && read_len < buf.len() {
                    buf[read_len] = bytes[i];
                }
                read_len += 1;
            }
        }

        if error {
            return Err(kernel::driver::DriverErr::HardwareFault);
        }

        // Return actual copied size (bounded by buf.len)
        Ok(core::cmp::min(pkt_len, buf.len()))
    }
}

/// FDT-compatible factory for LAN9118 Ethernet controller.
/// Matches compatible = "smsc,lan9118" and creates a driver instance.
/// Also supports manual registration on MCU platforms without FDT.
pub struct Lan9118Factory;

use core::cell::UnsafeCell;
use core::mem::MaybeUninit;

struct EthSlot(UnsafeCell<MaybeUninit<Lan9118>>);
unsafe impl Sync for EthSlot {}

const MAX_LAN9118: usize = 2;
static ETH_POOL: [EthSlot; MAX_LAN9118] = [
    EthSlot(UnsafeCell::new(MaybeUninit::uninit())),
    EthSlot(UnsafeCell::new(MaybeUninit::uninit())),
];
static ETH_POOL_IDX: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

impl DriverFactory for Lan9118Factory {
    fn compatible(&self) -> &[&str] {
        &["smsc,lan9118"]
    }

    fn probe(&self, base_addr: usize, irq: u32) -> Option<&'static dyn AnyDriver> {
        let idx = ETH_POOL_IDX.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        if idx >= MAX_LAN9118 {
            return None;
        }
        let slot = &ETH_POOL[idx];
        let driver = Lan9118::new(GenericDeviceConfig::new(base_addr, irq));
        unsafe {
            (*slot.0.get()).write(driver);
            Some(&*(*slot.0.get()).as_ptr())
        }
    }
}
