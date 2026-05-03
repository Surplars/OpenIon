#![no_std]
#![no_main]

// use core::arch::global_asm;

// global_asm!(include_str!(""));

#[unsafe(no_mangle)]
pub extern "C" fn bootloader_entry() {
    loop {}
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
