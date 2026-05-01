pub mod exfat;
pub mod detect;

pub const NAME_MAX: usize = 32;
pub const DIR_MAX_ENTRIES: usize = 16;
pub const FILE_MAX_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType { File, Directory }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenFlags(u32);

impl OpenFlags {
    pub const READ: Self = Self(1 << 0);
    pub const WRITE: Self = Self(1 << 1);
    pub const CREATE: Self = Self(1 << 2);
    pub const TRUNC: Self = Self(1 << 3);
    pub fn read(&self) -> bool { self.0 & 1 != 0 }
    pub fn write(&self) -> bool { self.0 & 2 != 0 }
    pub fn create(&self) -> bool { self.0 & 4 != 0 }
    pub fn trunc(&self) -> bool { self.0 & 8 != 0 }
}

#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: [u8; NAME_MAX],
    pub name_len: usize,
    pub file_type: FileType,
}

impl DirEntry {
    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("")
    }
}

pub enum Vnode { File(FileNode), Dir(DirNode) }

pub struct FileNode {
    pub name: [u8; NAME_MAX],
    pub name_len: usize,
    pub data: [u8; FILE_MAX_SIZE],
    pub size: usize,
}

pub struct DirNode {
    pub name: [u8; NAME_MAX],
    pub name_len: usize,
    pub children: [Option<*mut Vnode>; DIR_MAX_ENTRIES],
}

unsafe impl Send for Vnode {}
unsafe impl Sync for Vnode {}

impl Vnode {
    pub fn new_file(name: &str) -> Self {
        let mut name_buf = [0u8; NAME_MAX];
        let len = name.len().min(NAME_MAX - 1);
        name_buf[..len].copy_from_slice(&name.as_bytes()[..len]);
        Vnode::File(FileNode { name: name_buf, name_len: len, data: [0u8; FILE_MAX_SIZE], size: 0 })
    }
    pub fn new_dir(name: &str) -> Self {
        let mut name_buf = [0u8; NAME_MAX];
        let len = name.len().min(NAME_MAX - 1);
        name_buf[..len].copy_from_slice(&name.as_bytes()[..len]);
        Vnode::Dir(DirNode { name: name_buf, name_len: len, children: [None; DIR_MAX_ENTRIES] })
    }
    pub fn name(&self) -> &str {
        match self {
            Vnode::File(f) => core::str::from_utf8(&f.name[..f.name_len]).unwrap_or(""),
            Vnode::Dir(d) => core::str::from_utf8(&d.name[..d.name_len]).unwrap_or(""),
        }
    }
    pub fn file_type(&self) -> FileType {
        match self { Vnode::File(_) => FileType::File, Vnode::Dir(_) => FileType::Directory }
    }
}

static mut RAMFS_ROOT: Option<*mut Vnode> = None;

#[derive(Clone, Copy)]
struct MountInfo { path: [u8; 64], path_len: usize, fs: exfat::ExfatFs }

use crate::sync::Mutex;
static MOUNTS: Mutex<[Option<MountInfo>; 4]> = Mutex::new([None; 4]);

pub fn init() {
    unsafe {
        let root = alloc_node(Vnode::new_dir("/"));
        RAMFS_ROOT = Some(root);
        create_dir(root, "dev");
        create_dir(root, "proc");
        create_dir(root, "mnt");
    }
    crate::kinfo!("VFS initialized: /dev /proc /mnt");
}

fn register_mount(mount_path: &str, fs: exfat::ExfatFs) {
    let mut mnt_path = [0u8; 64];
    let len = mount_path.len().min(63);
    mnt_path[..len].copy_from_slice(&mount_path.as_bytes()[..len]);
    let mut mounts = MOUNTS.lock();
    for slot in mounts.iter_mut() {
        if slot.is_none() {
            *slot = Some(MountInfo { path: mnt_path, path_len: len, fs });
            return;
        }
    }
}

pub fn mount_fs(_dev_path: &str, mount_path: &str) -> bool {
    // Prevent scheduler intervention during mount
    crate::arch::disable_irq();

    let mut found_fs: Option<exfat::ExfatFs> = None;
    crate::driver::manager::DriverManager::for_each_driver(|drv| {
        if found_fs.is_some() { return; }
        if let Some(blk_dev) = drv.as_block_device() {
            if detect::detect_fs(blk_dev).fs_type == detect::FsType::Exfat {
                if let Ok(fs) = try_mount_exfat(blk_dev) {
                    found_fs = Some(fs);
                }
            }
        }
    });

    let ok = if let Some(fs) = found_fs {
        register_mount(mount_path, fs);
        true
    } else {
        false
    };

    crate::arch::enable_irq();
    ok
}

pub fn unmount(path: &str) -> bool {
    let mut mounts = MOUNTS.lock();
    for slot in mounts.iter_mut() {
        if let Some(m) = slot {
            let mp = core::str::from_utf8(&m.path[..m.path_len]).unwrap_or("");
            if mp == path { *slot = None; return true; }
        }
    }
    false
}

