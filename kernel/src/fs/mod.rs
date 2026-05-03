pub mod detect;
pub mod exfat;

pub const NAME_MAX: usize = 32;
pub const DIR_MAX_ENTRIES: usize = 16;
pub const FILE_MAX_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    File,
    Directory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    NotFound,
    NotDirectory,
    IsDirectory,
    AlreadyExists,
    NoSpace,
    NameTooLong,
}

pub type FsResult<T> = Result<T, FsError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeId(usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenFlags(u32);

impl OpenFlags {
    pub const READ: Self = Self(1 << 0);
    pub const WRITE: Self = Self(1 << 1);
    pub const CREATE: Self = Self(1 << 2);
    pub const TRUNC: Self = Self(1 << 3);
    pub fn read(&self) -> bool {
        self.0 & 1 != 0
    }
    pub fn write(&self) -> bool {
        self.0 & 2 != 0
    }
    pub fn create(&self) -> bool {
        self.0 & 4 != 0
    }
    pub fn trunc(&self) -> bool {
        self.0 & 8 != 0
    }
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

enum Vnode {
    File(FileNode),
    Dir(DirNode),
}

pub struct FileNode {
    pub name: [u8; NAME_MAX],
    pub name_len: usize,
    pub data: [u8; FILE_MAX_SIZE],
    pub size: usize,
}

pub struct DirNode {
    pub name: [u8; NAME_MAX],
    pub name_len: usize,
    pub children: [Option<NodeId>; DIR_MAX_ENTRIES],
}

impl Vnode {
    pub fn new_file(name: &str) -> Self {
        let mut name_buf = [0u8; NAME_MAX];
        let len = name.len().min(NAME_MAX - 1);
        name_buf[..len].copy_from_slice(&name.as_bytes()[..len]);
        Vnode::File(FileNode {
            name: name_buf,
            name_len: len,
            data: [0u8; FILE_MAX_SIZE],
            size: 0,
        })
    }
    pub fn new_dir(name: &str) -> Self {
        let mut name_buf = [0u8; NAME_MAX];
        let len = name.len().min(NAME_MAX - 1);
        name_buf[..len].copy_from_slice(&name.as_bytes()[..len]);
        Vnode::Dir(DirNode {
            name: name_buf,
            name_len: len,
            children: [None; DIR_MAX_ENTRIES],
        })
    }
    pub fn name(&self) -> &str {
        match self {
            Vnode::File(f) => core::str::from_utf8(&f.name[..f.name_len]).unwrap_or(""),
            Vnode::Dir(d) => core::str::from_utf8(&d.name[..d.name_len]).unwrap_or(""),
        }
    }
    pub fn file_type(&self) -> FileType {
        match self {
            Vnode::File(_) => FileType::File,
            Vnode::Dir(_) => FileType::Directory,
        }
    }
}

const NODE_POOL_CAP: usize = 64;
const ROOT_UNSET: usize = usize::MAX;

use crate::sync::Mutex;
use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicUsize, Ordering};

struct Pool(UnsafeCell<[MaybeUninit<Vnode>; NODE_POOL_CAP]>);
unsafe impl Sync for Pool {}

static POOL: Pool = Pool(UnsafeCell::new(
    [const { MaybeUninit::uninit() }; NODE_POOL_CAP],
));
static NODE_COUNT: AtomicUsize = AtomicUsize::new(0);
static RAMFS_ROOT: AtomicUsize = AtomicUsize::new(ROOT_UNSET);

#[derive(Clone, Copy)]
struct MountInfo {
    source: [u8; 64],
    source_len: usize,
    path: [u8; 64],
    path_len: usize,
    fs: exfat::ExfatFs,
    dev: &'static crate::driver::block::DynBlockDevice,
}

#[derive(Clone, Copy)]
pub struct MountEntry {
    pub source: [u8; 64],
    pub source_len: usize,
    pub path: [u8; 64],
    pub path_len: usize,
    pub fs_type: &'static str,
}

impl MountEntry {
    pub fn source_str(&self) -> &str {
        core::str::from_utf8(&self.source[..self.source_len]).unwrap_or("")
    }

    pub fn path_str(&self) -> &str {
        core::str::from_utf8(&self.path[..self.path_len]).unwrap_or("")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MountError {
    InvalidSource,
    InvalidTarget,
    TargetBusy,
    NoBlockDevice,
    UnsupportedFs,
    IoTimeout,
    NoSpace,
}

impl MountError {
    pub fn message(self) -> &'static str {
        match self {
            MountError::InvalidSource => "invalid source device",
            MountError::InvalidTarget => "invalid mount target",
            MountError::TargetBusy => "target is already mounted",
            MountError::NoBlockDevice => "block device not found",
            MountError::UnsupportedFs => "unsupported or invalid filesystem",
            MountError::IoTimeout => "block device I/O timed out",
            MountError::NoSpace => "mount table is full",
        }
    }
}

static MOUNTS: Mutex<[Option<MountInfo>; 4]> = Mutex::new([None; 4]);

#[derive(Clone, Copy)]
struct MountLookup {
    fs: exfat::ExfatFs,
    prefix_len: usize,
    dev: &'static crate::driver::block::DynBlockDevice,
}

pub fn init() {
    if root().is_ok() {
        return;
    }

    if let Ok(root) = alloc_node(Vnode::new_dir("/")) {
        RAMFS_ROOT.store(root.0, Ordering::Release);
        let _ = create_dir(root, "dev");
        let _ = create_dir(root, "proc");
        let _ = create_dir(root, "mnt");
    }
    crate::kinfo!("VFS initialized: /dev /proc /mnt");
}

fn register_mount(
    dev_path: &str,
    mount_path: &str,
    fs: exfat::ExfatFs,
    dev: &'static crate::driver::block::DynBlockDevice,
) -> Result<(), MountError> {
    let mut source = [0u8; 64];
    let source_len = dev_path.len().min(63);
    source[..source_len].copy_from_slice(&dev_path.as_bytes()[..source_len]);

    let mut mnt_path = [0u8; 64];
    let len = mount_path.len().min(63);
    mnt_path[..len].copy_from_slice(&mount_path.as_bytes()[..len]);
    let mut mounts = MOUNTS.lock();
    for slot in mounts.iter() {
        if let Some(m) = slot {
            let mp = core::str::from_utf8(&m.path[..m.path_len]).unwrap_or("");
            if mp == mount_path {
                return Err(MountError::TargetBusy);
            }
        }
    }
    for slot in mounts.iter_mut() {
        if slot.is_none() {
            *slot = Some(MountInfo {
                source,
                source_len,
                path: mnt_path,
                path_len: len,
                fs,
                dev,
            });
            return Ok(());
        }
    }
    Err(MountError::NoSpace)
}

pub fn mount_fs(dev_path: &str, mount_path: &str) -> Result<(), MountError> {
    if mount_path.is_empty() || !mount_path.starts_with('/') || mount_path.len() >= 64 {
        return Err(MountError::InvalidTarget);
    }
    if !valid_mount_target(mount_path) {
        return Err(MountError::InvalidTarget);
    }

    let dev = find_block_device(dev_path).ok_or(MountError::NoBlockDevice)?;

    let detected = detect::detect_fs(dev);
    if !detected.sector0_valid {
        return Err(MountError::IoTimeout);
    }
    if detected.fs_type != detect::FsType::Exfat {
        return Err(MountError::UnsupportedFs);
    }

    let fs = try_mount_exfat(dev).map_err(|_| MountError::UnsupportedFs)?;
    register_mount(dev_path, mount_path, fs, dev)
}

fn valid_mount_target(path: &str) -> bool {
    if path == "/" || path.ends_with('/') {
        return false;
    }

    let Some(slash) = path.rfind('/') else {
        return false;
    };
    let name = &path[slash + 1..];
    if name.is_empty() || name.len() >= NAME_MAX || name.contains('/') {
        return false;
    }

    let parent = if slash == 0 { "/" } else { &path[..slash] };
    matches!(resolve_path(parent), Ok(node) if node_file_type(node) == Ok(FileType::Directory))
}

fn find_block_device(dev_path: &str) -> Option<&'static crate::driver::block::DynBlockDevice> {
    let dev_name = dev_path.strip_prefix("/dev/")?;
    let wanted_idx = parse_blk_name(dev_name)?;

    let mut idx = 0usize;
    let mut found: Option<&'static crate::driver::block::DynBlockDevice> = None;
    crate::driver::manager::DriverManager::for_each_driver(|drv| {
        if found.is_some() {
            return;
        }
        if let Some(blk_dev) = drv.as_block_device() {
            if idx == wanted_idx {
                found = Some(blk_dev);
            } else {
                idx += 1;
            }
        }
    });
    found
}

fn parse_blk_name(name: &str) -> Option<usize> {
    let digits = name.strip_prefix("blk")?;
    if digits.is_empty() {
        return None;
    }

    let mut value = 0usize;
    for b in digits.bytes() {
        if !b.is_ascii_digit() {
            return None;
        }
        value = value.checked_mul(10)?.checked_add((b - b'0') as usize)?;
    }
    Some(value)
}

fn find_mount_for_path(path: &str) -> Option<MountLookup> {
    let mounts = MOUNTS.lock();
    let mut best: Option<MountLookup> = None;
    let mut best_len = 0;
    for slot in mounts.iter() {
        if let Some(m) = slot {
            let mp = core::str::from_utf8(&m.path[..m.path_len]).unwrap_or("");
            if path == mp || (path.starts_with(mp) && path.as_bytes().get(mp.len()) == Some(&b'/'))
            {
                if mp.len() > best_len {
                    best = Some(MountLookup {
                        fs: m.fs,
                        prefix_len: mp.len(),
                        dev: m.dev,
                    });
                    best_len = mp.len();
                }
            }
        }
    }
    best
}

pub fn unmount(path: &str) -> bool {
    let mut mounts = MOUNTS.lock();
    for slot in mounts.iter_mut() {
        if let Some(m) = slot {
            let mp = core::str::from_utf8(&m.path[..m.path_len]).unwrap_or("");
            if mp == path {
                *slot = None;
                return true;
            }
        }
    }
    false
}

pub fn list_mounts(callback: &mut dyn FnMut(&str, &str, &str)) {
    let entries = mount_entries();
    for entry in entries.iter().flatten() {
        callback(entry.source_str(), entry.path_str(), entry.fs_type);
    }
}

pub fn mount_entries() -> [Option<MountEntry>; 4] {
    let mut entries = [None; 4];
    {
        let mounts = MOUNTS.lock();
        for (dst, src) in entries.iter_mut().zip(mounts.iter()) {
            if let Some(m) = src {
                *dst = Some(MountEntry {
                    source: m.source,
                    source_len: m.source_len,
                    path: m.path,
                    path_len: m.path_len,
                    fs_type: "exFAT",
                });
            }
        }
    }
    entries
}

pub fn list_path(path: &str, callback: &mut dyn FnMut(&DirEntry)) -> Option<usize> {
    let mount = find_mount_for_path(path)?;
    let rel = &path[mount.prefix_len..];
    let rel = rel.trim_start_matches('/');

    let mut count: usize = 0;
    struct Adapter<'a> {
        dev: &'a crate::driver::block::DynBlockDevice,
    }
    impl<'a> exfat::BlockDev for Adapter<'a> {
        fn read_sector(&self, sector: u64, buf: &mut [u8]) -> Result<(), ()> {
            self.dev.read_block(sector as usize, buf).map_err(|_| ())
        }
    }
    let adapter = Adapter { dev: mount.dev };
    let entry = match mount.fs.find_entry(&adapter, rel) {
        Some(e) => e,
        None => return None,
    };
    if !entry.is_dir {
        let mut nm = [0u8; NAME_MAX];
        let cl = entry.name_len.min(NAME_MAX - 1);
        nm[..cl].copy_from_slice(&entry.name[..cl]);
        callback(&DirEntry {
            name: nm,
            name_len: cl,
            file_type: FileType::File,
        });
        return Some(1);
    }
    let _ = mount.fs.list_dir(&adapter, entry.first_cluster, &mut |e| {
        let mut nm = [0u8; NAME_MAX];
        let cl = e.name_len.min(NAME_MAX - 1);
        nm[..cl].copy_from_slice(&e.name[..cl]);
        callback(&DirEntry {
            name: nm,
            name_len: cl,
            file_type: if e.is_dir {
                FileType::Directory
            } else {
                FileType::File
            },
        });
        count += 1;
    });
    Some(count)
}

