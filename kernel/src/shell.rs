//! Built-in kernel shell.
//!
//! This is a minimal shell that runs when no init process is found.
//! Can be disabled with `cfg(feature = "builtin_shell")` to use an external shell from VFS.

use crate::{kp, kpln};

#[cfg(feature = "mcu_profile")]
pub const BUILTIN_SHELL_STACK_SIZE: usize = 8 * 1024;
#[cfg(not(feature = "mcu_profile"))]
pub const BUILTIN_SHELL_STACK_SIZE: usize = crate::generated_config::OPENION_SHELL_STACK_SIZE;
const LS_BUF_CAP: usize = 32;

struct ShellState {
    cwd: [u8; 256],
    cwd_len: usize,
    last_cmd: [u8; 128],
    last_cmd_len: usize,
}

impl ShellState {
    fn new() -> Self {
        let mut cwd = [0u8; 256];
        cwd[0] = b'/';
        Self {
            cwd,
            cwd_len: 1,
            last_cmd: [0u8; 128],
            last_cmd_len: 0,
        }
    }

    fn cwd_str(&self) -> &str {
        core::str::from_utf8(&self.cwd[..self.cwd_len]).unwrap_or("/")
    }

    fn remember_cmd(&mut self, cmd: &str) {
        let len = cmd.len().min(self.last_cmd.len());
        self.last_cmd_len = len;
        self.last_cmd[..len].copy_from_slice(&cmd.as_bytes()[..len]);
    }

    fn set_cwd(&mut self, path: &str) -> bool {
        if path.is_empty() || path.len() > self.cwd.len() {
            return false;
        }
        self.cwd_len = path.len();
        self.cwd[..self.cwd_len].copy_from_slice(path.as_bytes());
        true
    }
}

/// Escape sequence parser state
#[derive(Clone, Copy, PartialEq, Eq)]
enum EscState {
    Normal,
    SawEsc,
    SawBracket,
    Saw3, // delete key prefix
}

pub fn shell_main() -> ! {
    let mut state = ShellState::new();

    kpln!("===================================");
    kpln!(" OpenIon Shell v0.1.0 ");
    kpln!(" Type 'help' for commands ");
    kpln!("===================================");
    shell_prompt(&state);

    let mut input_buf = [0u8; 128];
    let mut cursor_pos: usize = 0; // insertion point
    let mut cursor_end: usize = 0; // end of actual text
    let mut esc_state = EscState::Normal;

    loop {
        let Some(c) = crate::driver::char::pop_from_rx_buf() else {
            if crate::driver::char::has_rx_poll_fn() {
                core::hint::spin_loop();
            } else {
                crate::arch::idle_hint();
            }
            continue;
        };

        match esc_state {
            EscState::SawBracket => {
                esc_state = EscState::Normal;
                match c {
                    b'A' => {
                        /* up: restore last command */
                        let last_len = state.last_cmd_len;
                        if last_len > 0 && last_len < input_buf.len() {
                            cursor_end = last_len;
                            cursor_pos = last_len;
                            for i in 0..last_len {
                                input_buf[i] = state.last_cmd[i];
                            }
                            redraw_input(&state, &input_buf, cursor_pos, cursor_end);
                        }
                    }
                    b'B' => { /* down: ignore */ }
                    b'C' => {
                        if cursor_pos < cursor_end {
                            cursor_pos += 1;
                            redraw_input(&state, &input_buf, cursor_pos, cursor_end);
                        }
                    }
                    b'D' => {
                        if cursor_pos > 0 {
                            cursor_pos -= 1;
                            redraw_input(&state, &input_buf, cursor_pos, cursor_end);
                        }
                    }
                    b'H' => {
                        cursor_pos = 0;
                        redraw_input(&state, &input_buf, cursor_pos, cursor_end);
                    }
                    b'F' => {
                        cursor_pos = cursor_end;
                        redraw_input(&state, &input_buf, cursor_pos, cursor_end);
                    }
                    b'3' => {
                        esc_state = EscState::Saw3;
                    }
                    _ => {}
                }
                continue;
            }
            EscState::Saw3 => {
                esc_state = EscState::Normal;
                if c == b'~' && cursor_pos < cursor_end {
                    // Delete character at cursor_pos: shift left
                    for i in cursor_pos..cursor_end - 1 {
                        input_buf[i] = input_buf[i + 1];
                    }
                    cursor_end -= 1;
                    redraw_input(&state, &input_buf, cursor_pos, cursor_end);
                }
                continue;
            }
            EscState::SawEsc => {
                esc_state = if c == b'[' {
                    EscState::SawBracket
                } else {
                    EscState::Normal
                };
                continue;
            }
            EscState::Normal => {}
        }

        if c == b'\x1b' {
            esc_state = EscState::SawEsc;
            continue;
        }

        if c == b'\r' || c == b'\n' {
            kpln!("");
            if cursor_end > 0 {
                input_buf[cursor_end] = 0;
                let cmd_str = core::str::from_utf8(&input_buf[..cursor_end]).unwrap_or("");
                state.remember_cmd(cmd_str);
                execute_command(&mut state, cmd_str.trim());
            }
            cursor_pos = 0;
            cursor_end = 0;
            shell_prompt(&state);
        } else if c == 8 || c == 127 {
            // Backspace: delete character before cursor, shift remaining left
            if cursor_pos > 0 {
                for i in cursor_pos..cursor_end {
                    input_buf[i - 1] = input_buf[i];
                }
                cursor_end -= 1;
                cursor_pos -= 1;
                redraw_input(&state, &input_buf, cursor_pos, cursor_end);
            }
        } else if c == 0x09 {
            // Tab key: auto-complete
            handle_tab_completion(&state, &mut input_buf, &mut cursor_pos, &mut cursor_end);
        } else if c >= 0x20 && c < 0x7f {
            // Printable ASCII: insert at cursor_pos
            if cursor_end < input_buf.len() {
                for i in (cursor_pos..cursor_end).rev() {
                    input_buf[i + 1] = input_buf[i];
                }
                input_buf[cursor_pos] = c;
                cursor_pos += 1;
                cursor_end += 1;
                redraw_input(&state, &input_buf, cursor_pos, cursor_end);
            }
        }
    }
}

