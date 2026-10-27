//! Files. Data is always copied between the kernel and the program's
//! buffers, never handed out by reference.

use crate::raw::{syscall, syscall_noptr};
use alloc::string::String;
use alloc::vec::Vec;
use pafos_abi::{Error, FileInfo, FsStats, nr};

pub use pafos_abi::Mode;

/// An open file, closed automatically when dropped.
pub struct File {
    fd: usize,
}

impl File {
    /// Opens an existing file; never creates one (see [`create`]).
    pub fn open(name: &str, mode: Mode) -> Result<File, Error> {
        let fd = unsafe { syscall(nr::OPEN, [name.as_ptr() as usize, name.len(), mode as usize, 0])? };
        Ok(File { fd })
    }

    /// Reads up to `buf.len()` bytes; returns 0 at end of file.
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        unsafe { syscall(nr::READ, [self.fd, buf.as_mut_ptr() as usize, buf.len(), 0]) }
    }

    pub fn write(&mut self, data: &[u8]) -> Result<usize, Error> {
        unsafe { syscall(nr::WRITE, [self.fd, data.as_ptr() as usize, data.len(), 0]) }
    }

    pub fn read_to_end(&mut self) -> Result<Vec<u8>, Error> {
        let mut out = Vec::new();
        let mut buf = [0u8; 128];
        loop {
            let n = self.read(&mut buf)?;
            if n == 0 {
                return Ok(out);
            }
            out.extend_from_slice(&buf[..n]);
        }
    }
}

impl Drop for File {
    fn drop(&mut self) {
        let _ = syscall_noptr(nr::CLOSE, [self.fd, 0, 0, 0]);
    }
}

/// Creates an empty file; fails with `AlreadyExists` if the name is taken.
pub fn create(name: &str) -> Result<(), Error> {
    unsafe { syscall(nr::CREATE, [name.as_ptr() as usize, name.len(), 0, 0]) }.map(drop)
}

pub fn remove(name: &str) -> Result<(), Error> {
    unsafe { syscall(nr::UNLINK, [name.as_ptr() as usize, name.len(), 0, 0]) }.map(drop)
}

/// Whole file as text; non-ASCII bytes become `?`.
pub fn read_to_string(name: &str) -> Result<String, Error> {
    let bytes = File::open(name, Mode::Read)?.read_to_end()?;
    Ok(bytes.iter().map(|&b| if b.is_ascii() { b as char } else { '?' }).collect())
}

/// The `index`-th file in name order, or `None` past the last one.
pub fn entry_at(index: usize) -> Option<FileInfo> {
    let mut info = FileInfo::EMPTY;
    let filled = unsafe { syscall(nr::READDIR, [index, &raw mut info as usize, 0, 0]) };
    (filled == Ok(1)).then_some(info)
}

pub fn stats() -> Result<FsStats, Error> {
    let mut stats = FsStats::default();
    unsafe { syscall(nr::FSSTAT, [&raw mut stats as usize, 0, 0, 0])? };
    Ok(stats)
}
