pub fn init(system_freq_hz: u32, tick_freq_hz: u32) {
    let reload_val = (system_freq_hz / tick_freq_hz) - 1;

    unsafe {
        let mut syst = cortex_m::Peripherals::steal().SYST;
        syst.set_clock_source(cortex_m::peripheral::syst::SystClkSource::Core);
        syst.set_reload(reload_val);
        syst.clear_current();
        syst.enable_counter();
        syst.enable_interrupt();
    }
}
