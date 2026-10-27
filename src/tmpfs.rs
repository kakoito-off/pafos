//! In-memory file system.
//!
//! File *data* lives in a dedicated, separately mapped memory region cut into
//! fixed-size blocks, with a bitmap tracking which blocks are used. File
//! *metadata* (names, per-file block lists, open-file table) lives on the
//! kernel heap. Running out of blocks is a normal `NoSpace` error instead of a
//! heap exhaustion panic.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;
use spin::Mutex;

pub const TMPFS_START: usize = 0x_5555_5555_0000;
pub const TMPFS_SIZE: usize = 1024 * 1024; // 1 MiB

pub const BLOCK_SIZE: usize = 512;
// Hard limits keep the worst-case heap usage of the metadata small and known.
pub const MAX_FILES: usize = 64;
pub const MAX_OPEN: usize = 16;
pub const MAX_NAME_LEN: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Read an existing file from the start.
    Read,
    /// Create the file if needed and truncate it to zero length.
    Write,
    /// Create the file if needed and write at its end.
    Append,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    Unavailable,
    NotFound,
    InvalidName,
    TooManyFiles,
    TooManyOpen,
    Busy,
    NoSpace,
    BadFd,
    WrongMode,
}

impl fmt::Display for FsError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let msg = match self {
            FsError::Unavailable => "file system not initialized",
            FsError::NotFound => "no such file",
            FsError::InvalidName => "invalid file name",
            FsError::TooManyFiles => "too many files",
            FsError::TooManyOpen => "too many open files",
            FsError::Busy => "file is open",
            FsError::NoSpace => "no space left",
            FsError::BadFd => "bad file descriptor",
            FsError::WrongMode => "file not opened in this mode",
        };
        f.write_str(msg)
    }
}

/// Plain-old-data description of a file, safe to copy out to a program.
#[derive(Clone, Copy)]
pub struct FileInfo {
    name: [u8; MAX_NAME_LEN],
    name_len: usize,
    pub size: usize,
}