pub fn path_file_type(path: &str) -> Option<FileType> {
    if let Ok(node) = resolve_path(path) {
        return node_file_type(node).ok();
    }

    let mount = find_mount_for_path(path)?;
    let rel = &path[mount.prefix_len..];
    let rel = rel.trim_start_matches('/');

    struct Adapter<'a> {
        dev: &'a crate::driver::block::DynBlockDevice,
    }
    impl<'a> exfat::BlockDev for Adapter<'a> {
        fn read_sector(&self, sector: u64, buf: &mut [u8]) -> Result<(), ()> {
            self.dev.read_block(sector as usize, buf).map_err(|_| ())
        }
    }
    let adapter = Adapter { dev: mount.dev };
    let entry = mount.fs.find_entry(&adapter, rel)?;
    Some(if entry.is_dir {
        FileType::Directory
    } else {
        FileType::File
    })
}

pub fn list_mount_children(parent_path: &str, callback: &mut dyn FnMut(&DirEntry)) -> usize {
    let mounts = mount_entries();
    let mut count = 0;
    for slot in mounts.iter() {
        if let Some(m) = slot {
            let mp = m.path_str();
            if let Some(slash) = mp.rfind('/') {
                let mp_parent = if slash == 0 { "/" } else { &mp[..slash] };
                if mp_parent == parent_path {
                    let name = &mp[slash + 1..];
                    let mut nb = [0u8; NAME_MAX];
                    let cl = name.len().min(NAME_MAX - 1);
                    nb[..cl].copy_from_slice(name.as_bytes());
                    callback(&DirEntry {
                        name: nb,
                        name_len: cl,
                        file_type: FileType::Directory,
                    });
                    count += 1;
                }
            }
        }
    }
    count
}

