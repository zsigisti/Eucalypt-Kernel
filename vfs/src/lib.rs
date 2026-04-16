#![no_std]

extern crate alloc;

use alloc::{boxed::Box, format};
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

pub const O_RDONLY: u32 = 0x0000;
pub const O_WRONLY: u32 = 0x0001;
pub const O_RDWR:   u32 = 0x0002;
pub const O_CREAT:  u32 = 0x0040;
pub const O_TRUNC:  u32 = 0x0200;
pub const O_APPEND: u32 = 0x0400;
pub const O_EXCL:   u32 = 0x0800;

pub const S_IRUSR: u32 = 0o400;
pub const S_IWUSR: u32 = 0o200;
pub const S_IXUSR: u32 = 0o100;
pub const S_IRGRP: u32 = 0o040;
pub const S_IWGRP: u32 = 0o020;
pub const S_IROTH: u32 = 0o004;
pub const S_IFREG: u32 = 0o100000;
pub const S_IFDIR: u32 = 0o040000;
pub const S_IMODE: u32 = 0o777;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    File,
    Dir,
}

#[derive(Clone, Debug)]
pub struct DirEntry {
    pub name:   String,
    pub kind:   NodeKind,
    pub size:   u64,
}

#[derive(Clone, Debug)]
pub struct FileStat {
    pub size:  u64,
    pub kind:  NodeKind,
    pub mode:  u32,
}

#[derive(Clone, Debug)]
pub struct FsStat {
    pub total_bytes: u64,
    pub free_bytes:  u64,
    pub fs_type:     &'static str,
}

pub trait FileSystem: Send + Sync {
    fn stat(&self, path: &str) -> Result<FileStat, VfsError>;
    fn read(&self, path: &str) -> Result<Vec<u8>, VfsError>;
    fn write(&self, path: &str, data: &[u8]) -> Result<(), VfsError>;
    fn create(&self, path: &str, data: &[u8], mode: u32) -> Result<(), VfsError>;
    fn unlink(&self, path: &str) -> Result<(), VfsError>;
    fn rename(&self, from: &str, to: &str) -> Result<(), VfsError>;
    fn mkdir(&self, path: &str, mode: u32) -> Result<(), VfsError>;
    fn rmdir(&self, path: &str) -> Result<(), VfsError>;
    fn readdir(&self, path: &str) -> Result<Vec<DirEntry>, VfsError>;
    fn stat_fs(&self) -> FsStat;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VfsError {
    NotFound,
    AlreadyExists,
    NotAFile,
    NotADir,
    NotEmpty,
    PermissionDenied,
    InvalidPath,
    NotSupported,
    IoError,
    NoSpace,
    NotMounted,
}

impl VfsError {
    pub fn as_str(self) -> &'static str {
        match self {
            VfsError::NotFound        => "not found",
            VfsError::AlreadyExists   => "already exists",
            VfsError::NotAFile        => "not a file",
            VfsError::NotADir         => "not a directory",
            VfsError::NotEmpty        => "directory not empty",
            VfsError::PermissionDenied => "permission denied",
            VfsError::InvalidPath     => "invalid path",
            VfsError::NotSupported    => "not supported",
            VfsError::IoError         => "I/O error",
            VfsError::NoSpace         => "no space left",
            VfsError::NotMounted      => "not mounted",
        }
    }
}

impl core::fmt::Display for VfsError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

struct MountEntry {
    point: String,
    fs:    Box<dyn FileSystem>,
}

struct Vfs {
    mounts: Vec<MountEntry>,
}

impl Vfs {
    const fn new_uninit() -> Self {
        Vfs {
            mounts: Vec::new(),
        }
    }

    fn find_mount(&self, point: &str) -> Option<(usize, &MountEntry)> {
        self.mounts.iter().enumerate().find(|(_, e)| e.point == point)
    }
}

static VFS_LOCK: AtomicBool = AtomicBool::new(false);
static VFS: Mutex<Vfs> = Mutex::new(Vfs::new_uninit());

fn lock() {
    while VFS_LOCK
        .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

fn unlock() {
    VFS_LOCK.store(false, Ordering::Release);
}

fn vfs() -> spin::MutexGuard<'static, Vfs> {
    VFS.lock()
}

fn split_path(path: &str) -> Result<(&str, &str), VfsError> {
    let path = path.trim_start_matches('/');
    if path.is_empty() {
        return Err(VfsError::InvalidPath);
    }
    match path.find('/') {
        Some(i) => {
            let point = &path[..i];
            let rel   = &path[i + 1..];
            if point.is_empty() || rel.is_empty() {
                Err(VfsError::InvalidPath)
            } else {
                Ok((point, rel))
            }
        }
        None => Err(VfsError::InvalidPath),
    }
}

pub fn vfs_init() {}

pub fn vfs_mount(point: &str, fs: Box<dyn FileSystem>) -> Result<(), VfsError> {
    lock();
    let mut v = vfs();
    let result = if v.find_mount(point).is_some() {
        Err(VfsError::AlreadyExists)
    } else {
        v.mounts.push(MountEntry { point: String::from(point), fs });
        Ok(())
    };
    unlock();
    result
}