impl FileInfo {
    pub fn name(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("?")
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FsStats {
    pub block_size: usize,
    pub total_blocks: usize,
    pub free_blocks: usize,
    pub files: usize,
}

struct Blocks {
    data: &'static mut [u8],
    bitmap: Vec<u64>,
    total: usize,
    free_count: usize,
}

impl Blocks {
    fn alloc(&mut self) -> Option<usize> {
        for (word_idx, word) in self.bitmap.iter_mut().enumerate() {
            if *word == u64::MAX {
                continue;
            }
            let bit = (!*word).trailing_zeros() as usize;
            let block = word_idx * 64 + bit;
            if block >= self.total {
                return None;
            }
            *word |= 1 << bit;
            self.free_count -= 1;
            return Some(block);
        }
        None
    }

    fn release(&mut self, block: usize) {
        self.bitmap[block / 64] &= !(1 << (block % 64));
        self.free_count += 1;
    }

    fn block(&self, block: usize) -> &[u8] {
        &self.data[block * BLOCK_SIZE..(block + 1) * BLOCK_SIZE]
    }

    fn block_mut(&mut self, block: usize) -> &mut [u8] {
        &mut self.data[block * BLOCK_SIZE..(block + 1) * BLOCK_SIZE]
    }
}

struct Inode {
    size: usize,
    blocks: Vec<usize>,
}

struct OpenFile {
    name: String,
    mode: Mode,
    pos: usize,
}

struct Tmpfs {
    blocks: Blocks,
    files: BTreeMap<String, Inode>,
    open_files: Vec<Option<OpenFile>>,
}

static FS: Mutex<Option<Tmpfs>> = Mutex::new(None);

/// # Safety
/// `start..start + size` must be mapped, writable, and used by nothing else.
/// The heap must already be initialized.
pub unsafe fn init(start: usize, size: usize) {
    let data = unsafe { core::slice::from_raw_parts_mut(start as *mut u8, size) };
    let total = size / BLOCK_SIZE;
    *FS.lock() = Some(Tmpfs {
        blocks: Blocks {
            data,
            bitmap: vec![0; total.div_ceil(64)],
            total,
            free_count: total,
        },
        files: BTreeMap::new(),
        open_files: (0..MAX_OPEN).map(|_| None).collect(),
    });
}

fn with_fs<T>(f: impl FnOnce(&mut Tmpfs) -> Result<T, FsError>) -> Result<T, FsError> {
    FS.lock().as_mut().map_or(Err(FsError::Unavailable), f)
}

fn validate_name(name: &str) -> Result<(), FsError> {
    let valid = !name.is_empty()
        && name.len() <= MAX_NAME_LEN
        && name.bytes().all(|b| b.is_ascii_graphic() && b != b'/');
    if valid { Ok(()) } else { Err(FsError::InvalidName) }
}

impl Tmpfs {
    fn is_open(&self, name: &str) -> bool {
        self.open_files.iter().flatten().any(|f| f.name == name)
    }

    fn open(&mut self, name: &str, mode: Mode) -> Result<usize, FsError> {
        validate_name(name)?;
        // One open handle per file keeps the semantics trivial: no concurrent writers.
        if self.is_open(name) {
            return Err(FsError::Busy);
        }
        let fd = self
            .open_files
            .iter()
            .position(Option::is_none)
            .ok_or(FsError::TooManyOpen)?;

        if !self.files.contains_key(name) {
            if mode == Mode::Read {
                return Err(FsError::NotFound);
            }
            if self.files.len() >= MAX_FILES {
                return Err(FsError::TooManyFiles);
            }
            self.files.insert(String::from(name), Inode { size: 0, blocks: Vec::new() });
        }

        let inode = self.files.get_mut(name).ok_or(FsError::NotFound)?;
        let pos = match mode {
            Mode::Read => 0,
            Mode::Append => inode.size,
            Mode::Write => {
                for block in inode.blocks.drain(..) {
                    self.blocks.release(block);
                }
                inode.size = 0;
                0
            }
        };

        self.open_files[fd] = Some(OpenFile { name: String::from(name), mode, pos });
        Ok(fd)
    }

    fn read(&mut self, fd: usize, buf: &mut [u8]) -> Result<usize, FsError> {
        let file = self.open_files.get_mut(fd).and_then(Option::as_mut).ok_or(FsError::BadFd)?;
        if file.mode != Mode::Read {
            return Err(FsError::WrongMode);
        }
        let inode = self.files.get(&file.name).ok_or(FsError::NotFound)?;

        let len = buf.len().min(inode.size - file.pos);
        let mut done = 0;
        while done < len {
            let offset = file.pos + done;
            let (block_idx, in_block) = (offset / BLOCK_SIZE, offset % BLOCK_SIZE);
            let chunk = (BLOCK_SIZE - in_block).min(len - done);
            let block = self.blocks.block(inode.blocks[block_idx]);
            buf[done..done + chunk].copy_from_slice(&block[in_block..in_block + chunk]);
            done += chunk;
        }
        file.pos += len;
        Ok(len)
    }

    fn write(&mut self, fd: usize, data: &[u8]) -> Result<usize, FsError> {
        let file = self.open_files.get_mut(fd).and_then(Option::as_mut).ok_or(FsError::BadFd)?;
        if file.mode == Mode::Read {
            return Err(FsError::WrongMode);
        }
        let inode = self.files.get_mut(&file.name).ok_or(FsError::NotFound)?;

        // Reserve every block up front, so a write either fully succeeds or changes nothing.
        let end = file.pos + data.len();
        let needed = end.div_ceil(BLOCK_SIZE).saturating_sub(inode.blocks.len());
        if needed > self.blocks.free_count {
            return Err(FsError::NoSpace);
        }
        for _ in 0..needed {
            let block = self.blocks.alloc().ok_or(FsError::NoSpace)?;
            inode.blocks.push(block);
        }

        let mut done = 0;
        while done < data.len() {
            let offset = file.pos + done;
            let (block_idx, in_block) = (offset / BLOCK_SIZE, offset % BLOCK_SIZE);
            let chunk = (BLOCK_SIZE - in_block).min(data.len() - done);
            let block = self.blocks.block_mut(inode.blocks[block_idx]);
            block[in_block..in_block + chunk].copy_from_slice(&data[done..done + chunk]);
            done += chunk;
        }
        file.pos = end;
        inode.size = inode.size.max(end);
        Ok(data.len())
    }

    fn close(&mut self, fd: usize) -> Result<(), FsError> {
        self.open_files.get_mut(fd).and_then(Option::take).map(|_| ()).ok_or(FsError::BadFd)
    }

    fn remove(&mut self, name: &str) -> Result<(), FsError> {
        validate_name(name)?;
        if self.is_open(name) {
            return Err(FsError::Busy);
        }
        let inode = self.files.remove(name).ok_or(FsError::NotFound)?;
        for block in inode.blocks {
            self.blocks.release(block);
        }
        Ok(())
    }

    fn file_at(&self, index: usize) -> Option<FileInfo> {
        let (name, inode) = self.files.iter().nth(index)?;
        let mut info = FileInfo { name: [0; MAX_NAME_LEN], name_len: name.len(), size: inode.size };
        info.name[..name.len()].copy_from_slice(name.as_bytes());
        Some(info)
    }

    fn stats(&self) -> FsStats {
        FsStats {
            block_size: BLOCK_SIZE,
            total_blocks: self.blocks.total,
            free_blocks: self.blocks.free_count,
            files: self.files.len(),
        }
    }
}

pub fn open(name: &str, mode: Mode) -> Result<usize, FsError> {
    with_fs(|fs| fs.open(name, mode))
}

pub fn read(fd: usize, buf: &mut [u8]) -> Result<usize, FsError> {
    with_fs(|fs| fs.read(fd, buf))
}

pub fn write(fd: usize, data: &[u8]) -> Result<usize, FsError> {
    with_fs(|fs| fs.write(fd, data))
}

pub fn close(fd: usize) -> Result<(), FsError> {
    with_fs(|fs| fs.close(fd))
}

pub fn remove(name: &str) -> Result<(), FsError> {
    with_fs(|fs| fs.remove(name))
}

pub fn file_at(index: usize) -> Option<FileInfo> {
    FS.lock().as_ref()?.file_at(index)
}

pub fn stats() -> Result<FsStats, FsError> {
    with_fs(|fs| Ok(fs.stats()))
}