pub fn read_mount_file(path: &str, buf: &mut [u8]) -> usize {
    let mount = match find_mount_for_path(path) {
        Some(v) => v,
        None => return 0,
    };
    let rel = &path[mount.prefix_len..];
    let rel = rel.trim_start_matches('/');
    if rel.is_empty() {
        return 0;
    }

    let mut result: usize = 0;
    struct Adapter<'a> {
        dev: &'a crate::driver::block::DynBlockDevice,
    }
    impl<'a> exfat::BlockDev for Adapter<'a> {
        fn read_sector(&self, sector: u64, buf: &mut [u8]) -> Result<(), ()> {
            self.dev.read_block(sector as usize, buf).map_err(|_| ())
        }
    }
    let adapter = Adapter { dev: mount.dev };
    if let Some(entry) = mount.fs.find_entry(&adapter, rel) {
        if !entry.is_dir {
            result = mount
                .fs
                .read_file(&adapter, entry.first_cluster, buf)
                .unwrap_or(0);
        }
    }
    result
}

fn try_mount_exfat(dev: &crate::driver::block::DynBlockDevice) -> Result<exfat::ExfatFs, ()> {
    struct BlockDevAdapter<'a> {
        dev: &'a crate::driver::block::DynBlockDevice,
    }
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

fn alloc_node(vnode: Vnode) -> FsResult<NodeId> {
    let idx = NODE_COUNT.fetch_add(1, Ordering::Relaxed);
    if idx >= NODE_POOL_CAP {
        return Err(FsError::NoSpace);
    }
    let base = POOL.0.get() as *mut MaybeUninit<Vnode>;
    let slot = unsafe { &mut *base.add(idx) };
    slot.write(vnode);
    Ok(NodeId(idx))
}