fn shell_prompt(state: &ShellState) {
    kp!("{} $ ", state.cwd_str());
}

/// Redraw the input line: clear, print prompt + buffer, reposition cursor.
fn redraw_input(state: &ShellState, buf: &[u8], pos: usize, end: usize) {
    let cwd = state.cwd_str();
    kp!("\r\x1b[K{} $ ", cwd);
    for i in 0..end {
        kp!("{}", buf[i] as char);
    }
    if pos < end {
        kp!("\r{} $ ", cwd);
        for i in 0..pos {
            kp!("{}", buf[i] as char);
        }
    }
}

/// Handle Tab key for auto-completion
fn handle_tab_completion(
    state: &ShellState,
    buf: &mut [u8; 128],
    cursor_pos: &mut usize,
    cursor_end: &mut usize,
) {
    let input_len = *cursor_end;
    if input_len == 0 {
        return;
    }

    // Complete the argument containing the cursor, not just the end of line.
    let mut arg_start: Option<usize> = None;
    let mut i = 0;
    while i < *cursor_pos {
        if buf[i] == b' ' {
            arg_start = Some(i + 1);
        }
        i += 1;
    }

    if let Some(start) = arg_start {
        if start <= *cursor_pos {
            let prefix_len = *cursor_pos - start;
            complete_filename(state, buf, start, prefix_len, cursor_pos, cursor_end);
        }
    } else {
        // Completing command (no space)
        complete_command(state, buf, input_len, cursor_pos, cursor_end);
    }
    redraw_input(state, buf, *cursor_pos, *cursor_end);
}

/// Complete command name
fn complete_command(
    state: &ShellState,
    buf: &mut [u8; 128],
    input_len: usize,
    cursor_pos: &mut usize,
    cursor_end: &mut usize,
) {
    const COMMANDS: &[&str] = &[
        "help",
        "version",
        "ver",
        "clear",
        "uptime",
        "ls",
        "cd",
        "cat",
        "mkdir",
        "touch",
        "echo",
        "vm",
        "mount",
        "mem",
        "top",
        "cpuinfo",
        "smp",
        "tasks",
        "ps",
        "drivers",
        "term",
        "gpio",
        "irq",
        #[cfg(feature = "async_rt")]
        "async",
    ];

    let input = core::str::from_utf8(&buf[..input_len]).unwrap_or("");

    // Find first match and count matches
    let mut first_match: Option<&str> = None;
    let mut match_count = 0;

    for &cmd in COMMANDS {
        if cmd.starts_with(input) {
            match_count += 1;
            if first_match.is_none() {
                first_match = Some(cmd);
            }
        }
    }

    match match_count {
        0 => { /* no match */ }
        1 => {
            if let Some(cmd) = first_match {
                let len = cmd.len().min(127);
                buf[..len].copy_from_slice(cmd.as_bytes());
                *cursor_pos = len;
                *cursor_end = len;
                // Can't use buf[len] = 0 directly since buf is immutable reference
            }
        }
        _ => {
            kpln!("");
            for &cmd in COMMANDS {
                if cmd.starts_with(input) {
                    kp!("{} ", cmd);
                }
            }
            kpln!("");
            shell_prompt(state);
            for i in 0..*cursor_end {
                kp!("{}", buf[i] as char);
            }
        }
    }
}

/// Complete filename (path)
fn complete_filename(
    state: &ShellState,
    buf: &mut [u8; 128],
    prefix_start: usize,
    prefix_len: usize,
    cursor_pos: &mut usize,
    cursor_end: &mut usize,
) {
    let search_prefix =
        core::str::from_utf8(&buf[prefix_start..prefix_start + prefix_len]).unwrap_or("");
    let (dir_part, name_prefix, replace_start) = split_completion_path(search_prefix);
    let dir_path = match normalize_path(state, dir_part) {
        Some(path) => path,
        None => return,
    };
    let dir = dir_path.as_str();

    let mut found_name = [0u8; 64];
    let mut found_len = 0;
    let mut found_is_dir = false;
    let mut match_count = 0;

    let mut collect = |entry: &crate::fs::DirEntry| {
        let name = entry.name_str();
        if name.starts_with(name_prefix) {
            match_count += 1;
            if match_count == 1 {
                found_len = name.len().min(63);
                found_name[..found_len].copy_from_slice(name.as_bytes());
                found_is_dir = entry.file_type == crate::fs::FileType::Directory;
            }
        }
    };

    if let Ok(node) = crate::fs::resolve_path(dir) {
        let _ = crate::fs::list_dir(node, &mut collect);
        crate::fs::list_mount_children(dir, &mut collect);
    } else {
        let _ = crate::fs::list_path(dir, &mut collect);
    }

    match match_count {
        0 => { /* no match */ }
        1 => {
            let suffix = if found_is_dir { "/" } else { "" };
            let suffix_len = suffix.len();
            let total_len = found_len + suffix_len;
            let replace_pos = prefix_start + replace_start;
            let available = 128 - replace_pos;
            let copy_len = total_len.min(available);

            if copy_len > 0 {
                let name_slice = core::str::from_utf8(&found_name[..found_len]).unwrap_or("");
                for (i, c) in name_slice.bytes().take(copy_len).enumerate() {
                    buf[replace_pos + i] = c;
                }
                if found_is_dir && copy_len > found_len {
                    buf[replace_pos + found_len] = b'/';
                }
                *cursor_pos = replace_pos + copy_len;
                *cursor_end = replace_pos + copy_len;
            }
        }
        _ => {
            kpln!("");
            let _ = list_directory_entries(dir, &mut |entry| {
                let name = entry.name_str();
                if name.starts_with(name_prefix) {
                    if entry.file_type == crate::fs::FileType::Directory {
                        kp!("{}/ ", name);
                    } else {
                        kp!("{} ", name);
                    }
                }
            });
            kpln!("");
            shell_prompt(state);
            for i in 0..*cursor_end {
                kp!("{}", buf[i] as char);
            }
        }
    }
}

struct ShellPath {
    buf: [u8; 256],
    len: usize,
}

