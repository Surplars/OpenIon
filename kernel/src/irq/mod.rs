use crate::sync::Mutex;
use core::sync::atomic::{AtomicU32, Ordering};

type IrqHandler = fn();
pub const MAX_EXTERNAL_IRQS: usize = 64;

struct IrqState {
    count: usize,
    table: [Option<IrqHandler>; MAX_EXTERNAL_IRQS],
}

impl IrqState {
    const fn new() -> Self {
        Self {
            count: 0,
            table: [None; MAX_EXTERNAL_IRQS],
        }
    }
}

#[derive(Clone, Copy)]
pub struct IrqStats {
    pub configured_count: usize,
    pub registered_handlers: usize,
    pub handled: u32,
    pub unhandled: u32,
    pub out_of_range: u32,
    pub last_irq: u32,
    pub last_unhandled_irq: u32,
}

static IRQ_STATE: Mutex<IrqState> = Mutex::new(IrqState::new());
static HANDLED_IRQS: AtomicU32 = AtomicU32::new(0);
static UNHANDLED_IRQS: AtomicU32 = AtomicU32::new(0);
static OUT_OF_RANGE_IRQS: AtomicU32 = AtomicU32::new(0);
static LAST_IRQ: AtomicU32 = AtomicU32::new(u32::MAX);
static LAST_UNHANDLED_IRQ: AtomicU32 = AtomicU32::new(u32::MAX);

pub fn init(count: usize) {
    let mut state = IRQ_STATE.lock();
    state.count = count.min(MAX_EXTERNAL_IRQS);
    for slot in state.table.iter_mut() {
        *slot = None;
    }
    HANDLED_IRQS.store(0, Ordering::Release);
    UNHANDLED_IRQS.store(0, Ordering::Release);
    OUT_OF_RANGE_IRQS.store(0, Ordering::Release);
    LAST_IRQ.store(u32::MAX, Ordering::Release);
    LAST_UNHANDLED_IRQ.store(u32::MAX, Ordering::Release);
}

pub fn add_irq_handler(irqn: usize, handler: IrqHandler) -> bool {
    let mut state = IRQ_STATE.lock();
    if irqn < state.count {
        state.table[irqn] = Some(handler);
        true
    } else {
        false
    }
}

pub fn remove_irq_handler(irqn: usize) -> bool {
    let mut state = IRQ_STATE.lock();
    if irqn < state.count {
        state.table[irqn] = None;
        true
    } else {
        false
    }
}

pub fn handle_irq(irqn: usize) -> bool {
    LAST_IRQ.store(irqn as u32, Ordering::Release);

    let handler = {
        let state = IRQ_STATE.lock();
        if irqn >= state.count {
            OUT_OF_RANGE_IRQS.fetch_add(1, Ordering::AcqRel);
            LAST_UNHANDLED_IRQ.store(irqn as u32, Ordering::Release);
            return false;
        }
        state.table[irqn]
    };

    let handled = if let Some(handler) = handler {
        handler();
        true
    } else {
        crate::driver::manager::DriverManager::dispatch_irq(irqn as u32)
    };

    if handled {
        HANDLED_IRQS.fetch_add(1, Ordering::AcqRel);
    } else {
        UNHANDLED_IRQS.fetch_add(1, Ordering::AcqRel);
        LAST_UNHANDLED_IRQ.store(irqn as u32, Ordering::Release);
    }

    #[cfg(target_arch = "arm")]
    if crate::sched::Scheduler::schedule_if_preempt_pending() {
        unsafe {
            if let Some(yield_fn) = crate::arch::YIELD_CPU_FN {
                yield_fn();
            }
        }
    }

    handled
}

pub fn stats() -> IrqStats {
    let state = IRQ_STATE.lock();
    let mut registered_handlers = 0usize;
    for handler in state.table.iter().take(state.count) {
        if handler.is_some() {
            registered_handlers += 1;
        }
    }

    IrqStats {
        configured_count: state.count,
        registered_handlers,
        handled: HANDLED_IRQS.load(Ordering::Acquire),
        unhandled: UNHANDLED_IRQS.load(Ordering::Acquire),
        out_of_range: OUT_OF_RANGE_IRQS.load(Ordering::Acquire),
        last_irq: LAST_IRQ.load(Ordering::Acquire),
        last_unhandled_irq: LAST_UNHANDLED_IRQ.load(Ordering::Acquire),
    }
}
