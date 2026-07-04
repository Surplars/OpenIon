//! Kernel init service manager.
//!
//! Root process startup is fixed-capacity and allocation-free so it remains
//! suitable for MCU/MPU targets and early boot paths.

pub const INIT_SERVICE_CAP: usize = 8;

pub type InitResult = Result<(), InitError>;
pub type InitStartFn = fn() -> InitResult;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitError {
    Disabled,
    NoSlots,
    StartFailed,
}

#[derive(Clone, Copy)]
pub struct InitService {
    pub name: &'static str,
    pub required: bool,
    pub start: InitStartFn,
}

impl InitService {
    pub const fn new(name: &'static str, required: bool, start: InitStartFn) -> Self {
        Self {
            name,
            required,
            start,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct InitStats {
    pub registered: usize,
    pub started: usize,
    pub skipped: usize,
    pub failed: usize,
}

pub struct InitManager {
    services: [Option<InitService>; INIT_SERVICE_CAP],
    len: usize,
}

impl InitManager {
    pub const fn new() -> Self {
        Self {
            services: [None; INIT_SERVICE_CAP],
            len: 0,
        }
    }

    pub fn register(&mut self, service: InitService) -> InitResult {
        if self.len >= self.services.len() {
            return Err(InitError::NoSlots);
        }

        self.services[self.len] = Some(service);
        self.len += 1;
        Ok(())
    }

    pub fn run(&self) -> InitStats {
        let mut stats = InitStats {
            registered: self.len,
            started: 0,
            skipped: 0,
            failed: 0,
        };

        for slot in self.services.iter().take(self.len) {
            let Some(service) = slot else {
                continue;
            };

            match (service.start)() {
                Ok(()) => {
                    stats.started += 1;
                    crate::kinfo!("init: started {}", service.name);
                }
                Err(InitError::Disabled) => {
                    stats.skipped += 1;
                    crate::kdebug!("init: skipped disabled {}", service.name);
                }
                Err(err) => {
                    stats.failed += 1;
                    if service.required {
                        crate::kerror!("init: required {} failed: {:?}", service.name, err);
                    } else {
                        crate::kwarn!("init: optional {} failed: {:?}", service.name, err);
                    }
                }
            }
        }

        stats
    }
}
