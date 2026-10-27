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
use pafos_abi::{Error, FileInfo, FsStats, MAX_NAME_LEN, Mode, Name};
use spin::Mutex;

pub const TMPFS_START: usize = 0x_5555_5555_0000;
pub const TMPFS_SIZE: usize = 1024 * 1024; // 1 MiB

pub const BLOCK_SIZE: usize = 512;
// Hard limits keep the worst-case heap usage of the metadata small and known.
pub const MAX_FILES: usize = 64;
pub const MAX_OPEN: usize = 16;

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

fn with_fs<T>(f: impl FnOnce(&mut Tmpfs) -> Result<T, Error>) -> Result<T, Error> {
    FS.lock().as_mut().map_or(Err(Error::Unavailable), f)
}

fn validate_name(name: &str) -> Result<(), Error> {
    let valid = !name.is_empty()
        && name.len() <= MAX_NAME_LEN
        && name.bytes().all(|b| b.is_ascii_graphic() && b != b'/');
    if valid { Ok(()) } else { Err(Error::InvalidName) }
}

impl Tmpfs {
    fn is_open(&self, name: &str) -> bool {
        self.open_files.iter().flatten().any(|f| f.name == name)
    }

    /// Files only come into existence here; `open` never creates them.
    fn create(&mut self, name: &str) -> Result<(), Error> {
        validate_name(name)?;
        if self.files.contains_key(name) {
            return Err(Error::AlreadyExists);
        }
        if self.files.len() >= MAX_FILES {
            return Err(Error::TooManyFiles);
        }
        self.files.insert(String::from(name), Inode { size: 0, blocks: Vec::new() });
        Ok(())
    }

    fn open(&mut self, name: &str, mode: Mode) -> Result<usize, Error> {
        validate_name(name)?;
        // One open handle per file keeps the semantics trivial: no concurrent writers.
        if self.is_open(name) {
            return Err(Error::Busy);
        }
        let fd = self
            .open_files
            .iter()
            .position(Option::is_none)
            .ok_or(Error::TooManyOpen)?;

        let inode = self.files.get_mut(name).ok_or(Error::NotFound)?;
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

    fn read(&mut self, fd: usize, buf: &mut [u8]) -> Result<usize, Error> {
        let file = self.open_files.get_mut(fd).and_then(Option::as_mut).ok_or(Error::BadFd)?;
        if file.mode != Mode::Read {
            return Err(Error::WrongMode);
        }
        let inode = self.files.get(&file.name).ok_or(Error::NotFound)?;

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

    fn write(&mut self, fd: usize, data: &[u8]) -> Result<usize, Error> {
        let file = self.open_files.get_mut(fd).and_then(Option::as_mut).ok_or(Error::BadFd)?;
        if file.mode == Mode::Read {
            return Err(Error::WrongMode);
        }
        let inode = self.files.get_mut(&file.name).ok_or(Error::NotFound)?;

        // Reserve every block up front, so a write either fully succeeds or changes nothing.
        let end = file.pos + data.len();
        let needed = end.div_ceil(BLOCK_SIZE).saturating_sub(inode.blocks.len());
        if needed > self.blocks.free_count {
            return Err(Error::NoSpace);
        }
        for _ in 0..needed {
            let block = self.blocks.alloc().ok_or(Error::NoSpace)?;
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

    fn close(&mut self, fd: usize) -> Result<(), Error> {
        self.open_files.get_mut(fd).and_then(Option::take).map(|_| ()).ok_or(Error::BadFd)
    }

    fn remove(&mut self, name: &str) -> Result<(), Error> {
        validate_name(name)?;
        if self.is_open(name) {
            return Err(Error::Busy);
        }
        let inode = self.files.remove(name).ok_or(Error::NotFound)?;
        for block in inode.blocks {
            self.blocks.release(block);
        }
        Ok(())
    }

    fn file_at(&self, index: usize) -> Option<FileInfo> {
        let (name, inode) = self.files.iter().nth(index)?;
        Some(FileInfo { name: Name::new(name), size: inode.size })
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

pub fn create(name: &str) -> Result<(), Error> {
    with_fs(|fs| fs.create(name))
}

pub fn open(name: &str, mode: Mode) -> Result<usize, Error> {
    with_fs(|fs| fs.open(name, mode))
}

pub fn read(fd: usize, buf: &mut [u8]) -> Result<usize, Error> {
    with_fs(|fs| fs.read(fd, buf))
}

pub fn write(fd: usize, data: &[u8]) -> Result<usize, Error> {
    with_fs(|fs| fs.write(fd, data))
}

pub fn close(fd: usize) -> Result<(), Error> {
    with_fs(|fs| fs.close(fd))
}

pub fn remove(name: &str) -> Result<(), Error> {
    with_fs(|fs| fs.remove(name))
}

pub fn file_at(index: usize) -> Option<FileInfo> {
    FS.lock().as_ref()?.file_at(index)
}

pub fn stats() -> Result<FsStats, Error> {
    with_fs(|fs| Ok(fs.stats()))
}