impl ShellPath {
    fn root() -> Self {
        let mut buf = [0u8; 256];
        buf[0] = b'/';
        Self { buf, len: 1 }
    }

    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("/")
    }

    fn push_component(&mut self, component: &str) -> bool {
        if component.is_empty() || component == "." {
            return true;
        }
        if component == ".." {
            self.pop_component();
            return true;
        }

        let extra_slash = usize::from(self.len > 1);
        if self.len + extra_slash + component.len() > self.buf.len() {
            return false;
        }
        if extra_slash == 1 {
            self.buf[self.len] = b'/';
            self.len += 1;
        }
        self.buf[self.len..self.len + component.len()].copy_from_slice(component.as_bytes());
        self.len += component.len();
        true
    }

    fn pop_component(&mut self) {
        while self.len > 1 && self.buf[self.len - 1] != b'/' {
            self.len -= 1;
        }
        if self.len > 1 {
            self.len -= 1;
        }
    }
}

fn split_completion_path(path: &str) -> (&str, &str, usize) {
    match path.rfind('/') {
        Some(pos) => {
            let dir_end = if pos == 0 { 1 } else { pos };
            (&path[..dir_end], &path[pos + 1..], pos + 1)
        }
        None => ("", path, 0),
    }
}

fn normalize_path(state: &ShellState, path: &str) -> Option<ShellPath> {
    let input = if path.is_empty() {
        state.cwd_str()
    } else {
        path
    };
    let mut out = ShellPath::root();

    if !input.starts_with('/') {
        for component in state.cwd_str().split('/') {
            if !out.push_component(component) {
                return None;
            }
        }
    }

    for component in input.split('/') {
        if !out.push_component(component) {
            return None;
        }
    }
    Some(out)
}

fn split_parent_child<'a>(state: &ShellState, path: &'a str) -> Option<(ShellPath, &'a str)> {
    let slash = path.rfind('/');
    let (parent_arg, child) = match slash {
        Some(0) => ("/", &path[1..]),
        Some(pos) => (&path[..pos], &path[pos + 1..]),
        None => ("", path),
    };
    if child.is_empty() || child.contains('/') {
        return None;
    }
    Some((normalize_path(state, parent_arg)?, child))
}

fn execute_command(state: &mut ShellState, input: &str) {
    if input.is_empty() {
        return;
    }

    let (cmd, args) = match input.find(' ') {
        Some(pos) => (&input[..pos], input[pos + 1..].trim()),
        None => (input, ""),
    };

    match cmd {
        "help" => cmd_help(),
        "version" | "ver" => cmd_version(),
        "clear" => kp!("\x1b[2J\x1b[H"),
        "uptime" => cmd_uptime(),
        "ls" => cmd_ls(state, args),
        "cd" => cmd_cd(state, args),
        "cat" => cmd_cat(state, args),
        "mkdir" => cmd_mkdir(state, args),
        "touch" => cmd_touch(state, args),
        "echo" => cmd_echo(state, args),
        "vm" => cmd_vm(args),
        "mount" => cmd_mount(args),
        "mem" => cmd_mem(),
        "cpuinfo" => cmd_cpuinfo(),
        "smp" => cmd_smp(),
        "tasks" | "ps" => cmd_tasks(),
        "top" => cmd_top(),
        "drivers" => cmd_drivers(),
        "term" => cmd_term(),
        "gpio" => cmd_gpio(),
        "irq" => cmd_irq(),
        #[cfg(feature = "async_rt")]
        "async" => cmd_async(args),
        _ => {
            kpln!("{}: command not found", cmd);
        }
    }
}

fn cmd_help() {
    kpln!("Built-in commands:");
    kpln!("  help              Show this help");
    kpln!("  version           Show version");
    kpln!("  clear             Clear screen");
    kpln!("  uptime            Show system uptime");
    kpln!("  mem               Show memory info");
    kpln!("  top               Show system status overview");
    kpln!("  cpuinfo           Show CPU and ISA info");
    kpln!("  smp               Show SMP bring-up status");
    kpln!("  tasks / ps        List running tasks");
    kpln!("  drivers           List registered drivers");
    kpln!("  term              List terminal devices");
    kpln!("  gpio              List GPIO controllers");
    kpln!("  irq               Show IRQ statistics");
    #[cfg(feature = "async_rt")]
    kpln!("  async             Show async executor stats");
    kpln!("  ls [path]         List directory");
    kpln!("  cd [path]         Change current directory");
    kpln!("  cat <file>        Read file");
    kpln!("  echo <text> > <f> Write text to file");
    kpln!("  mkdir <name>      Create directory");
    kpln!("  touch <name>      Create empty file");
    kpln!("  mount [dev target] List or mount block device");
    kpln!("  mount -u <target> Unmount filesystem");
    kpln!("  vm create <name>  Create a VM");
    kpln!("  vm load-demo <name> Prepare demo guest metadata");
    kpln!("  vm list           List VMs");
    kpln!("  vm run <name>     Run a VM");
}

fn cmd_version() {
    kpln!("{} v{}", crate::version::OS_NAME, crate::version::VERSION);
}

fn cmd_uptime() {
    let ticks = crate::timer::ticks();
    let secs = ticks / 1000;
    let ms = ticks % 1000;
    kpln!("Uptime: {}.{:03}s ({} ticks)", secs, ms, ticks);
}

#[cfg(feature = "async_rt")]
fn cmd_async(args: &str) {
    if args == "wake" {
        crate::sched::async_rt::signal_demo_event();
        kpln!("async: demo event signaled");
        return;
    }

    let stats = crate::sched::async_rt::stats();
    kpln!("Async runtime:");
    kpln!(
        "  enabled={}, slots={}, active={}, sleeping={}, spawned={}, completed={}",
        stats.enabled,
        stats.slots,
        stats.active,
        stats.sleeping,
        stats.spawned,
        stats.completed,
    );
    kpln!(
        "  polls={}, wakes={}, heartbeat={}, demo_events={}",
        stats.polls,
        stats.wakes,
        stats.heartbeat_ticks,
        stats.demo_events,
    );
    kpln!(
        "  rx_bytes={}, rx_events={}, last_rx=0x{:02x}",
        stats.rx_bytes,
        stats.rx_events,
        stats.last_rx_byte,
    );
    kpln!("  async wake        Signal demo async event");
}

