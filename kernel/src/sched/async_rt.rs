extern crate alloc;

use alloc::boxed::Box;
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use crate::sync::Mutex;

type TaskFuture = Pin<Box<dyn Future<Output = ()> + Send>>;

const MAX_ASYNC_TASKS: usize = crate::generated_config::OPENION_ASYNC_TASK_SLOTS;

static EXECUTOR: Mutex<Executor> = Mutex::new(Executor::new());
static HEARTBEAT_TICKS: AtomicU32 = AtomicU32::new(0);
static DEMO_EVENT: super::Event<1> = super::Event::new();
static DEMO_EVENTS: AtomicU32 = AtomicU32::new(0);
static RX_BYTES: AtomicU32 = AtomicU32::new(0);
static RX_EVENTS: AtomicU32 = AtomicU32::new(0);
static LAST_RX_BYTE: AtomicU32 = AtomicU32::new(0);
static CURRENT_POLL_TASK: AtomicUsize = AtomicUsize::new(NO_TASK);

const NO_TASK: usize = usize::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnError {
    Disabled,
    NoSlots,
    InvalidConfig,
}

#[derive(Clone, Copy, Debug)]
pub struct AsyncStats {
    pub enabled: bool,
    pub slots: usize,
    pub active: usize,
    pub spawned: u64,
    pub completed: u64,
    pub polls: u64,
    pub wakes: u64,
    pub sleeping: usize,
    pub heartbeat_ticks: u32,
    pub demo_events: u32,
    pub rx_bytes: u32,
    pub rx_events: u32,
    pub last_rx_byte: u8,
}

struct AsyncSlot {
    future: Option<TaskFuture>,
    name: &'static str,
    ready: bool,
}

impl AsyncSlot {
    const fn empty() -> Self {
        Self {
            future: None,
            name: "",
            ready: false,
        }
    }
}

#[derive(Clone, Copy)]
struct SleepSlot {
    task_idx: usize,
    wake_tick: u32,
    active: bool,
}

impl SleepSlot {
    const fn empty() -> Self {
        Self {
            task_idx: NO_TASK,
            wake_tick: 0,
            active: false,
        }
    }
}

struct Executor {
    slots: [AsyncSlot; MAX_ASYNC_TASKS],
    sleep_slots: [SleepSlot; MAX_ASYNC_TASKS],
    spawned: u64,
    completed: u64,
    polls: u64,
    wakes: u64,
}

impl Executor {
    const fn new() -> Self {
        Self {
            slots: [const { AsyncSlot::empty() }; MAX_ASYNC_TASKS],
            sleep_slots: [const { SleepSlot::empty() }; MAX_ASYNC_TASKS],
            spawned: 0,
            completed: 0,
            polls: 0,
            wakes: 0,
        }
    }

    fn active_count(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| slot.future.is_some())
            .count()
    }

    fn sleeping_count(&self) -> usize {
        self.sleep_slots.iter().filter(|slot| slot.active).count()
    }

    fn mark_ready(&mut self, idx: usize) {
        if let Some(slot) = self.slots.get_mut(idx) {
            if slot.future.is_some() {
                slot.ready = true;
                self.wakes = self.wakes.wrapping_add(1);
            }
        }
    }

    fn register_sleep(&mut self, task_idx: usize, wake_tick: u32) {
        for slot in self.sleep_slots.iter_mut() {
            if slot.active && slot.task_idx == task_idx {
                slot.wake_tick = wake_tick;
                return;
            }
        }

        for slot in self.sleep_slots.iter_mut() {
            if !slot.active {
                *slot = SleepSlot {
                    task_idx,
                    wake_tick,
                    active: true,
                };
                return;
            }
        }

        self.mark_ready(task_idx);
    }

    fn clear_sleep(&mut self, task_idx: usize) {
        for slot in self.sleep_slots.iter_mut() {
            if slot.active && slot.task_idx == task_idx {
                *slot = SleepSlot::empty();
                return;
            }
        }
    }
}

unsafe impl Send for Executor {}
unsafe impl Sync for Executor {}

