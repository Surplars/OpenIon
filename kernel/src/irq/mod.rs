use crate::sync::Mutex;

type IrqHandler = fn();
pub const MAX_EXTERNAL_IRQS: usize = 64;

struct IrqState {
    count: usize,
    table: [IrqHandler; MAX_EXTERNAL_IRQS],
}

impl IrqState {
    const fn new() -> Self {
        Self {
            count: 0,
            table: [default_handler; MAX_EXTERNAL_IRQS],
        }
    }
}

static IRQ_STATE: Mutex<IrqState> = Mutex::new(IrqState::new());

pub fn init(count: usize) {
    assert!(count <= MAX_EXTERNAL_IRQS);
    let mut state = IRQ_STATE.lock();
    state.count = count;
}

pub fn add_irq_handler(irqn: usize, handler: IrqHandler) {
    let mut state = IRQ_STATE.lock();
    if irqn < state.count {
        state.table[irqn] = handler;
    } else {
        panic!("invalid irq number: {}", irqn);
    }
}

pub fn handle_irq(irqn: usize) {
    let handler = {
        let state = IRQ_STATE.lock();
        if irqn < state.count {
            state.table[irqn]
        } else {
            panic!("unknown irq: {}", irqn);
        }
    };
    handler();
}

fn default_handler() {
     panic!("unhandled irq");
}
