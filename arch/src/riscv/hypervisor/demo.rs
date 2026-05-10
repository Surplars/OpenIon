#[derive(Clone, Copy)]
pub struct DemoGuest {
    pub entry: usize,
    pub bytes: &'static [u8],
}

// Minimal little-endian RISC-V guest:
//   addi a7, zero, 1      ; legacy SBI console putchar
//   addi a0, zero, 'H'
//   ecall
const DEMO_GUEST: [u8; 12] = [
    0x93, 0x08, 0x10, 0x00, // addi a7, zero, 1
    0x13, 0x05, 0x80, 0x04, // addi a0, zero, 'H'
    0x73, 0x00, 0x00, 0x00, // ecall
];

pub fn demo_guest() -> DemoGuest {
    DemoGuest {
        entry: 0,
        bytes: &DEMO_GUEST,
    }
}
