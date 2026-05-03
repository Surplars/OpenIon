use cortex_m::interrupt::InterruptNumber;
use cortex_m::peripheral::NVIC;

#[derive(Clone, Copy)]
pub struct Irq(pub u16);
unsafe impl InterruptNumber for Irq {
    fn number(self) -> u16 {
        self.0
    }
}

pub fn enable_irq(irqn: u16) {
    unsafe { NVIC::unmask(Irq(irqn)) }
}

pub fn disable_irq(irqn: u16) {
    NVIC::mask(Irq(irqn))
}

pub fn set_priority(irqn: u16, priority: u8) {
    unsafe {
        // cortex_m 0.7 does not expose static set_priority usually, so we write directly to the register
        // NVIC->IP[n] register. Priority is in bits [7:0] depending on grouping.
        let mut nvic = cortex_m::Peripherals::steal().NVIC;
        // set_priority takes &mut self, irq, priority
        nvic.set_priority(Irq(irqn), priority);
    }
}

pub fn clear_pending(irqn: u16) {
    NVIC::unpend(Irq(irqn))
}