pub fn vfs_unmount(point: &str) -> Result<(), VfsError> {
    lock();
    let mut v = vfs();
    let before = v.mounts.len();
    v.mounts.retain(|e| e.point != point);
    let result = if v.mounts.len() == before {
        Err(VfsError::NotMounted)
    } else {
        Ok(())
    };
    unlock();
    result
}

pub struct VfsNode {
    point: String,
    rel: String,
    flags: u32,
}

impl VfsNode {
    pub fn read(&self) -> Result<Vec<u8>, VfsError> {
        let path = format!("{}/{}", self.point, self.rel);
        vfs_read(&path)
    }

    pub fn write(&self, data: &[u8]) -> Result<(), VfsError> {
        let path = format!("{}/{}", self.point, self.rel);
        vfs_write(&path, data, self.flags, 0)
    }

    pub fn stat(&self) -> Result<FileStat, VfsError> {
        let path = format!("{}/{}", self.point, self.rel);
        vfs_stat(&path)
    }

    pub fn path(&self) -> String {
        format!("{}/{}", self.point, self.rel)
    }
}

pub fn vfs_open_node(path: &str, flags: u32, mode: u32) -> Result<VfsNode, VfsError> {
    let (point, rel) = split_path(path)?;
    lock();
    let v = vfs();
    let (_idx, entry) = v.find_mount(point).ok_or_else(|| { unlock(); VfsError::NotMounted })?;
    let fs = entry.fs.as_ref();

    let stat_res = fs.stat(rel);
    let exists = stat_res.is_ok();

    if flags & O_EXCL != 0 && flags & O_CREAT != 0 && exists {
        unlock();
        return Err(VfsError::AlreadyExists);
    }

    if flags & O_CREAT != 0 && !exists {
        fs.create(rel, &[], mode).map_err(|e| { unlock(); e })?;
    } else if !exists {
        unlock();
        return Err(VfsError::NotFound);
    }

    if flags & O_TRUNC != 0 {
        fs.write(rel, &[]).map_err(|e| { unlock(); e })?;
    }

    unlock();
    Ok(VfsNode {
        point: point.to_string(),
        rel: rel.to_string(),
        flags,
    })
}

pub fn vfs_create(path: &str, data: &[u8], mode: u32) -> Result<(), VfsError> {
    let (point, rel) = split_path(path)?;
    lock();
    let v = vfs();
    let (_idx, entry) = v.find_mount(point).ok_or_else(|| { unlock(); VfsError::NotMounted })?;
    let fs = entry.fs.as_ref();

    if fs.stat(rel).is_ok() {
        unlock();
        return Err(VfsError::AlreadyExists);
    }

    let result = fs.create(rel, data, mode).map_err(|e| { unlock(); e });
    unlock();
    result
}

pub fn vfs_read(path: &str) -> Result<Vec<u8>, VfsError> {
    let (point, rel) = split_path(path)?;
    lock();
    let v = vfs();
    let (_idx, entry) = v.find_mount(point).ok_or_else(|| { unlock(); VfsError::NotMounted })?;
    let data = entry.fs.read(rel).map_err(|e| { unlock(); e })?;
    unlock();
    Ok(data)
}

pub fn vfs_write(path: &str, data: &[u8], flags: u32, mode: u32) -> Result<(), VfsError> {
    let (point, rel) = split_path(path)?;
    lock();
    let v = vfs();
    let (_idx, entry) = v.find_mount(point).ok_or_else(|| { unlock(); VfsError::NotMounted })?;
    let fs = entry.fs.as_ref();

    let stat_res = fs.stat(rel);
    let exists = stat_res.is_ok();

    if flags & O_EXCL != 0 && flags & O_CREAT != 0 && exists {
        unlock();
        return Err(VfsError::AlreadyExists);
    }

    if flags & O_CREAT != 0 && !exists {
        fs.create(rel, &[], mode).map_err(|e| { unlock(); e })?;
    } else if !exists {
        unlock();
        return Err(VfsError::NotFound);
    }

    if (flags & 0x3) == O_RDONLY {
        unlock();
        return Err(VfsError::PermissionDenied);
    }

    let mut contents = if flags & O_APPEND != 0 && exists {
        fs.read(rel).map_err(|e| { unlock(); e })?
    } else {
        Vec::new()
    };

    if flags & O_TRUNC != 0 {
        contents.clear();
    }

    if flags & O_APPEND != 0 {
        contents.extend_from_slice(data);
    } else {
        contents = data.to_vec();
    }

    fs.write(rel, &contents).map_err(|e| { unlock(); e })?;
    unlock();
    Ok(())
}

pub fn vfs_stat(path: &str) -> Result<FileStat, VfsError> {
    let (point, rel) = split_path(path)?;
    lock();
    let v = vfs();
    let result = v.find_mount(point)
        .ok_or(VfsError::NotMounted)
        .and_then(|(_, e)| e.fs.stat(rel));
    unlock();
    result
}

pub fn vfs_file_exists(path: &str) -> bool {
    match vfs_stat(path) {
        Ok(_) => true,
        Err(_) => false,
    }
}

pub fn vfs_readdir(path: &str) -> Result<Vec<DirEntry>, VfsError> {
    let (point, rel) = split_path(path)?;
    lock();
    let v = vfs();
    let result = v.find_mount(point)
        .ok_or(VfsError::NotMounted)
        .and_then(|(_, e)| e.fs.readdir(rel));
    unlock();
    result
}