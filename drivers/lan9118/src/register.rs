use volatile_register::{RO, RW, WO};

#[repr(C)]
pub(super) struct Lan9118Registers {
    pub rx_data_fifo: RO<u32>,   // 0x00
    _reserved0: [u32; 7],        // 0x04 - 0x1C
    pub tx_data_fifo: WO<u32>,   // 0x20
    _reserved1: [u32; 7],        // 0x24 - 0x3C
    pub rx_status_fifo: RO<u32>, // 0x40
    pub rx_status_peek: RO<u32>, // 0x44
    pub tx_status_fifo: RO<u32>, // 0x48
    pub tx_status_peek: RO<u32>, // 0x4C
    pub id_rev: RO<u32>,         // 0x50
    pub irq_cfg: RW<u32>,        // 0x54
    pub int_sts: RW<u32>,        // 0x58
    pub int_en: RW<u32>,         // 0x5C
    _reserved2: [u32; 1],        // 0x60
    pub byte_test: RO<u32>,      // 0x64
    pub fifo_int: RW<u32>,       // 0x68
    pub rx_cfg: RW<u32>,         // 0x6C
    pub tx_cfg: RW<u32>,         // 0x70
    pub hw_cfg: RW<u32>,         // 0x74
    pub rx_dp_ctrl: RW<u32>,     // 0x78
    pub rx_fifo_inf: RO<u32>,    // 0x7C
    pub tx_fifo_inf: RO<u32>,    // 0x80
    pub pmt_ctrl: RW<u32>,       // 0x84
    pub gpio_cfg: RW<u32>,       // 0x88
    pub gpt_cfg: RW<u32>,        // 0x8C
    pub gpt_cnt: RO<u32>,        // 0x90
    _reserved3: [u32; 1],        // 0x94
    pub word_swap: RW<u32>,      // 0x98
    pub free_run_cnt: RO<u32>,   // 0x9C
    pub rx_drop: RO<u32>,        // 0xA0
    pub mac_csr_cmd: RW<u32>,    // 0xA4
    pub mac_csr_data: RW<u32>,   // 0xA8
    pub afc_cfg: RW<u32>,        // 0xAC
    pub e2p_cmd: RW<u32>,        // 0xB0
    pub e2p_data: RW<u32>,       // 0xB4
}