pub fn list_mounts(callback: &mut dyn FnMut(&str, &str)) {
    let mounts = MOUNTS.lock();
    for slot in mounts.iter() {
        if let Some(m) = slot {
            let path = core::str::from_utf8(&m.path[..m.path_len]).unwrap_or("");
            callback(path, "exFAT");
        }
    }
}

pub fn list_path(path: &str, callback: &mut dyn FnMut(&DirEntry)) -> Option<usize> {
    let mount_info = {
        let mounts = MOUNTS.lock();
        let mut best: Option<(exfat::ExfatFs, usize)> = None;
        let mut best_len = 0;
        for slot in mounts.iter() {
            if let Some(m) = slot {
                let mp = core::str::from_utf8(&m.path[..m.path_len]).unwrap_or("");
                if path == mp || (path.starts_with(mp) && path.as_bytes().get(mp.len()) == Some(&b'/')) {
                    if mp.len() > best_len { best = Some((m.fs, mp.len())); best_len = mp.len(); }
                }
            }
        }
        best
    };
    let (fs, prefix_len) = match mount_info { Some(v) => v, None => return None };
    let rel = &path[prefix_len..];
    let rel = rel.trim_start_matches('/');

    let mut count: usize = 0;
    let mut found = false;
    crate::driver::manager::DriverManager::for_each_driver(|drv| {
        if found { return; }
        if let Some(blk_dev) = drv.as_block_device() {
            found = true;
            struct Adapter<'a> { dev: &'a crate::driver::block::DynBlockDevice }
            impl<'a> exfat::BlockDev for Adapter<'a> {
                fn read_sector(&self, sector: u64, buf: &mut [u8]) -> Result<(), ()> {
                    self.dev.read_block(sector as usize, buf).map_err(|_| ())
                }
            }
            let adapter = Adapter { dev: blk_dev };
            let entry = match fs.find_entry(&adapter, rel) {
                Some(e) => e,
                None => return,
            };
            if !entry.is_dir { return; }
            let _ = fs.list_dir(&adapter, entry.first_cluster, &mut |e| {
                let mut nm = [0u8; NAME_MAX];
                let cl = e.name_len.min(NAME_MAX - 1);
                nm[..cl].copy_from_slice(&e.name[..cl]);
                callback(&DirEntry { name: nm, name_len: cl, file_type: if e.is_dir { FileType::Directory } else { FileType::File } });
                count += 1;
            });
        }
    });
    Some(count)
}

pub fn list_mount_children(parent_path: &str, callback: &mut dyn FnMut(&DirEntry)) -> usize {
    let mounts = MOUNTS.lock();
    let mut count = 0;
    for slot in mounts.iter() {
        if let Some(m) = slot {
            let mp = core::str::from_utf8(&m.path[..m.path_len]).unwrap_or("");
            if let Some(slash) = mp.rfind('/') {
                let mp_parent = if slash == 0 { "/" } else { &mp[..slash] };
                if mp_parent == parent_path {
                    let name = &mp[slash + 1..];
                    let mut nb = [0u8; NAME_MAX];
                    let cl = name.len().min(NAME_MAX - 1);
                    nb[..cl].copy_from_slice(name.as_bytes());
                    callback(&DirEntry { name: nb, name_len: cl, file_type: FileType::Directory });
                    count += 1;
                }
            }
        }
    }
    count
}

pub fn read_mount_file(path: &str, buf: &mut [u8]) -> usize {
    let mount_info = {
        let mounts = MOUNTS.lock();
        let mut best: Option<(exfat::ExfatFs, usize)> = None;
        let mut best_len = 0;
        for slot in mounts.iter() {
            if let Some(m) = slot {
                let mp = core::str::from_utf8(&m.path[..m.path_len]).unwrap_or("");
                if path == mp || (path.starts_with(mp) && path.as_bytes().get(mp.len()) == Some(&b'/')) {
                    if mp.len() > best_len { best = Some((m.fs, mp.len())); best_len = mp.len(); }
                }
            }
        }
        match best { Some(v) => v, None => return 0 }
    };
    let (fs, prefix_len) = mount_info;
    let rel = &path[prefix_len..];
    let rel = rel.trim_start_matches('/');
    if rel.is_empty() { return 0; }

    let mut result: usize = 0;
    let mut found = false;
    crate::driver::manager::DriverManager::for_each_driver(|drv| {
        if found { return; }
        if let Some(blk_dev) = drv.as_block_device() {
            found = true;
            struct Adapter<'a> { dev: &'a crate::driver::block::DynBlockDevice }
            impl<'a> exfat::BlockDev for Adapter<'a> {
                fn read_sector(&self, sector: u64, buf: &mut [u8]) -> Result<(), ()> {
                    self.dev.read_block(sector as usize, buf).map_err(|_| ())
                }
            }
            let adapter = Adapter { dev: blk_dev };
            if let Some(entry) = fs.find_entry(&adapter, rel) {
                if !entry.is_dir {
                    result = fs.read_file(&adapter, entry.first_cluster, buf).unwrap_or(0);
                }
            }
        }
    });
    result
}