fn cmd_smp() {
    let status = crate::platform::smp_status();
    kpln!("SMP:");
    kpln!("  enabled         : {}", status.enabled);
    kpln!("  possible_cpus   : {}", status.possible_cpus);
    kpln!("  online_cpus     : {}", status.online_cpus);
    kpln!("  active_cpus     : {}", status.active_cpus);
    kpln!("  parked_cpus     : {}", status.parked_cpus);
    kpln!("  online_mask     : {:#x}", status.online_mask);
    kpln!("  active_mask     : {:#x}", status.active_mask);
    kpln!("  parked_mask     : {:#x}", status.parked_mask);
    kpln!("  boot_cpu        : {}", status.boot_cpu);
    kpln!("  current_cpu     : {}", status.current_cpu);
    kpln!("  start_attempts  : {}", status.start_attempts);
    kpln!("  start_failures  : {}", status.start_failures);
}

fn cmd_mem() {
    let stats = crate::mm::stats();
    kpln!("Memory:");
    kpln!(
        "  heap: {}{}{} bytes used, {} bytes total, {} live allocs, {} failed",
        stats.heap_algorithm,
        if stats.heap.initialized {
            " "
        } else {
            " not initialized, "
        },
        stats.heap.used,
        stats.heap.size,
        stats.heap.allocations,
        stats.heap.failed_allocations,
    );
    kpln!(
        "  frames: {}{} base={}, free={}/{} pages ({} KiB free)",
        stats.frame_algorithm,
        if stats.frames.initialized {
            " "
        } else {
            " not initialized, "
        },
        stats.frames.base,
        stats.frames.free_pages,
        stats.frames.total_pages,
        stats.frames.free_pages * crate::mm::PAGE_SIZE / 1024,
    );
    kpln!("  object pools: {}", stats.object_pool_algorithm);
}

fn cmd_cpuinfo() {
    let cfg = crate::platform::get_config();
    let identity = collect_cpu_identity();

    kpln!("CPU:");
    kpln!("  vendor           : {}", identity.vendor_name());
    kpln!("  model            : {}", identity.model_name());
    match crate::log::cpu_id() {
        Some(id) => kpln!("  current cpu      : {}", id),
        None => kpln!("  current cpu      : unknown"),
    }
    let cpu_count = print_dtb_cpus();
    kpln!("  possible cpus    : {}", cpu_count);
    let smp = crate::platform::smp_status();
    kpln!("  online cpus      : {}", smp.online_cpus);
    kpln!("  active cpus      : {}", smp.active_cpus);
    kpln!("  parked cpus      : {}", smp.parked_cpus);
    kpln!("  online mask      : {:#x}", smp.online_mask);
    kpln!("  active mask      : {:#x}", smp.active_mask);
    kpln!("  parked mask      : {:#x}", smp.parked_mask);
    kpln!(
        "  arch             : {}",
        if crate::generated_config::OPENION_RISCV_XLEN_64 {
            "riscv64"
        } else if crate::generated_config::OPENION_RISCV_XLEN_32 {
            "riscv32"
        } else {
            "unknown"
        }
    );
    kpln!(
        "  privilege        : {}",
        if crate::generated_config::OPENION_RISCV_S_MODE {
            "S-mode"
        } else if crate::generated_config::OPENION_RISCV_M_MODE {
            "M-mode"
        } else {
            "unknown"
        }
    );
    kpln!(
        "  mmu              : {}",
        if crate::mm::translation_enabled() {
            "Sv32 identity active"
        } else if crate::generated_config::OPENION_RISCV_SV32_MMU {
            "Sv32 requested, inactive"
        } else {
            "disabled"
        }
    );
    kp!(
        "  isa              : rv{}",
        if crate::generated_config::OPENION_RISCV_XLEN_64 {
            64
        } else {
            32
        }
    );
    if crate::generated_config::OPENION_RISCV_EXT_M {
        kp!(" m");
    }
    if crate::generated_config::OPENION_RISCV_EXT_A {
        kp!(" a");
    }
    if crate::generated_config::OPENION_RISCV_EXT_C {
        kp!(" c");
    }
    if crate::generated_config::OPENION_RISCV_EXT_F {
        kp!(" f");
    }
    if crate::generated_config::OPENION_RISCV_EXT_D {
        kp!(" d");
    }
    if crate::generated_config::OPENION_RISCV_EXT_B {
        kp!(" b");
    }
    if crate::generated_config::OPENION_RISCV_EXT_V {
        kp!(" v");
    }
    if crate::generated_config::OPENION_RISCV_EXT_ZICSR {
        kp!(" zicsr");
    }
    if crate::generated_config::OPENION_RISCV_EXT_ZIFENCEI {
        kp!(" zifencei");
    }
    kpln!("");
    if let Some(isa) = identity.fdt_isa {
        kpln!("  fdt isa          : {}", isa);
    }
    kpln!("  cpu/timebase hz  : {}", cfg.cpu_freq_hz);
    kpln!("  systick hz       : {}", cfg.systick_hz);
    kpln!("  external irqs    : {}", cfg.external_irq_count);
    kpln!(
        "  memory           : {:#x}..{:#x} ({} KiB)",
        cfg.memory_base,
        cfg.memory_base + cfg.memory_size,
        cfg.memory_size / 1024
    );
    kpln!("  kernel end       : {:#x}", cfg.kernel_end);
    #[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
    kpln!("  supervisor csrs  : not probed");
}

struct CpuIdentity {
    root_model: Option<&'static str>,
    root_compatible: Option<&'static str>,
    fdt_isa: Option<&'static str>,
}

impl CpuIdentity {
    fn vendor_name(&self) -> &'static str {
        self.root_compatible
            .and_then(vendor_from_compatible)
            .or_else(|| self.root_model.and_then(vendor_from_compatible))
            .unwrap_or("unknown")
    }

    fn model_name(&self) -> &'static str {
        self.root_model
            .and_then(model_from_string)
            .or_else(|| self.root_compatible.and_then(model_from_string))
            .or(self.root_model)
            .or(self.root_compatible)
            .unwrap_or("unknown")
    }
}