pub fn spawn<F>(name: &'static str, future: F) -> Result<usize, SpawnError>
where
    F: Future<Output = ()> + Send + 'static,
{
    if !crate::generated_config::OPENION_ASYNC_RT {
        return Err(SpawnError::Disabled);
    }
    if MAX_ASYNC_TASKS == 0 {
        return Err(SpawnError::InvalidConfig);
    }

    let mut task: Option<TaskFuture> = Some(Box::pin(future));
    let mut exec = EXECUTOR.lock();
    for (idx, slot) in exec.slots.iter_mut().enumerate() {
        if slot.future.is_none() {
            slot.future = task.take();
            slot.name = name;
            slot.ready = true;
            exec.spawned = exec.spawned.wrapping_add(1);
            return Ok(idx);
        }
    }

    drop(exec);
    drop(task);
    Err(SpawnError::NoSlots)
}

pub fn current_poll_task() -> usize {
    CURRENT_POLL_TASK.load(Ordering::Acquire)
}
pub fn executor_main() -> ! {
    loop {
        if crate::generated_config::OPENION_ASYNC_RT {
            let _ = poll_once();
        }
        super::Scheduler::delay(1);
    }
}

pub fn poll_once() -> usize {
    let mut polled = 0usize;

    for idx in 0..MAX_ASYNC_TASKS {
        let future = {
            let mut exec = EXECUTOR.lock();
            if !exec.slots.get(idx).map(|slot| slot.ready).unwrap_or(false) {
                continue;
            }
            exec.slots[idx].ready = false;
            exec.slots[idx].future.take()
        };

        let Some(mut future) = future else {
            continue;
        };

        let waker = task_waker(idx);
        let mut cx = Context::from_waker(&waker);
        CURRENT_POLL_TASK.store(idx, Ordering::Release);
        let poll_result = future.as_mut().poll(&mut cx);
        CURRENT_POLL_TASK.store(NO_TASK, Ordering::Release);
        polled += 1;

        let mut exec = EXECUTOR.lock();
        exec.polls = exec.polls.wrapping_add(1);
        match poll_result {
            Poll::Ready(()) => {
                exec.slots[idx].name = "";
                exec.slots[idx].ready = false;
                exec.clear_sleep(idx);
                exec.completed = exec.completed.wrapping_add(1);
            }
            Poll::Pending => {
                exec.slots[idx].future = Some(future);
            }
        }
    }

    polled
}

pub fn tick_update() {
    if !crate::generated_config::OPENION_ASYNC_RT {
        return;
    }

    let now = crate::timer::ticks();
    let mut exec = EXECUTOR.lock();
    for idx in 0..exec.sleep_slots.len() {
        let sleep = exec.sleep_slots[idx];
        if sleep.active && now.wrapping_sub(sleep.wake_tick) < (u32::MAX / 2) {
            exec.sleep_slots[idx] = SleepSlot::empty();
            exec.mark_ready(sleep.task_idx);
        }
    }
}

pub fn wake_task(idx: usize) {
    if idx < MAX_ASYNC_TASKS {
        EXECUTOR.lock().mark_ready(idx);
    }
}

pub fn stats() -> AsyncStats {
    let exec = EXECUTOR.lock();
    AsyncStats {
        enabled: crate::generated_config::OPENION_ASYNC_RT,
        slots: MAX_ASYNC_TASKS,
        active: exec.active_count(),
        spawned: exec.spawned,
        completed: exec.completed,
        polls: exec.polls,
        wakes: exec.wakes,
        sleeping: exec.sleeping_count(),
        heartbeat_ticks: HEARTBEAT_TICKS.load(Ordering::Acquire),
        demo_events: DEMO_EVENTS.load(Ordering::Acquire),
        rx_bytes: RX_BYTES.load(Ordering::Acquire),
        rx_events: RX_EVENTS.load(Ordering::Acquire),
        last_rx_byte: LAST_RX_BYTE.load(Ordering::Acquire) as u8,
    }
}

pub fn heartbeat_task() -> impl Future<Output = ()> + Send + 'static {
    async {
        loop {
            sleep_ticks(1000).await;
            HEARTBEAT_TICKS.fetch_add(1, Ordering::AcqRel);
        }
    }
}

pub fn demo_event_task() -> impl Future<Output = ()> + Send + 'static {
    async {
        loop {
            DEMO_EVENT.wait_async().await;
            DEMO_EVENTS.fetch_add(1, Ordering::AcqRel);
        }
    }
}

pub fn rx_counter_task() -> impl Future<Output = ()> + Send + 'static {
    async {
        loop {
            crate::driver::char::wait_rx_async().await;
            RX_EVENTS.fetch_add(1, Ordering::AcqRel);
        }
    }
}

pub fn note_rx_byte(byte: u8) {
    LAST_RX_BYTE.store(byte as u32, Ordering::Release);
    RX_BYTES.fetch_add(1, Ordering::AcqRel);
}

pub fn signal_demo_event() {
    DEMO_EVENT.signal();
}

pub fn sleep_ticks(ticks: u32) -> SleepTicks {
    SleepTicks {
        wake_tick: crate::timer::ticks().wrapping_add(ticks),
        registered: false,
    }
}

pub struct SleepTicks {
    wake_tick: u32,
    registered: bool,
}

impl Future for SleepTicks {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        let now = crate::timer::ticks();
        if now.wrapping_sub(self.wake_tick) < (u32::MAX / 2) {
            Poll::Ready(())
        } else {
            if !self.registered {
                let task_idx = CURRENT_POLL_TASK.load(Ordering::Acquire);
                if task_idx != NO_TASK {
                    EXECUTOR.lock().register_sleep(task_idx, self.wake_tick);
                    self.registered = true;
                }
            }
            Poll::Pending
        }
    }
}

fn task_waker(idx: usize) -> Waker {
    unsafe { Waker::from_raw(task_raw_waker(idx)) }
}

fn task_raw_waker(idx: usize) -> RawWaker {
    RawWaker::new((idx + 1) as *const (), &TASK_WAKER_VTABLE)
}

static TASK_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
    task_waker_clone,
    task_waker_wake,
    task_waker_wake_by_ref,
    task_waker_drop,
);

unsafe fn task_waker_clone(data: *const ()) -> RawWaker {
    RawWaker::new(data, &TASK_WAKER_VTABLE)
}

unsafe fn task_waker_wake(data: *const ()) {
    unsafe {
        task_waker_wake_by_ref(data);
    }
}

unsafe fn task_waker_wake_by_ref(data: *const ()) {
    let encoded = data as usize;
    if encoded == 0 {
        return;
    }
    let idx = encoded - 1;
    wake_task(idx);
}

unsafe fn task_waker_drop(_data: *const ()) {}