fn try_mount_exfat(dev: &crate::driver::block::DynBlockDevice) -> Result<exfat::ExfatFs, ()> {
    struct BlockDevAdapter<'a> { dev: &'a crate::driver::block::DynBlockDevice }
    impl<'a> exfat::BlockDev for BlockDevAdapter<'a> {
        fn read_sector(&self, sector: u64, buf: &mut [u8]) -> Result<(), ()> {
            self.dev.read_block(sector as usize, buf).map_err(|_| ())
        }
    }
    let adapter = BlockDevAdapter { dev };
    let mut fs = exfat::ExfatFs::new();
    fs.mount(&adapter)?;
    Ok(fs)
}

fn alloc_node(vnode: Vnode) -> *mut Vnode {
    use core::cell::UnsafeCell;
    use core::mem::MaybeUninit;
    use core::sync::atomic::{AtomicUsize, Ordering};
    struct Pool(UnsafeCell<[MaybeUninit<Vnode>; 64]>);
    unsafe impl Sync for Pool {}
    static POOL: Pool = Pool(UnsafeCell::new([const { MaybeUninit::uninit() }; 64]));
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    let idx = COUNT.fetch_add(1, Ordering::Relaxed);
    if idx >= 64 { return core::ptr::null_mut(); }
    let base = POOL.0.get() as *mut MaybeUninit<Vnode>;
    let slot = unsafe { &mut *base.add(idx) };
    slot.write(vnode);
    slot.as_mut_ptr()
}

pub fn root() -> Option<*mut Vnode> { unsafe { RAMFS_ROOT } }

pub fn lookup(dir: *mut Vnode, name: &str) -> Option<*mut Vnode> {
    unsafe {
        match &*dir {
            Vnode::Dir(d) => {
                for child in &d.children {
                    if let Some(ptr) = child { if (**ptr).name() == name { return Some(*ptr); } }
                }
                None
            }
            _ => None,
        }
    }
}

pub fn create_file(dir: *mut Vnode, name: &str) -> Option<*mut Vnode> {
    unsafe {
        match &mut *dir {
            Vnode::Dir(d) => {
                for child in &d.children {
                    if let Some(ptr) = child { if (**ptr).name() == name { return None; } }
                }
                for slot in d.children.iter_mut() {
                    if slot.is_none() {
                        let node = alloc_node(Vnode::new_file(name));
                        if !node.is_null() { *slot = Some(node); return Some(node); }
                        return None;
                    }
                }
                None
            }
            _ => None,
        }
    }
}

pub fn create_dir(dir: *mut Vnode, name: &str) -> Option<*mut Vnode> {
    unsafe {
        match &mut *dir {
            Vnode::Dir(d) => {
                for child in &d.children {
                    if let Some(ptr) = child { if (**ptr).name() == name { return None; } }
                }
                for slot in d.children.iter_mut() {
                    if slot.is_none() {
                        let node = alloc_node(Vnode::new_dir(name));
                        if !node.is_null() { *slot = Some(node); return Some(node); }
                        return None;
                    }
                }
                None
            }
            _ => None,
        }
    }
}

pub fn write_file(file: *mut Vnode, data: &[u8]) -> usize {
    unsafe {
        match &mut *file {
            Vnode::File(f) => {
                let len = data.len().min(FILE_MAX_SIZE);
                f.data[..len].copy_from_slice(&data[..len]);
                f.size = len;
                len
            }
            _ => 0,
        }
    }
}

pub fn read_file(file: *mut Vnode, buf: &mut [u8]) -> usize {
    unsafe {
        match &*file {
            Vnode::File(f) => {
                let len = f.size.min(buf.len());
                buf[..len].copy_from_slice(&f.data[..len]);
                len
            }
            _ => 0,
        }
    }
}

pub fn list_dir(dir: *mut Vnode, callback: &mut dyn FnMut(&DirEntry)) {
    unsafe {
        match &*dir {
            Vnode::Dir(d) => {
                for child in &d.children {
                    if let Some(ptr) = child {
                        let node = &**ptr;
                        callback(&DirEntry {
                            name: match node { Vnode::File(f) => f.name, Vnode::Dir(d) => d.name },
                            name_len: match node { Vnode::File(f) => f.name_len, Vnode::Dir(d) => d.name_len },
                            file_type: node.file_type(),
                        });
                    }
                }
            }
            _ => {}
        }
    }
}

pub fn resolve_path(path: &str) -> Option<*mut Vnode> {
    let root = root()?;
    if path == "/" { return Some(root); }
    let path = path.trim_start_matches('/');
    let mut current = root;
    for component in path.split('/') {
        if component.is_empty() { continue; }
        current = lookup(current, component)?;
    }
    Some(current)
}