fn collect_cpu_identity() -> CpuIdentity {
    let dtb = crate::platform::dtb_addr();
    if dtb == 0 {
        return CpuIdentity {
            root_model: None,
            root_compatible: None,
            fdt_isa: None,
        };
    }

    let mut identity = CpuIdentity {
        root_model: None,
        root_compatible: None,
        fdt_isa: None,
    };

    unsafe {
        crate::fdt::walk_nodes(dtb, |node| {
            if node.name().is_empty() {
                identity.root_model = node.model();
                identity.root_compatible = node.first_compatible();
                return;
            }

            if identity.fdt_isa.is_none() && node.device_type() == Some("cpu") {
                identity.fdt_isa = node.riscv_isa();
            }
        });
    }

    identity
}

fn vendor_from_compatible(value: &str) -> Option<&'static str> {
    if value.starts_with("espressif,") || value.starts_with("esp,") {
        Some("Espressif")
    } else {
        None
    }
}

fn model_from_string(value: &str) -> Option<&'static str> {
    if value.contains("esp32s31") {
        Some("ESP32-S31")
    } else {
        None
    }
}

fn print_dtb_cpus() -> usize {
    let dtb = crate::platform::dtb_addr();
    if dtb == 0 {
        kpln!("  cpu list         : unknown");
        return 1;
    }

    let mut cpus = [0usize; 8];
    let mut count = 0usize;
    unsafe {
        crate::fdt::walk_nodes(dtb, |node| {
            if count >= cpus.len() || !node.is_available() {
                return;
            }
            if node.device_type() != Some("cpu") {
                return;
            }
            if let Some(reg) = node.first_reg() {
                cpus[count] = reg.base;
                count += 1;
            }
        });
    }

    kp!("  cpu list         : ");
    if count == 0 {
        kpln!("none");
        return 1;
    }
    for (i, cpu) in cpus.iter().take(count).enumerate() {
        if i != 0 {
            kp!(",");
        }
        kp!("{}", cpu);
    }
    kpln!(" ({})", count);
    count
}

fn cmd_drivers() {
    let stats = crate::driver::manager::DriverManager::registry_stats();
    kpln!("Drivers:");
    kpln!(
        "  registry: drivers={}, factories={}, block={}, char={}, term={}, gpio={}, fb={}, net={}, rng={}, probe_matches={}, probe_ok={}, probe_fail={}",
        stats.registered_drivers,
        stats.registered_factories,
        stats.registered_block_devices,
        stats.registered_char_devices,
        stats.registered_terminal_devices,
        stats.registered_gpio_controllers,
        stats.registered_framebuffer_devices,
        stats.registered_net_devices,
        stats.registered_rng_devices,
        stats.probe_matches,
        stats.probe_successes,
        stats.probe_failures
    );
    kpln!(
        "  irq_dispatch: total={}, hits={}, misses={}, fast_slots={}",
        stats.irq_dispatches,
        stats.irq_driver_hits,
        stats.irq_driver_misses,
        stats.irq_fast_slots
    );
    kpln!(
        "  irq_fast: hits={}, fallbacks={}, conflicts={}",
        stats.irq_fast_hits,
        stats.irq_fast_fallbacks,
        stats.irq_fast_conflicts
    );
    let mut count = 0usize;
    crate::driver::manager::DriverManager::for_each_driver(|drv| {
        count += 1;
        let class = driver_class_name(drv);
        kp!(
            "  {}: state={}, class={}",
            drv.name(),
            driver_state_name(drv.state()),
            class
        );
        kpln!("");
    });
    if count == 0 {
        kpln!("  none");
    }
}

fn cmd_term() {
    kpln!("Terminal devices:");
    let mut count = 0usize;
    crate::driver::manager::DriverManager::for_each_terminal_device(|term| {
        let size = term.size();
        kpln!(
            "  {}: {}x{}, mode={}, state={}",
            term.name(),
            size.columns,
            size.rows,
            terminal_mode_name(term.mode()),
            driver_state_name(term.state())
        );
        count += 1;
    });
    if count == 0 {
        kpln!("  none");
    }
}

fn cmd_gpio() {
    kpln!("GPIO controllers:");
    let mut count = 0usize;
    crate::driver::manager::DriverManager::for_each_gpio_controller(|gpio| {
        kpln!(
            "  {}: {} pin(s), state={}",
            gpio.name(),
            gpio.pin_count(),
            driver_state_name(gpio.state())
        );
        count += 1;
    });
    if count == 0 {
        kpln!("  none");
    }
}

fn cmd_irq() {
    let stats = crate::irq::stats();
    let driver_stats = crate::driver::manager::DriverManager::registry_stats();
    kpln!("IRQs:");
    kpln!(
        "  configured={}, registered_handlers={}",
        stats.configured_count,
        stats.registered_handlers
    );
    kpln!(
        "  handled={}, unhandled={}, out_of_range={}",
        stats.handled,
        stats.unhandled,
        stats.out_of_range
    );
    kpln!(
        "  driver_dispatch={}, driver_hits={}, driver_misses={}",
        driver_stats.irq_dispatches,
        driver_stats.irq_driver_hits,
        driver_stats.irq_driver_misses
    );
    kpln!(
        "  driver_fast_slots={}, fast_hits={}, fast_fallbacks={}, fast_conflicts={}",
        driver_stats.irq_fast_slots,
        driver_stats.irq_fast_hits,
        driver_stats.irq_fast_fallbacks,
        driver_stats.irq_fast_conflicts
    );
    kp!("  last=");
    print_irq_value(stats.last_irq);
    kp!(", last_unhandled=");
    print_irq_value(stats.last_unhandled_irq);
    kpln!("");
}

fn print_irq_value(irq: u32) {
    if irq == u32::MAX {
        kp!("none");
    } else {
        kp!("{}", irq);
    }
}

fn driver_class_name(drv: &dyn crate::driver::manager::AnyDriver) -> &'static str {
    drv.device_class_name()
}

fn driver_state_name(state: crate::driver::DeviceState) -> &'static str {
    match state {
        crate::driver::DeviceState::Uninitialized => "uninit",
        crate::driver::DeviceState::Ready => "ready",
        crate::driver::DeviceState::Busy => "busy",
        crate::driver::DeviceState::Error => "error",
        crate::driver::DeviceState::Suspended => "suspend",
    }
}