fn node(id: NodeId) -> FsResult<&'static Vnode> {
    if id.0 >= NODE_COUNT.load(Ordering::Acquire) || id.0 >= NODE_POOL_CAP {
        return Err(FsError::NotFound);
    }
    let base = POOL.0.get() as *const MaybeUninit<Vnode>;
    Ok(unsafe { (&*base.add(id.0)).assume_init_ref() })
}

fn node_mut(id: NodeId) -> FsResult<&'static mut Vnode> {
    if id.0 >= NODE_COUNT.load(Ordering::Acquire) || id.0 >= NODE_POOL_CAP {
        return Err(FsError::NotFound);
    }
    let base = POOL.0.get() as *mut MaybeUninit<Vnode>;
    Ok(unsafe { (&mut *base.add(id.0)).assume_init_mut() })
}

pub fn root() -> FsResult<NodeId> {
    let id = RAMFS_ROOT.load(Ordering::Acquire);
    if id == ROOT_UNSET {
        Err(FsError::NotFound)
    } else {
        Ok(NodeId(id))
    }
}

pub fn node_file_type(id: NodeId) -> FsResult<FileType> {
    Ok(node(id)?.file_type())
}

pub fn node_name(id: NodeId) -> FsResult<&'static str> {
    Ok(node(id)?.name())
}

