//! Built-in kernel shell.
//!
//! This is a minimal shell that runs when no init process is found.
//! Can be disabled with `cfg(feature = "builtin_shell")` to use an external shell from VFS.

use crate::sched::Scheduler;
use crate::{kp, kpln};

pub const BUILTIN_SHELL_STACK_SIZE: usize = 2048;

static mut CWD: [u8; 256] = [0u8; 256];
static mut CWD_LEN: usize = 1; // "/"

/// Escape sequence parser state
#[derive(Clone, Copy, PartialEq, Eq)]
enum EscState {
    Normal,
    SawEsc,
    SawBracket,
    Saw3, // \x1b[3 — delete key prefix
}

pub fn shell_main() -> ! {
    unsafe {
        CWD[0] = b'/';
        CWD_LEN = 1;
    }

    kpln!("===================================");
    kpln!(" OpenIon Shell v0.1.0 ");
    kpln!(" Type 'help' for commands ");
    kpln!("===================================");
    shell_prompt();

    let mut input_buf = [0u8; 128];
    let mut cursor_pos: usize = 0; // insertion point
    let mut cursor_end: usize = 0; // end of actual text
    let mut esc_state = EscState::Normal;

    loop {
        if let Some(c) = crate::driver::char::pop_from_rx_buf() {
            match esc_state {
                EscState::SawBracket => {
                    esc_state = EscState::Normal;
                    match c {
                        b'A' => { /* up: no history, ignore */ }
                        b'B' => { /* down: ignore */ }
                        b'C' => { if cursor_pos < cursor_end { cursor_pos += 1; redraw_input(&input_buf, cursor_pos, cursor_end); } }
                        b'D' => { if cursor_pos > 0 { cursor_pos -= 1; redraw_input(&input_buf, cursor_pos, cursor_end); } }
                        b'H' => { cursor_pos = 0; redraw_input(&input_buf, cursor_pos, cursor_end); }
                        b'F' => { cursor_pos = cursor_end; redraw_input(&input_buf, cursor_pos, cursor_end); }
                        b'3' => { esc_state = EscState::Saw3; }
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
                        redraw_input(&input_buf, cursor_pos, cursor_end);
                    }
                    continue;
                }
                EscState::SawEsc => {
                    esc_state = if c == b'[' { EscState::SawBracket } else { EscState::Normal };
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
                    execute_command(cmd_str.trim());
                }
                cursor_pos = 0;
                cursor_end = 0;
                shell_prompt();
            } else if c == 8 || c == 127 {
                // Backspace: delete character before cursor, shift remaining left
                if cursor_pos > 0 {
                    for i in cursor_pos..cursor_end {
                        input_buf[i - 1] = input_buf[i];
                    }
                    cursor_end -= 1;
                    cursor_pos -= 1;
                    redraw_input(&input_buf, cursor_pos, cursor_end);
                }
            } else if c >= 0x20 && c < 0x7f {
                // Printable ASCII: insert at cursor_pos
                if cursor_end < input_buf.len() {
                    for i in (cursor_pos..cursor_end).rev() {
                        input_buf[i + 1] = input_buf[i];
                    }
                    input_buf[cursor_pos] = c;
                    cursor_pos += 1;
                    cursor_end += 1;
                    redraw_input(&input_buf, cursor_pos, cursor_end);
                }
            }
        } else {
            Scheduler::yield_task();
        }
    }
}

fn shell_prompt() {
    let cwd = unsafe { core::str::from_utf8(&CWD[..CWD_LEN]).unwrap_or("/") };
    kp!("{} $ ", cwd);
}