fn terminal_mode_name(mode: crate::driver::terminal::TerminalMode) -> &'static str {
    match mode {
        crate::driver::terminal::TerminalMode::Raw => "raw",
        crate::driver::terminal::TerminalMode::Cooked => "cooked",
    }
}

fn cmd_tasks() {
    let stats = crate::sched::Scheduler::stats();
    kpln!("Tasks:");
    kpln!(
        "  scheduler: cpu={}, active_mask={:#x}, ready={}, top_prio={}, switches={}, preempts={}, pending={}",
        stats.current_cpu,
        stats.active_cpu_mask,
        stats.ready_tasks,
        stats.highest_ready_priority,
        stats.context_switches,
        stats.preemptions,
        stats.preempt_pending,
    );
    if let Some(current) = stats.current_task {
        kpln!(
            "  current: {}#{} prio={}",
            current.name,
            current.id,
            current.priority
        );
    }
    kpln!("  id  prio state      stack  wakeup name");

    let (tasks, count) = crate::sched::Scheduler::task_snapshot();
    let shown = count.min(tasks.len());
    for task in tasks.iter().take(shown).flatten() {
        kpln!(
            "  {}{}  {}    {}  {}  {} {}",
            task.id,
            if task.current { "*" } else { " " },
            task.priority,
            task_state_name(task.state),
            task.stack_size,
            task.wakeup_tick,
            task.name,
        );
    }
    if count > shown {
        kpln!("  ... {} more", count - shown);
    }
}

fn cmd_top() {
    let sched_stats = crate::sched::Scheduler::stats();
    let cs_abi = crate::sched::Scheduler::context_switch_abi_snapshot();
    let mm_stats = crate::mm::stats();
    let irq_stats = crate::irq::stats();

    kpln!("System Status:");
    kpln!("========================================");

    // Uptime
    let ticks = crate::timer::ticks();
    let secs = ticks / 1000;
    let ms = ticks % 1000;
    kpln!("Uptime: {}.{:03}s", secs, ms);

    // Scheduler
    kpln!("----------------------------------------");
    kpln!("Scheduler:");
    kpln!("  current_cpu    : {}", sched_stats.current_cpu);
    kpln!("  active_mask    : {:#x}", sched_stats.active_cpu_mask);
    kpln!("  ready_tasks    : {}", sched_stats.ready_tasks);
    kpln!("  top_priority   : {}", sched_stats.highest_ready_priority);
    kpln!("  ctx_switches   : {}", sched_stats.context_switches);
    kpln!("  preemptions    : {}", sched_stats.preemptions);
    kpln!("  preempt_pending: {}", sched_stats.preempt_pending);
    kpln!("  steal_attempts : {}", sched_stats.steal_attempts);
    kpln!("  steal_successes: {}", sched_stats.steal_successes);
    kpln!("  current_abi   : {:#x}", cs_abi.current_abi);
    kpln!("  next_abi      : {:#x}", cs_abi.next_abi);
    kpln!("  current_shadow: {:#x}", cs_abi.current_shadow);
    kpln!("  next_shadow   : {:#x}", cs_abi.next_shadow);
    kpln!("  current_slot  : {:#x}", cs_abi.current_slot);
    kpln!("  next_slot     : {:#x}", cs_abi.next_slot);
    if let Some(current) = sched_stats.current_task {
        kpln!(
            "  current_task   : {}#{} prio={}",
            current.name,
            current.id,
            current.priority
        );
    }

    // CPU/SMP info
    let smp_status = crate::platform::smp_status();
    kpln!("----------------------------------------");
    kpln!("CPUs:");
    kpln!("  enabled        : {}", smp_status.enabled);
    kpln!("  possible       : {}", smp_status.possible_cpus);
    kpln!("  online         : {}", smp_status.online_cpus);
    kpln!("  active         : {}", smp_status.active_cpus);
    kpln!("  parked         : {}", smp_status.parked_cpus);
    kpln!("  online_mask    : {:#x}", smp_status.online_mask);
    kpln!("  active_mask    : {:#x}", smp_status.active_mask);
    kpln!("  parked_mask    : {:#x}", smp_status.parked_mask);
    kpln!("  current_cpu    : {}", smp_status.current_cpu);

    // Memory
    kpln!("----------------------------------------");
    kpln!("Memory:");
    kpln!("  heap_used      : {} bytes", mm_stats.heap.used);
    kpln!("  heap_total     : {} bytes", mm_stats.heap.size);
    kpln!("  heap_allocs    : {}", mm_stats.heap.allocations);
    kpln!("  heap_failed    : {}", mm_stats.heap.failed_allocations);
    kpln!("  frames_free    : {} pages", mm_stats.frames.free_pages);
    kpln!("  frames_total   : {} pages", mm_stats.frames.total_pages);
    let frame_free_kb = mm_stats.frames.free_pages * crate::mm::PAGE_SIZE / 1024;
    let frame_total_kb = mm_stats.frames.total_pages * crate::mm::PAGE_SIZE / 1024;
    kpln!(
        "  frames_free_kb : {} KiB / {} KiB",
        frame_free_kb,
        frame_total_kb
    );

    // IRQ
    kpln!("----------------------------------------");
    kpln!("IRQs:");
    kpln!("  handled        : {}", irq_stats.handled);
    kpln!("  unhandled      : {}", irq_stats.unhandled);
    kpln!("  out_of_range   : {}", irq_stats.out_of_range);

    // Tasks
    kpln!("----------------------------------------");
    kpln!("Tasks:");
    kpln!(
        "  {:>4} {:>4} {:<8} {:>6} {:<8} {}",
        "ID",
        "PRI",
        "STATE",
        "STACK",
        "CPU",
        "NAME"
    );

    let (tasks, count) = crate::sched::Scheduler::task_snapshot();
    let shown = count.min(tasks.len());
    for task in tasks.iter().take(shown).flatten() {
        kpln!(
            "  {:>4} {:>4} {:<8} {:>6} {:<8} {}",
            task.id,
            task.priority,
            task_state_name(task.state),
            task.stack_size,
            if task.current { "*" } else { " " },
            task.name,
        );
    }
    if count > shown {
        kpln!("  ... {} more", count - shown);
    }
}