pub fn lookup(dir: NodeId, name: &str) -> FsResult<NodeId> {
    match node(dir)? {
        Vnode::Dir(d) => {
            for child in &d.children {
                if let Some(id) = child {
                    if node(*id)?.name() == name {
                        return Ok(*id);
                    }
                }
            }
            Err(FsError::NotFound)
        }
        _ => Err(FsError::NotDirectory),
    }
}

pub fn create_file(dir: NodeId, name: &str) -> FsResult<NodeId> {
    if name.is_empty() || name.len() >= NAME_MAX {
        return Err(FsError::NameTooLong);
    }
    {
        match node(dir)? {
            Vnode::Dir(d) => {
                for child in &d.children {
                    if let Some(id) = child {
                        if node(*id)?.name() == name {
                            return Err(FsError::AlreadyExists);
                        }
                    }
                }
            }
            _ => return Err(FsError::NotDirectory),
        }
    }

    let new_node = alloc_node(Vnode::new_file(name))?;
    match node_mut(dir)? {
        Vnode::Dir(d) => {
            for slot in d.children.iter_mut() {
                if slot.is_none() {
                    *slot = Some(new_node);
                    return Ok(new_node);
                }
            }
            Err(FsError::NoSpace)
        }
        _ => Err(FsError::NotDirectory),
    }
}

pub fn create_dir(dir: NodeId, name: &str) -> FsResult<NodeId> {
    if name.is_empty() || name.len() >= NAME_MAX {
        return Err(FsError::NameTooLong);
    }
    {
        match node(dir)? {
            Vnode::Dir(d) => {
                for child in &d.children {
                    if let Some(id) = child {
                        if node(*id)?.name() == name {
                            return Err(FsError::AlreadyExists);
                        }
                    }
                }
            }
            _ => return Err(FsError::NotDirectory),
        }
    }

    let new_node = alloc_node(Vnode::new_dir(name))?;
    match node_mut(dir)? {
        Vnode::Dir(d) => {
            for slot in d.children.iter_mut() {
                if slot.is_none() {
                    *slot = Some(new_node);
                    return Ok(new_node);
                }
            }
            Err(FsError::NoSpace)
        }
        _ => Err(FsError::NotDirectory),
    }
}

pub fn write_file(file: NodeId, data: &[u8]) -> FsResult<usize> {
    match node_mut(file)? {
        Vnode::File(f) => {
            let len = data.len().min(FILE_MAX_SIZE);
            f.data[..len].copy_from_slice(&data[..len]);
            f.size = len;
            Ok(len)
        }
        _ => Err(FsError::IsDirectory),
    }
}

pub fn read_file(file: NodeId, buf: &mut [u8]) -> FsResult<usize> {
    match node(file)? {
        Vnode::File(f) => {
            let len = f.size.min(buf.len());
            buf[..len].copy_from_slice(&f.data[..len]);
            Ok(len)
        }
        _ => Err(FsError::IsDirectory),
    }
}

pub fn list_dir(dir: NodeId, callback: &mut dyn FnMut(&DirEntry)) -> FsResult<usize> {
    let mut count = 0;
    match node(dir)? {
        Vnode::Dir(d) => {
            for child in &d.children {
                if let Some(id) = child {
                    let child = node(*id)?;
                    callback(&DirEntry {
                        name: match child {
                            Vnode::File(f) => f.name,
                            Vnode::Dir(d) => d.name,
                        },
                        name_len: match child {
                            Vnode::File(f) => f.name_len,
                            Vnode::Dir(d) => d.name_len,
                        },
                        file_type: child.file_type(),
                    });
                    count += 1;
                }
            }
            Ok(count)
        }
        _ => Err(FsError::NotDirectory),
    }
}

pub fn resolve_path(path: &str) -> FsResult<NodeId> {
    let root = root()?;
    if path == "/" {
        return Ok(root);
    }
    let path = path.trim_start_matches('/');
    let mut current = root;
    for component in path.split('/') {
        if component.is_empty() {
            continue;
        }
        current = lookup(current, component)?;
    }
    Ok(current)
}