/// Redraw the input line: clear, print prompt + buffer, reposition cursor.
fn redraw_input(buf: &[u8], pos: usize, end: usize) {
    let cwd = unsafe { core::str::from_utf8(&CWD[..CWD_LEN]).unwrap_or("/") };
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

fn execute_command(input: &str) {
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
        "ls" => cmd_ls(args),
        "cat" => cmd_cat(args),
        "mkdir" => cmd_mkdir(args),
        "touch" => cmd_touch(args),
        "echo" => cmd_echo(args),
        "vm" => cmd_vm(args),
        "mount" => cmd_mount(args),
        "mem" => cmd_mem(),
        "tasks" | "ps" => cmd_tasks(),
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
    kpln!("  tasks / ps        List running tasks");
    kpln!("  ls [path]         List directory");
    kpln!("  cat <file>        Read file");
    kpln!("  echo <text> > <f> Write text to file");
    kpln!("  mkdir <name>      Create directory");
    kpln!("  touch <name>      Create empty file");
    kpln!("  vm create <name>  Create a VM");
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

fn cmd_mem() {
    kpln!("Memory: (frame allocator stats not yet wired)");
}

fn cmd_tasks() {
    kpln!("Tasks: (scheduler introspection not yet implemented)");
}

fn cmd_ls(path_arg: &str) {
    let path = if path_arg.is_empty() { "/" } else { path_arg };

    // First, check if this path is a mount point (or under one)
    let mut mnt_count: usize = 0;
    match crate::fs::list_path(path, &mut |entry| {
        match entry.file_type {
            crate::fs::FileType::Directory => kp!("{}/  ", entry.name_str()),
            crate::fs::FileType::File => kp!("{}  ", entry.name_str()),
        };
        mnt_count += 1;
        if mnt_count % 5 == 0 { kpln!(""); }
    }) {
        Some(0) => return,
        Some(_) => { if mnt_count % 5 != 0 { kpln!(""); } return; }
        None => {}
    }

    // Try VFS resolution
    let node = crate::fs::resolve_path(path);
    match node {
        Some(n) => {
            unsafe {
                if (*n).file_type() == crate::fs::FileType::File {
                    kpln!("{}", (*n).name());
                    return;
                }
            }
            let mut count: usize = 0;
            crate::fs::list_dir(n, &mut |entry| {
                match entry.file_type {
                    crate::fs::FileType::Directory => kp!("{}/  ", entry.name_str()),
                    crate::fs::FileType::File => kp!("{}  ", entry.name_str()),
                };
                count += 1;
                if count % 5 == 0 { kpln!(""); }
            });
            if count == 0 {
                kpln!("(empty)");
            } else if count % 5 != 0 {
                kpln!("");
            }
        }
        None => {
            kpln!("ls: '{}': No such file or directory", path);
        }
    }
}

fn cmd_cat(path: &str) {
    if path.is_empty() {
        kpln!("cat: missing file operand");
        return;
    }
    // Try VFS first
    match crate::fs::resolve_path(path) {
        Some(node) => {
            unsafe {
                if (*node).file_type() != crate::fs::FileType::File {
                    kpln!("cat: '{}': Is a directory", path);
                    return;
                }
            }
            let mut buf = [0u8; crate::fs::FILE_MAX_SIZE];
            let n = crate::fs::read_file(node, &mut buf);
            if n > 0 {
                match core::str::from_utf8(&buf[..n]) {
                    Ok(s) => kpln!("{}", s),
                    Err(_) => kpln!("(binary, {} bytes)", n),
                }
            } else {
                kpln!("(empty file)");
            }
        }
        None => {
            // Not in VFS — check mount points
            let mut buf = [0u8; 4096];
            let n = crate::fs::read_mount_file(path, &mut buf);
            if n > 0 {
                match core::str::from_utf8(&buf[..n]) {
                    Ok(s) => kpln!("{}", s),
                    Err(_) => kpln!("(binary, {} bytes)", n),
                }
            } else {
                kpln!("cat: '{}': No such file", path);
            }
        }
    }
}

fn cmd_mkdir(name: &str) {
    if name.is_empty() {
        kpln!("mkdir: missing operand");
        return;
    }
    let root = match crate::fs::root() { Some(r) => r, None => return };
    match crate::fs::create_dir(root, name) {
        Some(_) => {}
        None => kpln!("mkdir: cannot create directory '{}'", name),
    }
}

fn cmd_touch(name: &str) {
    if name.is_empty() {
        kpln!("touch: missing operand");
        return;
    }
    let root = match crate::fs::root() { Some(r) => r, None => return };
    match crate::fs::create_file(root, name) {
        Some(_) => {}
        None => kpln!("touch: cannot create file '{}'", name),
    }
}

fn cmd_echo(args: &str) {
    if let Some(redir_pos) = args.find('>') {
        let text = args[..redir_pos].trim();
        let filename = args[redir_pos + 1..].trim();
        if filename.is_empty() {
            kpln!("echo: missing filename after '>'");
            return;
        }
        let root = match crate::fs::root() { Some(r) => r, None => return };
        let node = match crate::fs::lookup(root, filename) {
            Some(n) => n,
            None => match crate::fs::create_file(root, filename) {
                Some(n) => n,
                None => { kpln!("echo: cannot create '{}'", filename); return; }
            },
        };
        crate::fs::write_file(node, text.as_bytes());
    } else {
        kpln!("{}", args);
    }
}

fn cmd_mount(args: &str) {
    let parts: [&str; 3] = {
        let mut p = [""; 3];
        let mut i = 0;
        for part in args.split_whitespace() {
            if i < 3 { p[i] = part; i += 1; }
        }
        p
    };

    if parts[0].is_empty() {
        // List all mounts
        kpln!("  / on / type ramfs");
        crate::fs::list_mounts(&mut |path, fs_type| {
            kpln!("  {} on {} type {}", fs_type, path, fs_type);
        });
    } else if parts[0] == "-u" {
        if parts[1].is_empty() {
            kpln!("mount: missing path after -u");
        } else if crate::fs::unmount(parts[1]) {
            kpln!("unmounted {}", parts[1]);
        } else {
            kpln!("mount: '{}' not found", parts[1]);
        }
    } else {
        // mount <source> <target>
        let src = parts[0];
        let dst = if parts[1].is_empty() { "/mnt" } else { parts[1] };
        if crate::fs::mount_fs(src, dst) {
            kpln!("mounted {} at {}", src, dst);
        } else {
            kpln!("mount: failed to mount {} at {}", src, dst);
        }
    }
}

fn cmd_vm(args: &str) {
    let (subcmd, subargs) = match args.find(' ') {
        Some(pos) => (&args[..pos], args[pos + 1..].trim()),
        None => (args, ""),
    };
    match subcmd {
        "create" => {
            if subargs.is_empty() {
                kpln!("vm create <name>");
                return;
            }
            kpln!("VM '{}' created (placeholder)", subargs);
        }
        "list" => {
            kpln!("VMs: (none)");
        }
        "run" => {
            if subargs.is_empty() {
                kpln!("vm run <name>");
                return;
            }
            kpln!("VM '{}' run (not yet implemented)", subargs);
        }
        "stop" => {
            kpln!("VM stop (not yet implemented)");
        }
        _ => kpln!("vm: unknown subcommand '{}'", subcmd),
    }
}