fn task_state_name(state: crate::sched::task::TaskState) -> &'static str {
    match state {
        crate::sched::task::TaskState::Ready => "ready",
        crate::sched::task::TaskState::Running => "running",
        crate::sched::task::TaskState::Blocked => "blocked",
        crate::sched::task::TaskState::Suspended => "suspend",
        crate::sched::task::TaskState::Terminated => "term",
        crate::sched::task::TaskState::Sleeping => "sleep",
    }
}

fn cmd_cd(state: &mut ShellState, path_arg: &str) {
    let target = if path_arg.is_empty() { "/" } else { path_arg };
    let path_buf = match normalize_path(state, target) {
        Some(path) => path,
        None => {
            kpln!("cd: '{}': No such directory", target);
            return;
        }
    };
    let path = path_buf.as_str();

    match crate::fs::path_file_type(path) {
        Some(crate::fs::FileType::Directory) => {
            if !state.set_cwd(path) {
                kpln!("cd: '{}': path too long", target);
            }
        }
        Some(crate::fs::FileType::File) => kpln!("cd: '{}': Not a directory", target),
        None => kpln!("cd: '{}': No such directory", target),
    }
}

fn cmd_ls(state: &ShellState, path_arg: &str) {
    let display_path = if path_arg.is_empty() {
        state.cwd_str()
    } else {
        path_arg
    };
    let path_buf = match normalize_path(state, path_arg) {
        Some(path) => path,
        None => {
            kpln!("ls: '{}': No such file or directory", display_path);
            return;
        }
    };
    let path = path_buf.as_str();

    // RAMFS owns built-in directories such as /dev. Only fall back to mounted
    // filesystems when the path is not present in RAMFS.
    match crate::fs::resolve_path(path) {
        Ok(n) => {
            if crate::fs::node_file_type(n) == Ok(crate::fs::FileType::File) {
                kpln!("{}", crate::fs::node_name(n).unwrap_or(path));
                return;
            }
            let mut entries = [const { None }; LS_BUF_CAP];
            let mut count: usize = 0;
            let _ = crate::fs::list_dir(n, &mut |entry| {
                push_dir_entry(&mut entries, &mut count, entry);
            });
            crate::fs::list_mount_children(path, &mut |entry| {
                push_dir_entry(&mut entries, &mut count, entry);
            });
            print_dir_entries(&entries, count);
        }
        Err(_) => {
            let mut mnt_count: usize = 0;
            let mut entries = [const { None }; LS_BUF_CAP];
            match crate::fs::list_path(path, &mut |entry| {
                push_dir_entry(&mut entries, &mut mnt_count, entry);
            }) {
                Some(_) => {
                    print_dir_entries(&entries, mnt_count);
                    return;
                }
                None => {}
            }
            kpln!("ls: '{}': No such file or directory", display_path);
        }
    }
}

fn list_directory_entries(
    path: &str,
    callback: &mut dyn FnMut(&crate::fs::DirEntry),
) -> Option<usize> {
    if let Ok(node) = crate::fs::resolve_path(path) {
        let mut count = 0;
        let _ = crate::fs::list_dir(node, &mut |entry| {
            callback(entry);
            count += 1;
        });
        count += crate::fs::list_mount_children(path, callback);
        return Some(count);
    }

    crate::fs::list_path(path, callback)
}

fn push_dir_entry(
    entries: &mut [Option<crate::fs::DirEntry>; LS_BUF_CAP],
    count: &mut usize,
    entry: &crate::fs::DirEntry,
) {
    if *count < entries.len() {
        entries[*count] = Some(entry.clone());
    }
    *count += 1;
}

fn print_dir_entries(entries: &[Option<crate::fs::DirEntry>; LS_BUF_CAP], count: usize) {
    let shown = count.min(entries.len());
    for entry in entries.iter().take(shown).flatten() {
        print_dir_entry(entry);
    }
    if count > shown {
        kp!("... ");
    }
    if count > 0 {
        kpln!("");
    }
}

fn print_dir_entry(entry: &crate::fs::DirEntry) {
    match entry.file_type {
        crate::fs::FileType::Directory => kp!("{}/  ", entry.name_str()),
        crate::fs::FileType::File => kp!("{}  ", entry.name_str()),
    };
}

fn cmd_cat(state: &ShellState, path_arg: &str) {
    if path_arg.is_empty() {
        kpln!("cat: missing file operand");
        return;
    }
    let path_buf = match normalize_path(state, path_arg) {
        Some(path) => path,
        None => {
            kpln!("cat: '{}': No such file", path_arg);
            return;
        }
    };
    let path = path_buf.as_str();

    // Try VFS first
    match crate::fs::resolve_path(path) {
        Ok(node) => {
            if crate::fs::node_file_type(node) != Ok(crate::fs::FileType::File) {
                kpln!("cat: '{}': Is a directory", path_arg);
                return;
            }
            let mut buf = [0u8; crate::fs::FILE_MAX_SIZE];
            let n = crate::fs::read_file(node, &mut buf).unwrap_or(0);
            if n > 0 {
                match core::str::from_utf8(&buf[..n]) {
                    Ok(s) => kpln!("{}", s),
                    Err(_) => kpln!("(binary, {} bytes)", n),
                }
            } else {
                kpln!("(empty file)");
            }
        }
        Err(_) => {
            // Not in VFS - check mount points
            let mut buf = [0u8; 4096];
            let n = crate::fs::read_mount_file(path, &mut buf);
            if n > 0 {
                match core::str::from_utf8(&buf[..n]) {
                    Ok(s) => kpln!("{}", s),
                    Err(_) => kpln!("(binary, {} bytes)", n),
                }
            } else {
                kpln!("cat: '{}': No such file", path_arg);
            }
        }
    }
}

fn cmd_mkdir(state: &ShellState, name: &str) {
    if name.is_empty() {
        kpln!("mkdir: missing operand");
        return;
    }

    let (parent_path, child_name) = match split_parent_child(state, name) {
        Some(v) => v,
        None => {
            kpln!("mkdir: cannot create directory '{}'", name);
            return;
        }
    };
    let parent = match crate::fs::resolve_path(parent_path.as_str()) {
        Ok(node) => node,
        Err(_) => {
            kpln!("mkdir: cannot create directory '{}'", name);
            return;
        }
    };
    match crate::fs::create_dir(parent, child_name) {
        Ok(_) => {}
        Err(_) => kpln!("mkdir: cannot create directory '{}'", name),
    }
}

fn cmd_touch(state: &ShellState, name: &str) {
    if name.is_empty() {
        kpln!("touch: missing operand");
        return;
    }

    let (parent_path, child_name) = match split_parent_child(state, name) {
        Some(v) => v,
        None => {
            kpln!("touch: cannot create file '{}'", name);
            return;
        }
    };
    let parent = match crate::fs::resolve_path(parent_path.as_str()) {
        Ok(node) => node,
        Err(_) => {
            kpln!("touch: cannot create file '{}'", name);
            return;
        }
    };
    match crate::fs::create_file(parent, child_name) {
        Ok(_) => {}
        Err(_) => kpln!("touch: cannot create file '{}'", name),
    }
}

fn cmd_echo(state: &ShellState, args: &str) {
    if let Some(redir_pos) = args.find('>') {
        let text = args[..redir_pos].trim();
        let filename = args[redir_pos + 1..].trim();
        if filename.is_empty() {
            kpln!("echo: missing filename after '>'");
            return;
        }

        let (parent_path, child_name) = match split_parent_child(state, filename) {
            Some(v) => v,
            None => {
                kpln!("echo: cannot create '{}'", filename);
                return;
            }
        };
        let parent = match crate::fs::resolve_path(parent_path.as_str()) {
            Ok(node) => node,
            Err(_) => {
                kpln!("echo: cannot create '{}'", filename);
                return;
            }
        };
        let node = match crate::fs::lookup(parent, child_name) {
            Ok(n) => n,
            Err(_) => match crate::fs::create_file(parent, child_name) {
                Ok(n) => n,
                Err(_) => {
                    kpln!("echo: cannot create '{}'", filename);
                    return;
                }
            },
        };
        let _ = crate::fs::write_file(node, text.as_bytes());
    } else {
        kpln!("{}", args);
    }
}
fn cmd_mount(args: &str) {
    let (parts, part_count, too_many): ([&str; 3], usize, bool) = {
        let mut p = [""; 3];
        let mut i = 0;
        let mut extra = false;
        for part in args.split_whitespace() {
            if i < 3 {
                p[i] = part;
                i += 1;
            } else {
                extra = true;
            }
        }
        (p, i, extra)
    };

    if part_count == 0 {
        // List all mounts
        kpln!("  / on / type ramfs");
        crate::fs::list_mounts(&mut |source, path, fs_type| {
            kpln!("  {} on {} type {}", source, path, fs_type);
        });
    } else if parts[0] == "-u" {
        if part_count < 2 {
            kpln!("mount: missing path after -u");
        } else if part_count > 2 || too_many {
            kpln!("usage: mount -u <target>");
        } else if crate::fs::unmount(parts[1]) {
            kpln!("unmounted {}", parts[1]);
        } else {
            kpln!("mount: '{}' not found", parts[1]);
        }
    } else {
        if part_count > 2 || too_many {
            kpln!("usage: mount <source> <target>");
            return;
        }
        if part_count < 2 {
            kpln!("mount: missing target");
            kpln!("usage: mount <source> <target>");
            return;
        }

        let src = parts[0];
        let dst = parts[1];
        match crate::fs::mount_fs(src, dst) {
            Ok(()) => kpln!("mounted {} at {}", src, dst),
            Err(err) => kpln!(
                "mount: failed to mount {} at {}: {}",
                src,
                dst,
                err.message()
            ),
        }
    }
}

fn cmd_vm(args: &str) {
    let (subcmd, subargs) = match args.find(' ') {
        Some(pos) => (&args[..pos], args[pos + 1..].trim()),
        None => (args, ""),
    };
    match subcmd {
        "" | "status" => cmd_vm_status(),
        "create" => {
            if subargs.is_empty() {
                kpln!("vm create <name>");
                return;
            }
            match crate::hv::create_vm(subargs) {
                Ok(id) => kpln!("VM '{}' created id={}", subargs, id),
                Err(err) => kpln!("vm: {}", err.message()),
            }
        }
        "list" => {
            let (vms, count) = crate::hv::list_vms();
            kpln!("VMs: {}", count);
            for vm in vms.iter().flatten() {
                kpln!(
                    "  {}#{} state={} vcpus={} guest={:#x}+{:#x} host={:#x} entry={:#x}",
                    vm.name_str(),
                    vm.id,
                    vm.state.as_str(),
                    vm.vcpu_count,
                    vm.guest_base,
                    vm.guest_size,
                    vm.host_base,
                    vm.entry
                );
            }
        }
        "load-demo" => {
            if subargs.is_empty() {
                kpln!("vm load-demo <name>");
                return;
            }
            match crate::hv::load_demo(subargs) {
                Ok(()) => kpln!("VM '{}' demo guest ready", subargs),
                Err(err) => kpln!("vm: {}", err.message()),
            }
        }
        "run" => {
            if subargs.is_empty() {
                kpln!("vm run <name>");
                return;
            }
            match crate::hv::find_vm(subargs) {
                Ok(vm) => {
                    if vm.state != crate::hv::VmState::Ready {
                        kpln!("vm: '{}' is {}", subargs, vm.state.as_str());
                        return;
                    }
                    kpln!("vm: guest execution is not wired to a loader yet");
                }
                Err(err) => kpln!("vm: {}", err.message()),
            }
        }
        "stop" => {
            kpln!("vm: stop is not wired yet");
        }
        _ => kpln!("vm: unknown subcommand '{}'", subcmd),
    }
}

fn cmd_vm_status() {
    let stats = crate::hv::stats();
    kpln!("Hypervisor:");
    kpln!(
        "  compiled={}, enabled={}, state={}, h_extension={}",
        stats.compiled,
        stats.enabled,
        stats.state.as_str(),
        stats.h_extension
    );
    kpln!(
        "  vms={}, vcpus={}, exits={}",
        stats.vm_count,
        stats.vcpu_count,
        stats.exits
    );
    kpln!(
        "  last_exit={}, cause={:#x}, stval={:#x}",
        stats.last_exit.as_str(),
        stats.last_cause,
        stats.last_stval
    );
}
